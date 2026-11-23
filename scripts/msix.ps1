# Monta o pacote MSIX da Microsoft Store a partir do que o `tauri build` já
# compilou (target/release/larafim-app.exe e larafim-shim.exe).
#
#   .\scripts\msix.ps1          # pacote sem assinatura: é o que vai para a Store (ela assina)
#   .\scripts\msix.ps1 -Sign    # assina com um certificado de teste, só para instalar neste PC
#
# Precisa do Windows SDK (MakeAppx.exe e, com -Sign, SignTool.exe).
param(
    [switch]$Sign
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root "target/release"
$outDir = Join-Path $root "target/msix"
$stage = Join-Path $outDir "stage"
$template = Join-Path $root "packaging/msix/AppxManifest.xml"
$icons = Join-Path $root "app/src-tauri/icons"

# A Store exige versão X.Y.Z.0 (o último número é reservado para ela).
$version = (Get-Content (Join-Path $root "app/src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "versão inesperada em tauri.conf.json: $version" }
$msixVersion = "$version.0"

foreach ($exe in "larafim-app.exe", "larafim-shim.exe") {
    if (-not (Test-Path (Join-Path $release $exe))) {
        throw "$exe não encontrado em $release. Rode antes: cd app; npx tauri build"
    }
}

function Find-SdkTool([string]$name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
    $tool = Get-ChildItem $kits -Directory -Filter "10.*" -ErrorAction SilentlyContinue |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName "x64/$name" } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
    if (-not $tool) { throw "$name não encontrado. Instale o Windows SDK." }
    $tool
}

# Pasta do pacote: executáveis na raiz (o Larafim procura o larafim-shim.exe
# ao lado de si) e os ícones em Assets.
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage "Assets") | Out-Null
Copy-Item (Join-Path $release "larafim-app.exe") $stage
Copy-Item (Join-Path $release "larafim-shim.exe") $stage
foreach ($icon in "StoreLogo.png", "Square44x44Logo.png", "Square150x150Logo.png") {
    Copy-Item (Join-Path $icons $icon) (Join-Path $stage "Assets")
}
(Get-Content $template -Raw -Encoding utf8).Replace("{VERSION}", $msixVersion) |
    Set-Content (Join-Path $stage "AppxManifest.xml") -Encoding utf8 -NoNewline

$msix = Join-Path $outDir "Larafim_${msixVersion}_x64.msix"
$makeappx = Find-SdkTool "makeappx.exe"
& $makeappx pack /o /d $stage /p $msix | Out-Host
if ($LASTEXITCODE -ne 0) { throw "MakeAppx falhou ($LASTEXITCODE)" }

if ($Sign) {
    # O assunto do certificado tem de ser igual ao Publisher do manifesto.
    $publisher = ([xml](Get-Content $template -Raw)).Package.Identity.Publisher
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $publisher -and $_.HasPrivateKey } |
        Sort-Object NotAfter -Descending | Select-Object -First 1
    if (-not $cert) {
        Write-Host "Criando certificado de teste ($publisher) em Cert:\CurrentUser\My"
        $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature `
            -FriendlyName "Larafim MSIX (teste local)" -CertStoreLocation Cert:\CurrentUser\My `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    }
    $signtool = Find-SdkTool "signtool.exe"
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "SignTool falhou ($LASTEXITCODE)" }

    $cer = Join-Path $outDir "larafim-teste.cer"
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    Write-Host ""
    Write-Host "Para instalar neste PC, confie no certificado de teste uma vez (PowerShell como administrador):"
    Write-Host "  Import-Certificate -FilePath `"$cer`" -CertStoreLocation Cert:\LocalMachine\TrustedPeople"
    Write-Host "Depois: Add-AppxPackage `"$msix`""
}

Write-Host ""
Write-Host "Pacote: $msix"
