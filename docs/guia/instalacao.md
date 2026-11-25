# Instalação

## Requisitos

- Windows 10 ou 11, 64 bits.
- WebView2 (já vem no Windows 11; no Windows 10 o instalador baixa se faltar).
- Para o PHP: o *Visual C++ Redistributable* da Microsoft. Se faltar, o Larafim avisa e mostra o
  link oficial.

## Baixar

Os instaladores ficam na [página de releases](https://github.com/josevictoremiliano/larafim/releases/latest).
Baixe o `Larafim_<versão>_x64-setup.exe`.

Baixe **só** de lá. O Larafim não é distribuído em outros sites.

## Conferir o download

Cada release traz o `SHA256SUMS.txt` e uma **atestação de origem** do GitHub. A atestação prova que o
instalador foi compilado pelo workflow público deste repositório, a partir do código da tag.

Pelo hash, no PowerShell:

```powershell
Get-FileHash .\Larafim_0.1.1_x64-setup.exe -Algorithm SHA256
```

O resultado precisa ser igual à linha do arquivo no `SHA256SUMS.txt`.

Pela atestação, com o [GitHub CLI](https://cli.github.com/):

```powershell
gh attestation verify .\Larafim_0.1.1_x64-setup.exe --repo josevictoremiliano/larafim
```

## Instalar

O instalador:

- instala **só para o seu usuário** (em `%LOCALAPPDATA%\Programs`), sem pedir administrador;
- pergunta o idioma (português ou inglês; dá para trocar depois em *Configurações › Geral*);
- instala junto o `larafim-shim.exe`, usado pelos atalhos de terminal.

### Aviso do SmartScreen

O instalador não tem assinatura digital de código (o certificado é pago). Por isso o Windows pode mostrar
"O Windows protegeu o computador". Confira o download como explicado acima e clique em
*Mais informações › Executar assim mesmo*.

## Onde ficam os dados

Tudo o que o Larafim baixa e cria fica em `%LOCALAPPDATA%\Larafim`:

| Pasta | Conteúdo |
|---|---|
| `config.toml` | Preferências |
| `projects.toml` | Projetos importados e a configuração de cada um |
| `runtimes\` | PHP, Node, MariaDB, PostgreSQL, Mailpit, RustFS, Composer, Adminer |
| `data\` | Bancos de dados, storage S3, caixa do Mailpit, certificados locais |
| `backups\` | Backups de banco por projeto |
| `bin\` | Atalhos `php`/`composer`/`node` do terminal |
| `logs\` | Logs do próprio Larafim |

**Nada é gravado dentro dos seus projetos**, exceto quando você pede. Por exemplo, ao aplicar um
serviço no `.env`, o Larafim guarda antes uma cópia em `.env.larafim.bak`.

## Atualizar

O Larafim avisa quando sai uma versão nova. Ele confere a última release do GitHub ao abrir e a cada
6 horas, e o aviso aparece numa barra no topo da janela principal, no painel da bandeja, na dica do
ícone e em *Configurações › Sobre*. Lá também ficam o botão **Verificar atualizações** e a opção de
desligar a verificação automática.

Para atualizar, clique em **Atualizar e reiniciar** (na barra do topo, em *Configurações › Sobre* ou
no painel da bandeja). O Larafim baixa o instalador da release, confere a assinatura, fecha, instala a
versão nova e abre de novo sozinho, sem perguntar nada no caminho. Configurações, projetos e bancos são
mantidos. Nada é baixado sem esse clique.

Se preferir, baixe o instalador na página da release (botão **Novidades**) e rode por cima.

> A atualização pelo app existe a partir da v0.1.3. Quem está na v0.1.2 ou antes instala a v0.1.3 pelo
> instalador uma última vez.

## Desinstalar

Use *Configurações do Windows › Aplicativos › Larafim › Desinstalar*. O desinstalador remove o
programa e a preferência de idioma. Seus dados em `%LOCALAPPDATA%\Larafim` **ficam**, para não
apagar bancos por engano. Para remover tudo:

1. Em *Configurações › Terminal* do Larafim, desligue os atalhos no PATH (antes de desinstalar).
2. Desinstale.
3. Apague a pasta `%LOCALAPPDATA%\Larafim`.
4. Se tiver confiado no certificado HTTPS local, remova-o em `certmgr.msc` ›
   *Autoridades de Certificação Raiz Confiáveis* (procure por "Larafim").
5. Se quiser, apague o bloco `# BEGIN LARAFIM` … `# END LARAFIM` do arquivo
   `C:\Windows\System32\drivers\etc\hosts`.
