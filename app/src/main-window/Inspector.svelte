<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { api, kindLabel, projectUrl } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import { confirm, php, type PhpMatch } from "../lib/php";
  import { projectsApi, slugDomain, toConfig, type Project, type ProjectConfig, type ProjectsOverview } from "../lib/projects";
  import { asServerError, server, type ServerError } from "../lib/server";
  import { nodeApi } from "../lib/node";
  import { terminal } from "../lib/terminal";
  import DbPanel from "./DbPanel.svelte";
  import WorkersPanel from "./WorkersPanel.svelte";
  import AliasesPanel from "./AliasesPanel.svelte";
  import { toolsApi } from "../lib/tools";
  import { s3 } from "../lib/s3";
  import { redis } from "../lib/redis";
  import InstallProgress from "../lib/InstallProgress.svelte";
  import DevPanel from "./DevPanel.svelte";
  import { app } from "../lib/state.svelte";
  import Toggle from "../lib/Toggle.svelte";

  let {
    project,
    tick = 0,
    onchange,
    onreview,
  }: {
    project: Project | null;
    tick?: number;
    onchange: (data: ProjectsOverview) => void;
    onreview: (path: string) => void;
  } = $props();

  let copied = $state(false);
  let match = $state<PhpMatch | null>(null);
  let docroots = $state<string[]>([]);
  let error = $state<string | null>(null);
  let saving = $state(false);

  let nodeMajors = $state<string[]>([]);

  async function chooseMail(kind: string) {
    if (!project) return;
    const p = project;
    try {
      if (kind === "mailpit") {
        const diff = await toolsApi.mailPreview(p.path, kind);
        const lines = diff.map((d) => `${d.key}: ${d.before ?? "—"} → ${d.after ?? t("(comentada)")}`).join("\n");
        const ok = await confirm(
          t("Usar o Mailpit? Os e-mails do projeto vão para a caixa local (mail.{tld}).", { tld: app.config?.tld ?? "" }) +
            (lines ? "\n\n" + t("Mudanças no .env (com cópia em .env.larafim.bak):") + "\n" + lines : ""),
          t("E-mail do projeto"),
          t("Aplicar"),
        );
        if (!ok) {
          mailKey++;
          return;
        }
      }
      await toolsApi.mailApply(p.path, kind);
    } catch (e) {
      error = String(e);
      mailKey++;
    }
  }
  let mailKey = $state(0);
  let storageKey = $state(0);
  let redisKey = $state(0);
  let redisBusy = $state(false);

  // Redis do Larafim nas filas do projeto: mostra o que muda no .env antes.
  async function chooseRedis(kind: string) {
    if (!project) return;
    const p = project;
    try {
      if (kind === "redis") {
        const pv = await redis.queuePreview(p.path);
        if (!pv.configured) {
          error = t("Adicione o Redis em Configurações › Serviços.");
          redisKey++;
          return;
        }
        if (pv.needs_predis) {
          // Sem o pacote o Laravel não conversa com o Redis: oferece instalar (muda o composer.json).
          const install = await confirm(
            t("O projeto precisa do pacote predis/predis para usar o Redis. Instalar agora? (composer require predis/predis, altera composer.json e composer.lock)"),
            "Redis",
            t("Instalar"),
          );
          if (!install) {
            redisKey++;
            return;
          }
          redisBusy = true;
          try {
            await redis.installPredis(p.path);
          } finally {
            redisBusy = false;
          }
        }
        const lines = pv.diff.map((d) => `${d.key}: ${d.before ?? "—"} → ${d.after ?? t("(comentada)")}`).join("\n");
        const ok = await confirm(
          t("Usar o Redis do Larafim nas filas deste projeto?") +
            (lines ? "\n\n" + t("Mudanças no .env (com cópia em .env.larafim.bak):") + "\n" + lines : ""),
          "Redis",
          t("Aplicar"),
        );
        if (!ok) {
          redisKey++;
          return;
        }
      }
      await redis.queueConnection(p.path, kind === "redis" ? "redis" : "database");
    } catch (e) {
      error = String(e);
      redisKey++;
    }
  }

  async function chooseStorage(kind: string) {
    if (!project) return;
    const p = project;
    try {
      if (kind === "s3") {
        const st = await s3.status();
        if (!st.installed) {
          error = t("Instale o storage S3 em Configurações › Serviços.");
          storageKey++;
          return;
        }
        const pv = await s3.preview(p.path);
        const lines = pv.diff.map((d) => `${d.key}: ${d.before ?? "—"} → ${d.after ?? t("(comentada)")}`).join("\n");
        const pkg = pv.needs_package
          ? "\n\n" + t("O Laravel precisa do pacote do S3. Rode no terminal do projeto:") + '\ncomposer require league/flysystem-aws-s3-v3 "^3.0" --with-all-dependencies'
          : "";
        const ok = await confirm(
          t('Usar o storage S3 local? O bucket "{bucket}" será criado.', { bucket: pv.bucket }) +
            (lines ? "\n\n" + t("Mudanças no .env (com cópia em .env.larafim.bak):") + "\n" + lines : "") +
            pkg,
          t("Storage do projeto"),
          t("Aplicar"),
        );
        if (!ok) {
          storageKey++;
          return;
        }
      }
      await s3.apply(p.path, kind);
    } catch (e) {
      error = String(e);
      storageKey++;
    }
  }
  $effect(() => {
    void tick;
    nodeApi.installed().then((m) => (nodeMajors = m)).catch(() => {});
  });

  // Campos de texto: só gravam ao confirmar (Enter ou sair do campo).
  let domain = $state("");
  let nodeCommand = $state("");

  const url = $derived(project && app.config ? projectUrl(app.config, project.domain) : "");
  const imported = $derived(project?.status === "imported");

  // Ao trocar de projeto (ou ele mudar), recarrega campos e dados auxiliares.
  $effect(() => {
    const p = project;
    void tick;
    error = null;
    match = null;
    if (!p) return;
    domain = p.domain;
    nodeCommand = p.node_command ?? "";
    if (p.kind !== "node" && p.status !== "missing") {
      php
        .match(p.php_request, p.php_pin)
        .then((m) => {
          if (project?.path === p.path) match = m;
        })
        .catch(() => {});
    }
    if (p.status === "imported") {
      projectsApi
        .docroots(p.path)
        .then((d) => {
          if (project?.path === p.path) docroots = d.includes(p.docroot) ? d : [...d, p.docroot];
        })
        .catch(() => (docroots = [p.docroot]));
    }
  });

  async function save(change: Partial<ProjectConfig>) {
    if (!project) return;
    saving = true;
    try {
      onchange(await projectsApi.save({ ...toConfig(project), ...change }));
      error = null;
    } catch (e) {
      error = String(e);
      // Volta os campos de texto ao valor gravado.
      domain = project.domain;
      nodeCommand = project.node_command ?? "";
    } finally {
      saving = false;
    }
  }

  function commitDomain() {
    if (!project) return;
    const value = slugDomain(domain);
    if (!value) {
      domain = project.domain;
      return;
    }
    domain = value;
    if (value !== project.domain) save({ domain: value });
  }

  function commitNodeCommand() {
    if (project && nodeCommand.trim() !== (project.node_command ?? "")) save({ node_command: nodeCommand.trim() });
  }

  async function act(action: Promise<ProjectsOverview>) {
    try {
      onchange(await action);
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  // ---------- Online / offline ----------
  let switching = $state(false);
  let serverError = $state<ServerError | null>(null);
  const altPort = $derived(app.config?.http_port === 8080 ? 8081 : 8080);

  $effect(() => {
    void project?.path;
    serverError = null;
  });

  async function toggleOnline(online: boolean) {
    if (!project) return;
    const path = project.path;
    switching = true;
    serverError = null;
    try {
      await server.setOnline(path, online);
    } catch (e) {
      if (project?.path === path) serverError = asServerError(e);
    } finally {
      switching = false;
    }
  }

  async function useAltPort() {
    await app.update({ http_port: altPort });
    await toggleOnline(true);
  }

  async function removeProject() {
    if (!project) return;
    const ok = await confirm(
      t('Tirar "{name}" do Larafim? Os arquivos do projeto NÃO são apagados.', { name: project.name }) +
        (project.root ? " " + t("Ele deixa de aparecer na lista; dá para mostrar de novo em Configurações › Pastas de projetos.") : ""),
      t("Remover do Larafim"),
      t("Remover"),
    );
    if (ok) act(projectsApi.remove(project.path));
  }

  async function copyUrl() {
    await navigator.clipboard.writeText(url);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const docrootLabel = (d: string) => (d ? `${d}\\` : t("Raiz do projeto"));
  const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
</script>

<aside class="inspector">
  {#if project && app.config}
    <div class="head">
      <div class="title-row">
        <h2 title={url}>{project.domain}.{app.config.tld}</h2>
        <button class="btn subtle icon" title={copied ? t("Copiado!") : t("Copiar endereço")} onclick={copyUrl}>
          <Glyph name={copied ? "check" : "link"} />
        </button>
        {#if project.status !== "missing"}
          <button class="btn subtle icon" title={t("Logs do projeto")} onclick={() => api.openLogs(project.path)}>
            <Glyph name="logs" />
          </button>
          <button
            class="btn subtle icon"
            title={t("Abrir terminal na pasta (php e composer com a versão do projeto)")}
            onclick={() => terminal.openTerminal(project.path).catch((e) => (error = String(e)))}
          >
            <Glyph name="terminal" />
          </button>
          <button class="btn subtle icon" title={t("Abrir pasta no Explorador")} onclick={() => api.openFolder(project.path)}>
            <Glyph name="folderOpen" />
          </button>
        {/if}
      </div>
      <div class="path mono" title={project.path}>{project.path}</div>
    </div>

    {#if error}
      <div class="notice err" role="alert">
        <Glyph name="info" size={14} />
        <span>{capitalize(error)}</span>
      </div>
    {/if}

    {#if project.status === "new"}
      <div class="notice accent">
        <Glyph name="info" size={14} />
        <span>{t("Projeto encontrado na pasta, ainda não importado. Importe para configurar e usar.")}</span>
      </div>
      <div class="buttons">
        <button class="btn accent" onclick={() => act(projectsApi.import(project.path))}>{t("Importar")}</button>
        <button class="btn" onclick={() => onreview(project.path)}>{t("Revisar…")}</button>
        <span class="grow"></span>
        <button class="btn subtle" title={t("Esconder da lista")} onclick={() => act(projectsApi.ignore(project.path))}>
          {t("Ignorar")}
        </button>
      </div>
    {:else if project.status === "missing"}
      <div class="notice warn">
        <Glyph name="info" size={14} />
        <span>{t("A pasta deste projeto não existe mais (foi movida, renomeada ou apagada).")}</span>
      </div>
      <div class="buttons">
        <button class="btn" onclick={removeProject}>{t("Remover do Larafim")}</button>
      </div>
    {:else}
      <div class="status card" class:is-on={project.online}>
        <span class="dot" class:on={project.online} class:busy={switching}></span>
        <div class="status-text">
          <strong>{switching ? (project.online ? t("Desligando…") : t("Ligando…")) : project.online ? t("Online") : t("Offline")}</strong>
          <span class="caption">
            {#if project.online}
              {url.replace(/^https?:\/\//, "")}
            {:else}
              {t("Ligue para acessar no navegador")}
            {/if}
          </span>
        </div>
        <Toggle
          label={t("Projeto online")}
          checked={project.online}
          disabled={switching}
          onchange={(v) => toggleOnline(v)}
        />
      </div>

      {#if serverError}
        <div class="notice err server-error" role="alert">
          <Glyph name="info" size={14} />
          <div class="err-body">
            <span>{capitalize(serverError.message)}</span>
            {#if serverError.code === "port_busy"}
              <span class="caption">
                {t("Feche o outro programa ou use outra porta (os endereços passam a ter")}
                <span class="mono">:{altPort}</span>).
              </span>
            {/if}
            <div class="err-actions">
              {#if serverError.code === "port_busy"}
                <button class="btn" onclick={useAltPort}>{t("Usar porta {port}", { port: altPort })}</button>
                <button class="btn subtle" onclick={() => toggleOnline(true)}>{t("Tentar de novo")}</button>
              {:else if serverError.code === "uac_denied" || serverError.code === "hosts_failed"}
                <button class="btn" onclick={() => toggleOnline(true)}>{t("Tentar de novo")}</button>
              {:else if serverError.code === "no_node"}
                <button class="btn" onclick={() => api.openSettings("node")}>
                  {serverError.suggest ? t("Instalar Node {v}", { v: serverError.suggest }) : t("Instalar Node.js")}
                </button>
              {:else if serverError.code === "no_php"}
                <button class="btn" onclick={() => api.openSettings("php")}>
                  {serverError.suggest ? t("Instalar PHP {v}", { v: serverError.suggest }) : t("Instalar PHP")}
                </button>
              {/if}
            </div>
          </div>
        </div>
      {/if}

      <button class="btn wide" disabled={!project.online} onclick={() => server.openUrl(url)}>
        <Glyph name="globe" />
        {t("Abrir no navegador")}
      </button>
    {/if}

    {#if project.status !== "missing"}
      <div class="section">
        <div class="section-title">{t("Ambiente")}</div>

        <div class="field">
          <span>{t("Tipo")}</span>
          <span class="value">{kindLabel[project.kind]}{project.kind !== "node" && project.has_node ? " + Node" : ""}</span>
        </div>

        {#if imported}
          <label class="field">
            <span>{t("Domínio")}</span>
            <span class="domain">
              <input
                class="input"
                bind:value={domain}
                onblur={commitDomain}
                onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
                spellcheck="false"
              />
              <span class="caption">.{app.config.tld}</span>
            </span>
          </label>
        {/if}

        {#if project.kind !== "node"}
          <div class="field">
            <span>{t("Versão do PHP")}</span>
            <div class="select">
              <select
                disabled={!imported || saving}
                value={project.php_pin ?? ""}
                onchange={(e) => save({ php: e.currentTarget.value || null })}
              >
                <option value="">
                  {match?.installed && !project.php_pin ? t("Automática ({v})", { v: match.installed }) : t("Automática")}
                </option>
                {#each match?.available ?? [] as minor (minor)}
                  <option value={minor}>PHP {minor}</option>
                {/each}
                {#if project.php_pin && !(match?.available ?? []).includes(project.php_pin)}
                  <option value={project.php_pin}>{t("PHP {v} (não instalado)", { v: project.php_pin })}</option>
                {/if}
              </select>
            </div>
          </div>
          <p class="caption hint">
            {project.php_request
              ? t("Pedida pelo projeto: {v}", { v: project.php_request })
              : t("O projeto não informa a versão do PHP.")}
          </p>
          {#if match && !match.installed}
            <div class="notice warn">
              <Glyph name="info" size={14} />
              <span>
                {project.php_pin
                  ? t("O PHP {v} não está instalado.", { v: project.php_pin })
                  : t("Nenhum PHP instalado atende a este projeto.")}
              </span>
              <button class="btn" onclick={() => api.openSettings("php")}>
                {match.suggest ? t("Instalar {v}", { v: match.suggest }) : t("Ver versões")}
              </button>
            </div>
          {/if}

          <div class="field">
            <span>{t("Pasta pública")}</span>
            {#if imported}
              <div class="select">
                <select disabled={saving} value={project.docroot} onchange={(e) => save({ docroot: e.currentTarget.value })}>
                  {#each docroots as d (d)}
                    <option value={d}>{docrootLabel(d)}</option>
                  {/each}
                </select>
              </div>
            {:else}
              <span class="value mono">{docrootLabel(project.docroot)}</span>
            {/if}
          </div>
        {/if}

        {#if project.has_node}
          <div class="field">
            <span>{t("Servidor do Node")}</span>
            <Toggle
              label={t("Rodar o servidor de desenvolvimento do Node")}
              checked={project.node_enabled}
              disabled={!imported || saving}
              onchange={(v) => save({ node: v, node_command: project.node_command ?? "npm run dev" })}
            />
          </div>
          {#if imported && project.node_enabled}
            <label class="field">
              <span>{t("Comando")}</span>
              <input
                class="input mono-input"
                bind:value={nodeCommand}
                onblur={commitNodeCommand}
                onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
                spellcheck="false"
              />
            </label>
          {/if}
          {#if imported}
            <div class="field">
              <span>{t("Versão do Node")}</span>
              <div class="select">
                <select
                  disabled={saving}
                  value={project.node_pin ?? ""}
                  onchange={(e) => save({ node_version: e.currentTarget.value || null })}
                >
                  <option value="">{t("Automática")}</option>
                  {#each nodeMajors as m (m)}<option value={m}>Node {m}</option>{/each}
                  {#if project.node_pin && !nodeMajors.includes(project.node_pin)}
                    <option value={project.node_pin}>{t("Node {v} (não instalado)", { v: project.node_pin })}</option>
                  {/if}
                </select>
              </div>
            </div>
          {/if}
          {#if project.node_request}
            <p class="caption hint">{t("Pedido pelo projeto: {v}", { v: project.node_request })}</p>
          {/if}
          {#if imported && (project.kind === "node" || project.node_enabled)}
            <DevPanel path={project.path} online={project.online} />
          {/if}
        {/if}
      </div>
    {/if}

    {#if imported}
      <div class="section">
        <AliasesPanel {project} {onchange} />
      </div>
    {/if}

    {#if imported && project.kind !== "node"}
      <div class="section">
        <div class="section-title">{t("Serviços")}</div>
        <DbPanel {project} />
        <div class="field">
          <span>{t("E-mail")}</span>
          {#key mailKey}
            <div class="select">
              <select value={project.mail ?? "none"} onchange={(e) => chooseMail(e.currentTarget.value)}>
                <option value="none">{t("Não gerenciado")}</option>
                <option value="mailpit">{t("Mailpit (caixa local)")}</option>
              </select>
            </div>
          {/key}
        </div>
        {#if project.mail === "mailpit"}
          <button class="btn wide" onclick={() => toolsApi.openMailpit().catch((e) => (error = String(e)))}>
            <Glyph name="mail" /> {t("Abrir caixa de e-mails")}
          </button>
        {/if}
        <div class="field">
          <span>{t("Storage")}</span>
          {#key storageKey}
            <div class="select">
              <select value={project.storage ?? "none"} onchange={(e) => chooseStorage(e.currentTarget.value)}>
                <option value="none">{t("Não gerenciado")}</option>
                <option value="s3">{t("S3 local (RustFS)")}</option>
              </select>
            </div>
          {/key}
        </div>
        {#if project.storage === "s3"}
          <button class="btn wide" onclick={() => s3.open().catch((e) => (error = String(e)))}>
            <Glyph name="openExternal" /> {t("Abrir painel do storage")}
          </button>
        {/if}
        {#if project.kind === "laravel"}
          <div class="field">
            <span>Redis</span>
            {#key redisKey}
              <div class="select">
                <select value={project.queue_connection ?? "none"} disabled={redisBusy} onchange={(e) => chooseRedis(e.currentTarget.value)}>
                  <option value="none">{t("Não gerenciado")}</option>
                  <option value="redis">{t("Redis do Larafim (filas)")}</option>
                </select>
              </div>
            {/key}
          </div>
          {#if redisBusy}<InstallProgress path={project.path} label={t("Instalando predis/predis…")} />{/if}
        {/if}
      </div>
    {/if}

    {#if imported && project.kind === "laravel"}
      <div class="section">
        <WorkersPanel {project} />
      </div>
    {/if}

    {#if imported}
      <div class="footer">
        <button class="btn subtle danger" onclick={removeProject}>
          <Glyph name="delete" />
          {t("Remover do Larafim")}
        </button>
      </div>
    {/if}
  {:else}
    <div class="none caption">{t("Selecione um projeto para ver os detalhes.")}</div>
  {/if}
</aside>


<style>
  .inspector {
    width: 340px;
    flex: none;
    overflow: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    background: var(--win-bg);
    border-left: 1px solid var(--stroke);
  }
  .inspector > :global(*) {
    flex-shrink: 0;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font: 600 16px/22px var(--font-display);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }
  .path {
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 8px 8px 10px;
    border-radius: var(--r-card);
    border: 1px solid var(--stroke);
    font-size: 12px;
    line-height: 16px;
    user-select: text;
  }
  .notice span {
    flex: 1;
  }
  .notice.accent {
    background: var(--accent-soft);
  }
  .notice.accent :global(.glyph) {
    color: var(--accent-text);
  }
  .notice.warn {
    background: color-mix(in srgb, var(--warn) 12%, var(--card));
  }
  .notice.warn :global(.glyph) {
    color: var(--warn);
  }
  .notice.err {
    background: color-mix(in srgb, var(--err) 12%, var(--card));
  }
  .notice.err :global(.glyph) {
    color: var(--err);
  }

  .buttons {
    display: flex;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
  }
  .status-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .status-text .caption {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status.is-on {
    border-color: color-mix(in srgb, var(--ok) 35%, var(--stroke));
  }
  .dot.busy {
    background: var(--warn);
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
  .wide {
    width: 100%;
  }
  .server-error {
    align-items: flex-start;
  }
  .server-error :global(.glyph) {
    margin-top: 1px;
  }
  .err-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .err-actions {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }
  .err-actions:empty {
    display: none;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 4px;
  }
  .section-title {
    margin-bottom: 2px;
  }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 32px;
  }
  .field .select {
    width: 170px;
  }
  .domain {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .domain input {
    width: 156px;
    text-align: right;
  }
  .mono-input {
    width: 170px;
    font-family: var(--mono);
    font-size: 12px;
  }
  .value {
    color: var(--text-2);
  }
  .hint {
    margin: -4px 0 2px;
    text-align: right;
  }
  p {
    margin: 2px 0 0;
  }

  .footer {
    margin-top: auto;
    padding-top: 8px;
    border-top: 1px solid var(--stroke);
  }
  .danger {
    color: var(--err);
  }

  .none {
    margin: auto;
    text-align: center;
  }
</style>
