# Publicar uma versão

## CI

[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) roda em cada push na `main` e em cada pull
request, no Windows:

1. `npm ci`, traduções completas (`i18n.mjs check --strict`) e tipos da interface (`npm run check`);
2. compila a interface e o `larafim-shim`;
3. `cargo clippy` sem avisos;
4. `cargo test --workspace` (inclui os testes de segurança).

Um segundo job audita as dependências contra vulnerabilidades conhecidas (`cargo audit` e
`npm audit --audit-level=high`).

O [Dependabot](../../.github/dependabot.yml) abre pull requests semanais com atualizações de Rust, npm e
das próprias Actions.

## Release

[`.github/workflows/release.yml`](../../.github/workflows/release.yml) roda quando uma tag `vX.Y.Z` é
enviada:

1. confere se a tag bate com a `version` de `app/src-tauri/tauri.conf.json`;
2. roda os testes de novo;
3. confere se a chave pública do updater está no `tauri.conf.json`;
4. `npx tauri build`, que gera o instalador NSIS e o `.sig` dele, assinado com a chave privada do
   updater (veja [Atualização pelo app](#atualização-pelo-app));
5. gera o `SHA256SUMS.txt` e o `latest.json`;
6. cria a **atestação de origem** (*build provenance*) do instalador, que qualquer pessoa confere com
   `gh attestation verify`;
7. publica a release com o instalador, o `.sig`, o `latest.json`, o `SHA256SUMS.txt` e as notas
   (seção do `CHANGELOG.md` + como instalar + lista de PRs).

### Passo a passo

1. Atualize a versão em `app/src-tauri/tauri.conf.json`, `app/package.json` e no
   `[workspace.package]` do `Cargo.toml` (as três iguais).
2. Escreva a seção `## vX.Y.Z` no [`CHANGELOG.md`](../../CHANGELOG.md): destaques, correções e o que
   a pessoa precisa saber. Esse texto vira o começo da release. O workflow acrescenta "Instalar" (com o
   SHA-256 do instalador) e a lista de PRs agrupada por categoria
   ([`.github/release.yml`](../../.github/release.yml)). Sem essa seção, o workflow falha.
3. Faça o commit e espere o CI ficar verde.
4. Crie e envie a tag:

   ```powershell
   git tag v0.2.0
   git push origin v0.2.0
   ```

5. Quando o workflow terminar, confira a release no GitHub (notas e arquivos).
6. Atualize o Larafim no winget (veja abaixo).

Dá para recompilar uma tag existente em *Actions › Release › Run workflow*.

## winget

Os manifestos ficam em [`packaging/winget`](../../packaging/winget), no mesmo formato de pastas do
repositório [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs). O ID do pacote é
`Larafim.Larafim`.

O manifesto leva o SHA-256 do instalador, então só dá para fazê-lo **depois** que a release sai.

**Primeiro envio:** valide e envie a pasta da versão com o
[`wingetcreate`](https://github.com/microsoft/winget-create). Ele pede um token do GitHub com
`public_repo`, que você passa só na linha de comando, sem salvar em nenhum arquivo.

```powershell
winget validate --manifest .\packaging\winget\manifests\l\Larafim\Larafim\0.1.1
wingetcreate submit --token SEU_TOKEN .\packaging\winget\manifests\l\Larafim\Larafim\0.1.1
```

**Versões seguintes:** com o pacote já no winget, o `wingetcreate` gera o manifesto novo sozinho a
partir da URL do instalador:

```powershell
wingetcreate update Larafim.Larafim --version 0.2.0 --urls https://github.com/josevictoremiliano/larafim/releases/download/v0.2.0/Larafim_0.2.0_x64-setup.exe --submit --token SEU_TOKEN
```

Um robô valida o pull request e um moderador aprova (alguns dias).

## macOS e Linux

Não há builds para macOS e Linux. O app depende de partes do Windows (veja
[Arquitetura › Outras plataformas](arquitetura.md#outras-plataformas)). Um instalador dessas plataformas
abriria, mas não conseguiria baixar nem rodar PHP, bancos ou domínios. Por isso o workflow só publica o
instalador de Windows.

## Assinatura de código

O instalador **não é assinado** com certificado de código: os certificados reconhecidos pelo Windows
são pagos. Por isso o SmartScreen avisa na primeira instalação, e a confiança vem do `SHA256SUMS.txt` e
da atestação de origem. Na Microsoft Store não há aviso, porque a própria Store assina o pacote MSIX.

Isto é diferente da assinatura das **atualizações** (abaixo), que é gratuita e já é usada.

## Atualização pelo app

O Larafim instalado pelo NSIS se atualiza quando a pessoa clica em *Atualizar e reiniciar*. Ele usa o
*updater* do Tauri: lê o `latest.json` da última release (`releases/latest/download/latest.json`),
baixa o instalador, confere a assinatura com a chave pública embutida (`plugins.updater.pubkey` no
`tauri.conf.json`) e roda o instalador com `/P /UPDATE /R`: passivo, sem a tela de desinstalar a versão
anterior, reabrindo o app no fim. Os *hooks* do instalador (`installer-hooks.nsh`) não mexem na chave
de idioma do registro em modo de atualização.

A assinatura usa um par de chaves **minisign**, gratuito, sem relação com certificado de código.

### Configurar a chave (uma vez)

1. Gere o par de chaves **fora do repositório**, com uma senha:

   ```powershell
   cd app
   npx tauri signer generate -w "$env:USERPROFILE\.tauri\larafim.key"
   ```

2. Copie o conteúdo de `larafim.key.pub` para `plugins.updater.pubkey` em
   `app/src-tauri/tauri.conf.json`, no lugar de `CHAVE_PUBLICA_DO_UPDATER`. A chave pública pode ir para
   o repositório.
3. Em *Settings › Secrets and variables › Actions* do repositório, crie:
   - `TAURI_SIGNING_PRIVATE_KEY`: o conteúdo de `larafim.key`;
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: a senha.
4. Guarde `larafim.key` e a senha num lugar seguro (gerenciador de senhas). **Sem essa chave não dá para
   publicar atualizações para quem já tem o app**: trocar a chave obriga todo mundo a reinstalar pelo
   instalador.

Sem a chave pública ou sem o *secret*, o workflow de release falha antes de publicar.

Para testar um build com updater na sua máquina, defina `TAURI_SIGNING_PRIVATE_KEY` (caminho ou
conteúdo da chave) e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` no terminal antes do `npx tauri build`.
Arquivos `.env` não funcionam para isso.
