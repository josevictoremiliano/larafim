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

Na submissão, a seção *Funcionalidades restritas* tem uma caixa por capacidade, com **limite de 500
caracteres**. Cole cada bloco abaixo na caixa correspondente. A explicação completa (por que, alternativas,
caminhos gravados, proteções, suporte e garantia) fica em [CAPABILITIES.md](CAPABILITIES.md), que os
textos linkam no `main`: ele precisa estar no GitHub antes do envio.

A primeira versão foi recusada no envio da 0.1.2 (política 10.6.3, "forneça informações novas ou
ampliadas"). Se mudar o comportamento do hosts, do PATH ou da pasta de dados, atualize os dois arquivos.

### runFullTrust

```text
Open-source dev tool (Tauri/WebView2) that installs PHP, Node.js and databases and runs them as local processes (PHP-FastCGI, MariaDB, PostgreSQL, Redis, Mailpit) plus an HTTP gateway on 127.0.0.1 only. Needs a full-trust Win32 process. Details: https://github.com/josevictoremiliano/larafim/blob/main/packaging/msix/CAPABILITIES.md
```

### unvirtualizedResources

```text
The user's terminal runs outside the package and must find the PHP/Node Larafim installs. Writes go ONLY to %LOCALAPPDATA%\Larafim and, if the user opts in (off by default), one entry %LOCALAPPDATA%\Larafimin in HKCU\Environment Path. No HKLM. Alternatives, safeguards, support: https://github.com/josevictoremiliano/larafim/blob/main/packaging/msix/CAPABILITIES.md
I confirm it is used only for this and writes only to these paths.
```

### allowElevation

```text
Only to add "127.0.0.1 <project>.offline" lines to the hosts file, after the user accepts UAC. Writes ONLY Larafim's own block in etc\hosts plus a one-time hosts.larafim-backup. Names are validated twice (also in the helper); IP is always 127.0.0.1. Main app never elevated. Alternatives, safeguards, support: https://github.com/josevictoremiliano/larafim/blob/main/packaging/msix/CAPABILITIES.md
I confirm it is used only for this and writes only to these paths.
```
