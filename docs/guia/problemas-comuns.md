# Problemas comuns

### "O Windows protegeu o computador" ao instalar

O instalador não tem assinatura de código. Confira o download
([como](instalacao.md#conferir-o-download)) e use *Mais informações › Executar assim mesmo*.

### O projeto não abre no navegador ("não foi possível encontrar o endereço")

- Confirme que o projeto está **Online**.
- O domínio precisa estar no arquivo `hosts`: se você recusou o pedido de administrador (UAC), ligue o
  projeto de novo para o Larafim pedir outra vez.
- Use o endereço completo com `http://` (ou `https://`): digitar só `loja.offline` pode virar busca.
- VPNs e alguns antivírus com "proteção de navegação" ignoram o arquivo `hosts`. Teste com eles
  desligados.

### A porta 80 (ou 443, 3306) está ocupada

O Larafim mostra o programa que ocupa a porta (Laragon, IIS, Apache, Skype antigo…). Feche esse
programa ou escolha outra porta em *Configurações › Rede e domínios* (ou na tela do banco).

### `php` não encontrado ou versão errada no terminal

- Abra um **terminal novo** depois de colocar os atalhos no `PATH`.
- Veja se há outro `php.exe` antes no `PATH` (o Larafim avisa em *Configurações › Terminal*).
- Use **Testar uma pasta** para ver qual versão seria escolhida e por quê.

### "Falta o Visual C++ Redistributable"

O PHP para Windows precisa dele. Use o link que o Larafim mostra (site da Microsoft), instale e tente de
novo.

### O antivírus bloqueou o `php-cgi.exe` ou um download

Os arquivos vêm das fontes oficiais e são conferidos pelo SHA-256 antes de serem usados. Se o antivírus
segurar algum, libere a pasta `%LOCALAPPDATA%\Larafim\runtimes` ou baixe de novo.

### HTTPS: aviso de certificado

- Clique em **Confiar…** em *Configurações › Rede e domínios*.
- No **Firefox**, importe a CA ou ligue `security.enterprise_roots.enabled`
  ([detalhes](https-e-dominios.md#firefox-e-ferramentas-de-linha-de-comando)).
- `curl` do Windows: use `--ssl-no-revoke`.

### Onde estão os logs do Larafim?

`%LOCALAPPDATA%\Larafim\logs`. Para mais detalhes, abra o Larafim com `$env:LARAFIM_LOG = "debug"`.
Ao abrir uma issue, anexe o trecho relevante (confira antes se não há caminhos ou nomes que você
prefira não publicar).
