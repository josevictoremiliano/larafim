# PHP e Node

## PHP

### Versões

*Configurações › PHP* lista as versões publicadas no
[catálogo oficial do PHP para Windows](https://downloads.php.net/~windows/releases/), com a última de
cada minor. O Larafim usa as builds **NTS x64**, as indicadas para FastCGI.

Ao instalar, o Larafim:

1. baixa o zip oficial e confere o **SHA-256** publicado no catálogo;
2. extrai com proteção contra arquivos maliciosos dentro do zip (veja [Segurança](../seguranca.md#downloads));
3. cria o `php.ini` a partir do `php.ini-development`, com as extensões comuns ligadas (`curl`,
   `fileinfo`, `gd`, `intl`, `mbstring`, `mysqli`, `openssl`, `pdo_mysql`, `pdo_pgsql`, `pdo_sqlite`,
   `pgsql`, `sodium`, `zip`) e os avisos de obsolescência escondidos da página
   (`error_reporting = E_ALL & ~E_DEPRECATED`);
4. baixa os certificados raiz (`cacert.pem`, do curl.se, também conferido), para o PHP conseguir usar
   HTTPS no Windows.

### Atualizações

O Larafim compara as versões instaladas com o catálogo ao abrir e a cada poucas horas, e mostra um
aviso quando há atualização. Ao atualizar, a build anterior do mesmo minor fica guardada para poder
voltar, e as suas mudanças no `php.ini` são levadas para a nova.

### Extensões e `php.ini`

Em cada versão: ligar e desligar extensões, abrir o `php.ini` no editor e abrir a pasta da versão.

**Avisos de obsolescência:** liga e desliga a exibição dos avisos `E_DEPRECATED` na página. O Larafim
só troca o `~E_DEPRECATED` do `error_reporting` atual (o resto do valor fica) e só quando você usa o
botão: o `php.ini` de uma versão já instalada nunca é alterado sozinho. Vale na próxima vez que o PHP
iniciar.

### Composer

O `composer.phar` é baixado do site oficial e conferido pelo SHA-256 publicado lá. No terminal, use
`composer` normalmente (veja [Terminal](terminal.md)).

### Qual PHP cada projeto usa

1. A versão fixada no projeto, se houver.
2. Senão, a mais nova instalada que atende o que o projeto pede (`.php-version` ou `require.php` do
   `composer.json`).
3. Senão, a mais nova instalada.

Se nenhuma versão instalada atende, o Larafim diz qual instalar. Ele nunca usa uma versão errada
em silêncio.

## Node

*Configurações › Node.js* lista as linhas LTS (18 em diante) e a Current mais nova, a partir de
`nodejs.org/dist/index.json`. O zip de Windows é conferido pelo `SHASUMS256.txt` oficial.

Versão de cada projeto: a fixada no projeto, senão a pedida (`.nvmrc`, `.node-version`,
`engines.node`), senão a LTS mais nova instalada.

### Servidor de desenvolvimento

Com **Servidor do Node** ligado no projeto, o comando (`npm run dev`, `pnpm dev`, `yarn dev`,
`bun run dev`, conforme o lockfile) roda junto com o projeto:

- com `PORT` definido (projetos Node) e `APP_URL` com o endereço `.offline` (Laravel + Vite);
- dentro de um *Job Object* do Windows: desligar o projeto encerra toda a árvore de processos
  (`cmd` → `npm` → `node`), sem deixar processos órfãos;
- com a saída ao vivo no painel do projeto.

O comando é do próprio projeto (`package.json`): o Larafim roda exatamente o que o projeto define.
Só ligue projetos em que você confia (veja [Segurança](../seguranca.md#o-que-o-larafim-não-protege)).
