# Novidades

As versões do Larafim e o que mudou em cada uma. O instalador de cada versão fica em
[Releases](https://github.com/josevictoremiliano/larafim/releases).

## v0.1.3

### Novidades

- **Atualização pelo próprio app:** *Atualizar e reiniciar* baixa a versão nova, confere a assinatura,
  instala sem a tela de desinstalar a versão anterior e reabre o Larafim. Quem está na v0.1.2 instala
  esta versão pelo instalador uma última vez.

### Correções

- O `php.ini` das versões de PHP instaladas é consertado ao abrir o app quando a pasta de dados mudou
  de lugar (antes as extensões deixavam de carregar).

### Outras mudanças

- Removida do README a política de assinatura de código da SignPath, que não chegou a ser usada.

## v0.1.2

### Novidades

- **Reverb por projeto:** porta própria, credenciais no `.env` (com prévia) e liga e desliga junto
  com o projeto.
- **Filas descobertas no projeto:** o Larafim encontra as filas usadas no código e sobe um worker para
  cada uma.
- **Redis gerenciado:** Redis nativo para Windows, sem WSL, baixado com versão e SHA-256 fixos.
- **Domínios extras (aliases)** por projeto.
- **Novo Laravel** cria projetos no Laravel 13 (PHP 8.3 a 8.5).
- **Pacote para a Microsoft Store (MSIX).** Instalado pela Store, o Larafim é atualizado por ela e não
  consulta o GitHub.

### Correções

- Reverb e workers de fila param quando o projeto é desligado, removido ou falha ao subir.
- Escolher "Não gerenciado" restaura os valores anteriores de `QUEUE_CONNECTION` e `REDIS_*`.
- Domínios extras recusam os nomes reservados (`db`, `mail`, `s3`).
- `php.ini` sem avisos de recursos obsoletos e com `pgsql` ligado por padrão.

## v0.1.1

Versão de manutenção do instalador.

### Correções

- O instalador agora mostra o **ícone do Larafim**, em vez do ícone genérico do NSIS.

### Outras mudanças

- Manifestos para publicar o Larafim no **winget** (`winget install larafim`), em
  [`packaging/winget`](https://github.com/josevictoremiliano/larafim/tree/main/packaging/winget).
- **Política de assinatura de código** no README, preparando a assinatura do instalador pela
  SignPath Foundation. O instalador desta versão ainda não é assinado.

## v0.1.0

O Larafim monta o ambiente de desenvolvimento local para projetos **PHP, Laravel e Node** no
Windows: cada projeto no seu endereço (`http://loja.offline`), cada um com a sua versão de PHP, tudo
ao mesmo tempo.

### Destaques

- **Várias versões de PHP ao mesmo tempo**, baixadas do site oficial com o SHA-256 conferido, com
  aviso de atualização, extensões e `php.ini` por versão, e Composer incluso.
- **Um domínio por projeto** (`.offline`), servido por um gateway próprio em Rust: estáticos direto,
  PHP por FastCGI e projetos Node por proxy com *hot reload*. HTTPS local opcional, com uma CA restrita
  ao domínio local.
- **Serviços sob demanda:** MariaDB (compartilhado ou próprio do projeto), PostgreSQL, bancos que você
  já tem (Laragon, XAMPP…), Mailpit, storage S3 (RustFS) e Adminer.
- **`.env` com segurança:** prévia do que muda, cópia automática e desfazer.
- **Terminal certo:** `php`, `composer`, `node` e `npm` usam a versão do projeto da pasta atual.
- **Ferramentas:** visualizador de logs do Laravel (níveis, filtros e stack trace com o `vendor/`
  recolhido), Xdebug com um clique, workers (fila, agendador, Reverb) e novo projeto Laravel.
- **Bandeja:** painel de atalhos no clique esquerdo; inicia com o Windows, se você quiser.
- **Português e inglês:** o idioma é escolhido no instalador e pode ser trocado depois, sem reiniciar.
- **Leve:** cerca de 10 MB de memória na bandeja.

### Segurança

Downloads só de fontes oficiais e com SHA-256; extração de zip protegida (caminhos para fora da pasta,
links simbólicos, zip bomb); validação do arquivo `hosts`, do `.env` e dos caminhos do servidor local;
CSP estrita na interface. Detalhes em [docs/seguranca.md](https://github.com/josevictoremiliano/larafim/blob/main/docs/seguranca.md).

### Requisitos e limitações

- Windows 10 ou 11 (x64). Só Windows, por enquanto.
- O instalador **ainda não tem assinatura digital**: o SmartScreen pode avisar
  (*Mais informações › Executar assim mesmo*). Confira o download antes, como explicado abaixo.
- Na primeira vez que um projeto fica online, o Windows pede administrador (UAC) para registrar os
  domínios no arquivo `hosts`.
