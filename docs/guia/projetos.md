# Projetos

## Pastas raiz

Em *Configurações › Pastas de projetos* ficam as pastas onde o Larafim procura projetos. Cada
subpasta direta é um candidato. **Atualizar** (na janela principal) procura de novo, sob demanda: o
Larafim não fica varrendo o disco em segundo plano.

Projetos fora dessas pastas entram por **Adicionar projeto**.

## Detecção

O Larafim só lê arquivos que já existem no projeto:

| Sinal | Conclusão |
|---|---|
| `artisan` + `composer.json` | Laravel, pasta pública `public/` |
| `composer.json` ou `index.php` | PHP; pasta pública `public/` se existir, senão a raiz |
| `.php-version` | Versão do PHP pedida (tem prioridade) |
| `composer.json` › `require.php` | Versão do PHP pedida (ex.: `^8.3`) |
| `package.json` com script `dev`/`start`/`serve` | Tem servidor de desenvolvimento do Node |
| `pnpm-lock.yaml` / `yarn.lock` / `bun.lock` | Gerenciador de pacotes usado no comando |
| `.nvmrc`, `.node-version`, `engines.node` | Versão do Node pedida |

Ele **nunca cria nem altera** esses arquivos.

## Configuração de cada projeto

No painel à direita da janela principal. No topo ficam o interruptor **Online** (com o botão de abrir
no navegador) e as **ações rápidas**; o resto fica em seções que abrem e fecham (**Ambiente**,
**Domínios extras**, **Serviços** e **Workers**). Fechada, cada seção mostra um resumo (versão do
PHP, serviços em uso, workers ligados), e o Larafim lembra quais você deixou abertas.


- **Domínio:** o endereço `<domínio>.offline`. Só aceita letras minúsculas, números e hífen.
- **Domínios extras:** apontam para o mesmo projeto. `db`, `mail`, `s3` e seus subdomínios são
  reservados para Adminer, Mailpit e RustFS.
- **Versão do PHP:** automática (a mais nova compatível com o que o projeto pede) ou fixa num minor
  (`8.4`). Fixar o minor faz o projeto acompanhar as atualizações de patch (8.4.10 → 8.4.12).
- **Pasta pública:** de onde os arquivos são servidos (`public/`, raiz…).
- **Servidor do Node:** liga o `npm run dev` (ou equivalente) junto com o projeto. O comando e a
  versão do Node são editáveis.
- **Serviços:** banco de dados, e-mail e storage (veja [Serviços e bancos](servicos.md)).

A configuração fica em `%LOCALAPPDATA%\Larafim\projects.toml`, **fora do repositório do projeto**:
nada do Larafim aparece no seu Git.

### Ações rápidas

Um clique roda o comando no projeto, com o PHP dele, sem abrir o terminal:

- **Laravel:** **Migrar** (`migrate`), **Migrar com seed** (`migrate --seed`), **Limpar cache**
  (`cache:clear`) e **Limpar otimizações** (`optimize:clear`). As migrações ligam antes o banco do
  projeto, se for um banco do Larafim.
- **`php artisan`:** campo para qualquer outro comando (`route:list`, `make:model Post -m`…). Enter
  executa. Comandos que ficam rodando (`serve`, `tinker`, `queue:work`, `reverb:start`…) são recusados:
  use o terminal do projeto ou os workers.
- **Composer:** `composer install` e `composer dump-autoload` (também em projetos PHP sem Laravel).

O resultado aparece logo abaixo, com o tempo e a saída do comando (que abre sozinha se der erro).
Cada comando tem até 10 minutos para terminar.

## Online e offline

O interruptor **Online** liga o projeto no endereço dele. Projetos online voltam sozinhos quando o
Larafim abre. **Desligar todos** fica na barra de status e no menu da bandeja: desliga os projetos e
também os bancos (MariaDB, PostgreSQL), o Redis e o PHP, deixando o servidor parado.

Como funciona por baixo:

- **Projetos PHP:** os arquivos estáticos são servidos direto; o resto vai para o `php-cgi` da versão
  do projeto, por FastCGI. Projetos da mesma versão compartilham um grupo de processos, que sobe na
  primeira requisição e desliga depois de 10 minutos parado.
- **Projetos Node:** o servidor de desenvolvimento roda com `PORT` definido, e o gateway encaminha o
  domínio para ele, com WebSocket (o *hot reload* do Vite funciona).
- **Projetos Laravel + Vite:** o PHP atende o domínio, e o Vite roda ao lado com `APP_URL` apontando
  para o endereço `.offline`.

## O que o servidor local nunca entrega

Por segurança, mesmo dentro da pasta pública:

- arquivos e pastas ocultos (`.env`, `.git`, `.htaccess`…), exceto `.well-known`;
- código PHP como texto (sem PHP instalado, `.php` dá 403);
- caminhos com `..`, `\`, `:`, nomes curtos do Windows (`ARQUIV~1`), ponto ou espaço no fim do nome
  e nomes de dispositivo (`CON`, `NUL`…).

Detalhes em [Segurança](../seguranca.md#servidor-local).

## Remover um projeto

**Remover do Larafim** só tira o projeto da lista: nenhum arquivo é apagado. Projetos de uma pasta
raiz removidos vão para "Projetos ignorados" em *Configurações › Pastas de projetos*, de onde podem
voltar.
