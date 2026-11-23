# Pacote MSIX (Microsoft Store)

A Store assina o pacote, então ele vai **sem assinatura** para o Partner Center.

## Gerar

```powershell
cd app; npx tauri build; cd ..
.\scripts\msix.ps1            # target/msix/Larafim_X.Y.Z.0_x64.msix
```

Na release (tag `vX.Y.Z`), o workflow gera o mesmo arquivo em *Actions › execução › Artifacts*
(`larafim-msix-vX.Y.Z`). Ele não vai para a página da release, porque sem assinatura não instala
fora da Store.

## Testar neste PC antes de enviar

```powershell
.\scripts\msix.ps1 -Sign
```

O script cria um certificado de teste com o mesmo `Publisher` do manifesto e assina o pacote. Depois
você confia no certificado uma vez (PowerShell como administrador) e instala, como o script
mostra no fim. O que conferir:

- `php -v` num terminal novo, com os atalhos no PATH (Configurações › Terminal);
- um endereço `.offline` (pede UAC para o arquivo `hosts`);
- "Iniciar com o Windows" (reinicie e veja o ícone na bandeja);
- Configurações › Sobre mostra "Atualizado pela Microsoft Store".

Para remover: `Get-AppxPackage jotav.larafim | Remove-AppxPackage`. Os dados em `%LOCALAPPDATA%\Larafim`
ficam, como no instalador NSIS.

## Como o app muda dentro do pacote

| | Instalador NSIS | MSIX |
|---|---|---|
| Iniciar com o Windows / UAC do `hosts` | caminho do `larafim-app.exe` | alias `larafim.exe` (caminho fixo entre versões) |
| Verificar versão nova no GitHub | sim | não (a Store atualiza) |
| Idioma inicial | escolhido no instalador | idioma do Windows |

Detecção em `app/src-tauri/src/packaged.rs`.

## Capacidades restritas: justificativa no envio

No Partner Center, em *Pacotes › Submission options › Restricted capabilities*, cole:

> Larafim is a local development tool (PHP, Node.js and databases for web projects).
> **unvirtualizedResources**: it installs PHP/Node runtimes under %LOCALAPPDATA%\Larafim and adds
> their command-line shims to the user's PATH (HKCU\Environment). Both must be the real locations,
> because the user's own terminal, which runs outside the package, needs to find them.
> **allowElevation**: only to add the app's own block of `127.0.0.1 <project>.offline` lines to the
> Windows hosts file, after the user confirms the UAC prompt. Nothing else runs elevated.
> **runFullTrust**: desktop app (Tauri/WebView2) that starts local web servers for the user's projects.
