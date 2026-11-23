# Ferramentas

## Visualizador de logs

Abre pelo botão de logs no painel do projeto ou pela bandeja (*Visualizador de logs*). Mostra os
arquivos `.log` de `storage\logs` (Laravel) ou `logs` do projeto, ao vivo.

- O `laravel.log` é separado em **entradas**, com nível (Error, Warning, Info, Debug), hora e
  mensagem. A mais nova fica no topo.
- **Filtros** por nível com contagem, **busca** no texto e **Agrupar** mensagens iguais (×N).
- Na entrada selecionada aparecem a exceção, o arquivo e a linha (relativos ao projeto), o
  **stack trace** com os trechos de `vendor/` recolhidos e o contexto em JSON.
- **Copiar** a entrada original ou **Markdown** pronto para uma issue.
- Setas ↑/↓ navegam pelas entradas; **Ao vivo** pausa e continua.

Logs de outros formatos aparecem uma linha por entrada.

## Xdebug

Em *Configurações › PHP*, por versão: **ligar Xdebug** baixa a DLL certa de
[xdebug.org](https://xdebug.org/download) (versão do PHP, NTS, x64, conferida pelo SHA-256 da página)
e ajusta o `php.ini` no modo `debug` com `start_with_request=trigger`. O Xdebug só entra em ação
quando o navegador ou a IDE pedem, para não deixar tudo lento. O Larafim mostra o `launch.json`
pronto para o VS Code.

## Workers do Laravel

No painel de projetos Laravel: **fila** (`queue:work`), **agendador** (`schedule:work`) e **Reverb**
(`reverb:start`). Rodam como processos gerenciados, com log ao vivo, reinício e
volta automática quando o Larafim abre.

### Reverb

Ligar o **Reverb** configura tudo de uma vez:

1. escolhe uma porta só do projeto, a partir da 8080 (8080–8179), pulando as dos outros projetos
   (mesmo desligados), as dos serviços do Larafim e as que outro programa estiver usando;
2. mostra o que vai mudar no `.env` e pede confirmação (cópia em `.env.larafim.bak`, como nos
   bancos);
3. grava `BROADCAST_CONNECTION=reverb` e as credenciais que faltarem (`REVERB_APP_ID`, `_KEY`,
   `_SECRET`; as que já existem ficam). O Laravel publica direto no Reverb
   (`REVERB_HOST=127.0.0.1`, `REVERB_PORT=<porta>`, `REVERB_SCHEME=http`). O navegador conecta pelo
   domínio do projeto (`VITE_REVERB_HOST=<projeto>.<domínio>`, porta e esquema do servidor local,
   `https`/443 se o HTTPS estiver ligado) e o Larafim repassa o WebSocket de `/app/...` para o Reverb.
   Assim funciona também em páginas `https://`, onde o navegador bloquearia `ws://127.0.0.1`.
   Chaves repetidas no `.env` são todas atualizadas (vale a última, como no Laravel e no Vite);
4. liga o banco do projeto, se preciso (o `reverb:start` lê o cache), e inicia na hora
   (`reverb:start --host=127.0.0.1 --port=<porta>`), mesmo com o projeto offline.
   Depois, liga e desliga junto com o projeto.

A porta fica guardada no projeto. Se outro programa ocupar a porta, desligue e ligue o Reverb: o
Larafim escolhe outra e atualiza o `.env`. Faça o mesmo se trocar o domínio ou ligar/desligar o
HTTPS. Se o servidor do Vite estiver rodando, ele recarrega
sozinho com o `.env` novo.

Se o projeto ainda não tem o Reverb, a linha mostra **não instalado**. Ao ligar, o Larafim pergunta se
deve instalar e, se você confirmar, roda `php artisan install:broadcasting --reverb --without-node`
com o PHP e o Composer dele (altera `composer.json`, `composer.lock`, rotas e `.env`). Depois segue
para a escolha da porta. Nada é instalado sem a sua confirmação.

### Filas

O Laravel não tem uma lista de filas: o nome é só um texto em `onQueue('...')`. Ao ligar a **Fila**
pela primeira vez, o Larafim junta a fila padrão da conexão, as filas dos supervisores do Horizon
(`config/horizon.php`), os valores de `config('queue.names')` e os `onQueue('nome')` escritos no código
em `app/`. O worker roda `queue:work --queue=<lista>`, em ordem de prioridade.

No ícone de engrenagem da linha **Fila** dá para editar a lista ou **Detectar de novo**. Filas cujo
nome é montado em tempo de execução precisam ser acrescentadas à mão.

O Horizon não roda no Windows (precisa das extensões `pcntl` e `posix`). Com a fila no Redis do
Larafim, o worker usa `queue:work redis --queue=<lista>`, que processa as mesmas filas.

## Novo projeto Laravel

**Novo Laravel** (na janela principal ou na bandeja) cria um projeto com `composer create-project`:
nome, pasta, versão do PHP, starter kit (nenhum, React, Vue, Livewire) e banco. A saída aparece ao
vivo; no fim, o projeto já entra na lista com o `.env` configurado.

O nome só aceita letras, números, `-` e `_`.

## Painel da bandeja

O **clique esquerdo** no ícone do Larafim, perto do relógio, abre um painel com:

- **Abrir Larafim** e **Novo projeto Laravel**;
- **E-mail** (Mailpit), **Visualizador de logs** e **Serviços**;
- os **projetos online** (clique abre no navegador) e **Desligar todos**;
- o **PHP do terminal**: a versão usada fora dos projetos (automática ou uma fixa);
- **Configurações** e **Sair**.

O painel some ao clicar fora ou com Esc. O **clique direito** abre o menu clássico, com as mesmas
ações principais.

## Iniciar com o Windows

*Configurações › Geral*: com essa opção, o Larafim abre direto na bandeja ao entrar no Windows. Ele
usa a entrada do usuário em `HKCU\...\Run`, sem administrador.
