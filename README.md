<div align="center">

<img src="docs/assets/logo.svg" width="96" height="96" alt="Logo do Larafim">

# Larafim

**PHP, Node e bancos de dados locais para todos os seus projetos, no Windows.**

Cada projeto no seu endereço (`https://loja.offline`), cada um com a sua versão de PHP,
ao mesmo tempo, sem baixar nem configurar nada à mão.

[![CI](https://github.com/josevictoremiliano/larafim/actions/workflows/ci.yml/badge.svg)](https://github.com/josevictoremiliano/larafim/actions/workflows/ci.yml)
[![Licença: MIT](https://img.shields.io/badge/licen%C3%A7a-MIT-blue.svg)](LICENSE)
![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078d4)

[Baixar](https://github.com/josevictoremiliano/larafim/releases/latest) ·
[Documentação](docs/README.md) ·
[Segurança](SECURITY.md) ·
[English](README.en.md)

</div>

---

## O que é

O Larafim é um app de bandeja, leve e nativo do Windows 11, que monta o ambiente de
desenvolvimento local para projetos **PHP, Laravel e Node**, na linha do Laravel Herd e do
Laragon:

- **Várias versões de PHP ao mesmo tempo.** O projeto A roda em 8.3 e o B em 8.5, lado a lado.
  O Larafim baixa as versões oficiais, confere o SHA-256 e avisa quando há atualização.
- **Um endereço por projeto.** `http://loja.offline` (ou `https://`, com certificado local), sem
  Apache nem Nginx: um servidor próprio em Rust encaminha cada domínio para o PHP ou para o
  servidor de desenvolvimento do Node.
- **Serviços sob demanda.** MariaDB, PostgreSQL, Mailpit (caixa de e-mails local), storage S3
  (RustFS) e Adminer. Ficam desligados e só sobem quando um projeto precisa.
- **`.env` sem medo.** Ao ligar um serviço num projeto, o Larafim mostra o que vai mudar no `.env`,
  guarda uma cópia e permite desfazer.
- **Terminal certo.** `php` e `composer` usam, em qualquer terminal, a versão do projeto da pasta
  atual.
- **Ferramentas do dia a dia.** Visualizador de logs do Laravel com o stack trace organizado, Xdebug
  com um clique, workers (fila, agendador, Reverb) e novo projeto Laravel.
- **Leve.** Na bandeja, o Larafim ocupa cerca de 10 MB de memória.

Interface em português e inglês. Mais idiomas são fáceis de acrescentar
([como](docs/desenvolvimento/traducoes.md)).

## Instalação

1. Baixe o `Larafim_x.y.z_x64-setup.exe` na [página de releases](https://github.com/josevictoremiliano/larafim/releases/latest).
2. Confira se o arquivo é o original: veja o [guia de instalação](docs/guia/instalacao.md#conferir-o-download).
3. Rode o instalador. Ele instala só para o seu usuário, sem pedir administrador, e pergunta o idioma.

O instalador ainda não tem assinatura digital, então o Windows pode mostrar o aviso do SmartScreen
("O Windows protegeu o computador"). Em *Mais informações › Executar assim mesmo* a instalação
continua.

Requisitos: Windows 10 ou 11 (x64) com o WebView2, que já vem no Windows 11.

## Primeiros passos

1. Na primeira abertura, escolha a pasta onde ficam seus projetos (ex.: `D:\www`).
2. Instale uma versão do PHP em *Configurações › PHP*.
3. Importe os projetos encontrados e ligue um deles: ele abre em `http://<nome>.offline`.

O primeiro projeto ligado pede permissão de administrador (UAC) uma única vez, para registrar os
domínios no arquivo `hosts`. O guia completo está em [Primeiros passos](docs/guia/primeiros-passos.md).

## Documentação

| | |
|---|---|
| **Usar o Larafim** | [Instalação](docs/guia/instalacao.md) · [Primeiros passos](docs/guia/primeiros-passos.md) · [Projetos](docs/guia/projetos.md) · [PHP e Node](docs/guia/php-e-node.md) · [Serviços e bancos](docs/guia/servicos.md) · [Ferramentas](docs/guia/ferramentas.md) · [HTTPS e domínios](docs/guia/https-e-dominios.md) · [Terminal](docs/guia/terminal.md) · [Problemas comuns](docs/guia/problemas-comuns.md) |
| **Desenvolver** | [Arquitetura](docs/desenvolvimento/arquitetura.md) · [Compilar e testar](docs/desenvolvimento/compilar-e-testar.md) · [Traduções](docs/desenvolvimento/traducoes.md) · [Publicar uma versão](docs/desenvolvimento/publicar-versao.md) |
| **Segurança** | [Modelo de segurança e proteções](docs/seguranca.md) · [Como reportar uma falha](SECURITY.md) |

Índice completo em [docs/README.md](docs/README.md).

## Contribuir

Contribuições são bem-vindas: leia o [guia de contribuição](CONTRIBUTING.md). Agentes de IA (Claude
Code, Codex, Copilot…) devem seguir o [AGENTS.md](AGENTS.md).

Para compilar:

```powershell
cd app
npm ci
npm run tauri dev
```

Detalhes, requisitos e testes em [Compilar e testar](docs/desenvolvimento/compilar-e-testar.md).

## Plataformas

O Larafim é **só para Windows** por enquanto. Ele depende de partes do Windows, como o arquivo
`hosts` com UAC, a proteção de senhas (DPAPI), o registro e os Job Objects, e baixa as versões de
Windows do PHP, do MariaDB e do PostgreSQL. Versões para macOS e Linux não estão descartadas, mas
exigem portar essas partes. Veja a [arquitetura](docs/desenvolvimento/arquitetura.md#outras-plataformas).

## Privacidade

O Larafim não envia dados seus para nenhum servidor. Ele só se conecta à internet
para baixar o que você pede (PHP, Node, bancos de dados e ferramentas, das fontes oficiais) e para
ver se há versão nova do Larafim no GitHub. Essa última verificação pode ser desligada em
*Configurações › Sobre › Verificar automaticamente*.

## Licença

[MIT](LICENSE).
