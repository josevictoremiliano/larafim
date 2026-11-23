# Serviços e bancos

*Configurações › Serviços* mostra os serviços instalados numa lista agrupada (Bancos de dados,
E-mail, Storage). Ao lado fica o painel do serviço selecionado, com a configuração, as ações, as
versões, as variáveis de ambiente prontas para copiar e o fim do log. Novos serviços entram por
**Adicionar serviço**.

Regras gerais:

- **Desligados por padrão.** Sobem quando um projeto que usa o serviço fica online, ou quando você
  liga na mão.
- **Só em `127.0.0.1`.** Nenhum serviço aceita conexões de outros computadores da rede.
- **Downloads conferidos.** Todo binário é conferido pelo SHA-256 publicado pela fonte oficial antes
  de ser extraído.

## MariaDB

O banco padrão para projetos PHP/Laravel, compatível com MySQL (`pdo_mysql`, `mysqli`).

- **Versões:** as linhas LTS da [API oficial da MariaDB](https://downloads.mariadb.org/), com
  SHA-256. Dá para ter várias instaladas e escolher a "versão em uso".
- **Modo leve:** `my.ini` com `performance_schema` desligado e buffers pequenos (~150 MB de memória
  ligado).
- **Acesso:** `127.0.0.1`, porta 3306 por padrão (use 3307 se já houver outro banco na 3306), usuário
  `root`, sem senha.
- **Dados:** `%LOCALAPPDATA%\Larafim\data\mariadb\<linha>`. Remover uma versão não apaga os dados.

### MariaDB próprio do projeto

Para um projeto que precisa de versão ou dados isolados: escolha **MariaDB próprio** no projeto. Ele
ganha um servidor só dele, com porta própria (sugerida a partir de 3310) e dados em
`data\mariadb-projetos`.

## PostgreSQL

Download opcional (~50 MB), com builds do projeto
[theseus-rs/postgresql-binaries](https://github.com/theseus-rs/postgresql-binaries), conferidas pelo
SHA-256 da release.

- **Acesso:** `127.0.0.1:5432`, usuário `postgres`, sem senha.
- Ao usar num projeto, o Larafim liga as extensões `pgsql`/`pdo_pgsql` no PHP do projeto.

## Bancos que você já tem

Um MySQL ou MariaDB que já roda no PC (Laragon, XAMPP, serviço do Windows) pode ser usado nos
projetos sem instalar outro. O Larafim **detecta** servidores nas portas 3306 a 3310 e oferece
"Usar este".

- A senha é guardada protegida pelo Windows (DPAPI): só o seu usuário, neste PC, consegue lê-la.
- O Larafim **não liga nem desliga** esse servidor. Ele só cria bancos, ajusta o `.env` e faz backups.

## Banco do projeto e o `.env`

No painel do projeto, em **Serviços › Banco de dados**, escolha SQLite, MariaDB, MariaDB próprio,
PostgreSQL ou uma conexão existente. O Larafim:

1. instala e liga o que faltar;
2. cria o banco com o nome do projeto (`loja-virtual` vira `loja_virtual`);
3. mostra **antes** o que vai mudar no `.env` (linha antiga → linha nova);
4. guarda o `.env` original em `.env.larafim.bak` na primeira mudança;
5. altera só as chaves daquele serviço (`DB_*`, `MAIL_*`, `AWS_*`…).

**Desfazer** restaura o `.env` a partir da cópia. Depois de qualquer mudança (ou de desfazer), se o
projeto tiver a configuração em cache (`bootstrap/cache/config.php`), o Larafim roda
`php artisan config:clear`, e os workers ligados (fila, agendador, Reverb) reiniciam para ler o `.env`
novo. Os valores são gravados de forma literal (o
Laravel não expande `${...}` neles) e nunca com quebra de linha.

### Backups

No projeto: **Backup** exporta o banco para `backups\<projeto>\<projeto>-<data>.sql`; **Restaurar**
importa um backup.

## Adminer

Gerenciador de bancos pelo navegador, em `http://db.offline`. Versão fixa (6.3.1), conferida por um
SHA-256 gravado no código do Larafim. Entra sozinho nos bancos do Larafim e pede senha nos demais.

## Mailpit

Caixa de e-mails local: tudo que os projetos enviam fica nela, nada sai para a internet.

- SMTP em `127.0.0.1:1025`, caixa de entrada em `http://mail.offline`.
- Variáveis para o Laravel: `MAIL_MAILER=smtp`, `MAIL_HOST=127.0.0.1`, `MAIL_PORT=1025`.

## Storage S3 (RustFS)

Um "S3 de mentira" para testar upload de arquivos como na AWS, sem custo. Usa o
[RustFS](https://github.com/rustfs/rustfs) (~118 MB).

- API em `http://127.0.0.1:9000`, painel em `http://s3.offline`.
- A chave secreta é gerada no seu PC e guardada protegida pela DPAPI.
- Ao usar num projeto, o Larafim cria um bucket com o nome do projeto e preenche `AWS_*` no `.env`.
- O Laravel precisa do pacote `league/flysystem-aws-s3-v3`; o Larafim avisa quando falta.

## Redis (filas e cache)

Opcional. Em **Adicionar serviço › Filas e cache › Redis**, escolha onde ele roda:

- **Windows (nativo)**, o padrão: o Larafim baixa o `redis-server.exe` (~14 MB) das compilações do
  projeto [redis-windows](https://github.com/redis-windows/redis-windows). A versão e o SHA-256 estão
  fixados no código. Esse modo funciona sem Docker ou WSL.
- **Docker**: contêiner `redis:7.4-alpine` com os dados num volume. O Docker Desktop precisa estar aberto.
- **WSL2**: usa o `redis-server` da distribuição escolhida. Instale antes pelo
  [repositório oficial](https://redis.io/docs/latest/operate/oss_and_stack/install/archive/install-redis/install-redis-on-windows/)
  (`sudo apt-get install redis`).

Nos três casos ele escuta só em `127.0.0.1`, liga quando um projeto online precisa e desliga ao sair.
No painel do projeto, **Serviços › Redis › Redis do Larafim (filas)** mostra a prévia e grava
`QUEUE_CONNECTION=redis`, `REDIS_CLIENT=predis`, `REDIS_HOST`, `REDIS_PORT` e `REDIS_PASSWORD` no
`.env` (com cópia). O Larafim usa o pacote `predis/predis` como cliente. Ao voltar para **Não
gerenciado**, confirme a prévia para repor os valores anteriores dessas chaves. Chaves que estavam
ausentes são comentadas. As demais configurações do `.env` são preservadas. Se o Redis foi ligado numa
versão que ainda não guardava os valores anteriores, o seletor avisa e deixa o `.env` como está.
