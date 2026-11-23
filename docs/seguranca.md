# Segurança

Este documento explica o que o Larafim protege, como e quais testes garantem cada proteção. Para
reportar uma falha, veja [SECURITY.md](../SECURITY.md).

## Modelo de ameaças

O Larafim roda com os privilégios do seu usuário e baixa e executa programas (PHP, Node, bancos).
Os riscos que ele trata:

| Origem | Exemplo de ataque | Onde é barrado |
|---|---|---|
| **Internet** (catálogos e downloads) | Catálogo adulterado aponta para outro servidor ou usa `../` na versão; zip com arquivos fora da pasta ou "zip bomb" | [Downloads](#downloads) |
| **Arquivos de projetos** (repositórios clonados) | Nome de pasta ou valor que injeta linhas no `hosts` ou no `.env`; nome que vira SQL | [Arquivo hosts](#arquivo-hosts), [.env](#env), [Bancos](#bancos-de-dados) |
| **Navegador** (requisições ao servidor local) | `/../`, `/.env`, `/info.php.` para ler código; HTML/JS injetado nas páginas do Larafim | [Servidor local](#servidor-local) |
| **Outros programas e sites** | Usar o servidor local ou os bancos a partir da rede | Tudo escuta só em `127.0.0.1` |
| **Interface** | Script injetado na janela do app chamando comandos do Rust | [Interface](#interface) |

### O que o Larafim não protege

- **Código dos seus projetos.** Ligar um projeto roda o PHP dele e o comando do Node que o
  `package.json` define (`npm run dev`), e workers rodam `php artisan`. Isso é o objetivo do app, então
  só ligue projetos em que você confia, como faria com `npm install` ou `php artisan serve`.
- **Outros programas do seu usuário.** Um programa malicioso rodando como você já tem os mesmos
  acessos que o Larafim.
- **Bancos locais sem senha.** MariaDB (`root`) e PostgreSQL (`postgres`) do Larafim não têm senha,
  como no Herd e no Laragon. Eles só aceitam conexões de `127.0.0.1`, mas qualquer programa do PC
  consegue acessá-los. Não guarde dados reais ou sensíveis neles.

## Proteções

### Downloads

Código: [`download.rs`](../crates/larafim-runtimes/src/download.rs),
[`archive.rs`](../crates/larafim-runtimes/src/archive.rs),
[`safety.rs`](../crates/larafim-core/src/safety.rs) e os catálogos em
[`larafim-runtimes`](../crates/larafim-runtimes/src).

- **Só fontes oficiais, só `https://`.** Todo download precisa começar num destes hosts:
  `downloads.php.net`, `windows.php.net`, `nodejs.org`, `downloads.mariadb.org`, `github.com`,
  `getcomposer.org`, `curl.se`, `xdebug.org`. Um catálogo adulterado não consegue apontar para outro
  servidor (nem com truques como `github.com@evil.com`).
- **SHA-256 obrigatório.** O hash vem da fonte oficial (catálogo do PHP, `SHASUMS256.txt` do Node, API
  da MariaDB, campo `digest` do GitHub) ou fica fixo no código (Adminer). O arquivo é gravado como
  `.part` e só vira "pronto" depois de conferido. O Redis nativo vem só de
  `github.com/redis-windows/redis-windows`, com URL e nome do pacote conferidos e o `digest` do release.
- **Versões viram nomes de pasta só se forem seguras.** Versões e tags lidas dos catálogos passam por
  `is_safe_component`: nada de `..`, `/`, `\`, `:`, nomes reservados do Windows ou caracteres
  estranhos. Isso importa porque reinstalar uma versão apaga a pasta antiga.
- **Extração segura de zip.** Mesmo com o hash certo, o zip é tratado como hostil:
  - nenhum arquivo sai da pasta de destino (`../`, caminho absoluto, letra de unidade);
  - nada de `:` (fluxos alternativos do NTFS), nomes reservados (`CON`, `NUL`…), ponto ou espaço no
    fim do nome;
  - links simbólicos são recusados;
  - limite de 100 mil arquivos e 4 GB descompactados, contados nos bytes realmente escritos (o tamanho
    declarado no zip pode mentir);
  - se algo falhar, a pasta é apagada inteira.

### Arquivo hosts

Código: [`hosts.rs`](../crates/larafim-server/src/hosts.rs).

- O Larafim só escreve entre `# BEGIN LARAFIM` e `# END LARAFIM`.
- Cada domínio é validado (`valid_host`): só letras minúsculas, números, `-` e `.`, sem `..`, sem hífen
  no começo dos rótulos e sem quebra de linha. Um nome de pasta como `loja\r\n0.0.0.0 banco.com` não
  vira uma linha nova.
- O ajudante que roda como administrador **valida tudo de novo** antes de gravar (não confia no
  processo que o chamou), aceita no máximo 2000 domínios e guarda um backup do `hosts`.
- O sufixo (`.offline`) também é validado ao salvar as configurações.

### `.env`

Código: [`envfile.rs`](../crates/larafim-core/src/envfile.rs).

- Chaves só com letras, números e `_`.
- Valores com quebra de linha ou caracteres de controle são **recusados**: um valor como
  `loja\nAPP_DEBUG=true` criaria outra variável.
- Valores são gravados de forma **literal**: entre aspas simples, que o phpdotenv não expande. Se o
  valor tiver aspas simples, vai entre aspas duplas com `\`, `"` e `$` escapados. `${APP_KEY}` não é
  expandido e uma `\` no fim não quebra o arquivo.
- Antes da primeira mudança, o `.env` original vai para `.env.larafim.bak` (**Desfazer**).
- Reverb ([`reverb.rs`](../crates/larafim-core/src/reverb.rs)): valores já existentes no `.env` do
  projeto (a chave do app, copiada para `VITE_REVERB_APP_KEY`) passam pela mesma validação e
  escape; a porta vem só da faixa 8080–8179, nunca do arquivo.
- Chaves repetidas: todas as linhas da chave recebem o valor novo, e a leitura usa a última (a que o
  phpdotenv e o Vite usam). Antes, uma linha repetida mais abaixo continuava valendo.

### Bancos de dados

Código: [`mariadb.rs`](../crates/larafim-server/src/mariadb.rs),
[`postgres.rs`](../crates/larafim-server/src/postgres.rs),
[`dbclient.rs`](../crates/larafim-server/src/dbclient.rs).

- Nomes de banco vêm do domínio do projeto convertido para `[a-z0-9_]` (`database_name`) e são
  validados de novo (`valid_db`) antes de entrar em `CREATE DATABASE`.
- Os servidores escutam só em `127.0.0.1`.
- Senhas de bancos externos e a chave do S3 ficam cifradas com a **DPAPI** do Windows
  ([`secret.rs`](../app/src-tauri/src/secret.rs)), legíveis só pelo seu usuário neste PC.

### Servidor local

Código: [`resolve.rs`](../crates/larafim-server/src/resolve.rs),
[`pages.rs`](../crates/larafim-server/src/pages.rs),
[`gateway.rs`](../crates/larafim-server/src/gateway.rs).

- Escuta só em `127.0.0.1`. Domínios desconhecidos recebem uma página de "não encontrado", o que
  bloqueia *DNS rebinding* contra os projetos.
- O caminho da URL é decodificado e cada pedaço é checado. São recusados:
  - `..`, `\`, `:`, byte nulo e caracteres de controle;
  - arquivos ocultos (`.env`, `.git`), exceto `.well-known`;
  - **truques do Windows**: ponto ou espaço no fim (`/info.php.` abriria `info.php` como texto e
    vazaria o código), nomes curtos 8.3 (`/ENV~1` alcançaria o `.env`), fluxos do NTFS
    (`::$DATA`), caracteres proibidos (`<>"|?*`) e nomes de dispositivo (`CON`, `NUL`).
- Sem PHP para o projeto, arquivos `.php` dão 403 e nunca são entregues como texto.
- As páginas do próprio Larafim (projeto desligado, erro do PHP…) escapam tudo que vem da requisição
  ou do PHP (`& < > " '`).

### HTTPS local

Código: [`tls.rs`](../crates/larafim-server/src/tls.rs), [`https.rs`](../app/src-tauri/src/https.rs).

- A CA tem *name constraints*: só emite certificados para o sufixo local. Mesmo com a chave vazada,
  ela não serviria para criar certificados de sites reais.
- A chave da CA fica cifrada com DPAPI. Os certificados dos domínios são gerados em memória.
- A CA só é instalada quando você clica em **Confiar…**, e só no repositório do seu usuário.

### Interface

Código: [`tauri.conf.json`](../app/src-tauri/tauri.conf.json),
[`capabilities/`](../app/src-tauri/capabilities).

- **CSP estrita:** só scripts do próprio app (`script-src 'self'`); sem `<object>`, `<iframe>`, `<base>`
  nem envio de formulários; conexões só com a ponte do Tauri.
- **Protótipos congelados** (`freezePrototype`), contra *prototype pollution*.
- A interface nunca injeta HTML cru (`{@html}`, `innerHTML`): o Svelte escapa todo texto, inclusive
  os logs dos projetos.
- **Permissões mínimas:** as janelas só podem minimizar, maximizar, fechar, esconder, arrastar e abrir
  os diálogos de arquivo. Nada de acesso direto a arquivos ou shell pelo JavaScript.
- Os comandos do Rust validam o que recebem: `open_url` só abre endereços `http(s)://*.offline` e
  `localhost` (`is_local_url`), `open_folder` só abre pastas que existem e os logs só aceitam nomes
  `*.log` dentro da pasta de logs do projeto.

### Aviso de versão nova

Código: [`app_update.rs`](../crates/larafim-runtimes/src/app_update.rs),
[`updates.rs`](../app/src-tauri/src/updates.rs).

- Só **avisa**: nada é baixado nem executado sozinho. A pessoa baixa pela página da release.
- A resposta do GitHub é validada: a tag precisa ser `vX.Y.Z` estável e o link precisa ser uma página de
  release **deste** repositório. Uma resposta adulterada não leva o usuário para outro site.
- A interface nunca envia URLs para abrir: ela pede um link pelo nome (`repo`, `issues`, `update`…), e o
  endereço sai do Rust.
- A verificação pode ser desligada em *Configurações › Sobre*. A única informação enviada ao GitHub é
  o *User-Agent* com a versão do Larafim.

### Processos

- `php-cgi`, Node, workers e serviços rodam em **Job Objects**: se o Larafim fechar, o Windows encerra
  tudo, sem processos esquecidos.
- A maioria dos programas é iniciada com argumentos separados, sem passar por um shell. Passam pelo
  `cmd.exe` só os processos supervisionados: o comando do Node do projeto, os workers, o Mailpit, o
  RustFS e o `composer create-project`. Fora o comando do Node, que é do próprio projeto, todos são
  textos fixos montados pelo Larafim, com caminhos entre aspas. O nome do projeto no
  `create-project` só aceita letras, números, `-` e `_`.

## Testes

`cargo test --workspace` roda todos. Os de segurança:

| Proteção | Teste |
|---|---|
| Nomes seguros, URLs, SHA-256, `.env`, endereços locais | `larafim-core` › `safety::tests::*` |
| Injeção no `.env` | `envfile::tests::newline_in_value_cannot_create_variables`, `bad_keys_are_refused`, `values_are_literal`, `write_refuses_injection`, `reverb::tests::hostile_env_cannot_inject_lines` |
| Domínios e sufixo | `hosts::tests::host_validation`, `config::tests::rejects_bad_tld` |
| Só fontes oficiais | `download::tests::only_official_sources`, `refuses_before_connecting` |
| Zips maliciosos | `archive::tests::refuses_path_traversal`, `refuses_symlinks`, `refuses_zip_bomb`, `refuses_too_many_entries`, `refuses_garbage` |
| Catálogos adulterados | `mariadb::tests::tampered_catalog_is_ignored`, `php::catalog::tests::picks_nts_x64_sorted_and_rejects_unsafe_paths`, `postgres::tests::skips_without_digest`, `redis::tests::tampered_release_is_refused` |
| Filas e WSL na linha de comando | `safety::tests::queue_names`, `wsl_distros`, `queues::tests::hostile_names_never_become_arguments`, `redis::tests::hostile_wsl_names_are_dropped`, `config::tests::redis_config_is_validated` |
| Caminhos no servidor local | `resolve::tests::protections`, `windows_name_tricks` |
| XSS nas páginas do gateway | `pages::tests::request_data_is_escaped` |
| CA restrita ao domínio local | `tls::tests::ca_issues_only_inside_tld` |
| Nomes de banco | `mariadb::tests::names_and_ini` |
| Release adulterada no aviso de versão | `app_update::tests::refuses_tampered_release` |

Para rodar só esses: `cargo test --workspace -- safety envfile hosts download archive tampered resolve pages tls`.

## Cadeia de fornecimento

- **Dependências travadas:** `Cargo.lock` e `package-lock.json` versionados; o CI usa `--locked` e `npm ci`.
- **Auditoria:** o CI roda `cargo audit` e `npm audit` em cada push. O Dependabot abre PRs com
  atualizações.
- **Releases verificáveis:** cada instalador sai do workflow público, com `SHA256SUMS.txt` e atestação
  de origem (`gh attestation verify`).
- **Sem segredos no repositório:** chaves de assinatura e tokens, quando existirem, ficam só em
  *secrets* do GitHub.

## Ao mexer no código

Se a mudança lê algo de fora (arquivo de projeto, resposta de catálogo, requisição HTTP, nome de
pasta), siga estas regras:

1. Valide **antes** de usar, com as funções de [`safety.rs`](../crates/larafim-core/src/safety.rs)
   (ou acrescente uma lá, com testes).
2. Nunca monte caminho, linha de arquivo, SQL ou comando com texto não validado.
3. Para extrair arquivos, use `archive::extract_zip`; para baixar, `download_verified`.
4. Acrescente um teste com a **entrada maliciosa** (`../`, quebra de linha, `"`, `;`, `<script>`…).
