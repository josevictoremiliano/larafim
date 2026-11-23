<script lang="ts">
  import { plural, t } from "../lib/i18n.svelte";
  import { api, kindLabel, onEvent, projectUrl } from "../lib/api";
  import { updates, type UpdateStatus } from "../lib/updates";
  import { pickFolder } from "../lib/folders";
  import Glyph from "../lib/Glyph.svelte";
  import { php } from "../lib/php";
  import { projectsApi, type FolderProposal, type Project, type ProjectsOverview } from "../lib/projects";
  import { asServerError, server, type ServerError, type ServerStatus } from "../lib/server";
  import { app } from "../lib/state.svelte";
  import TitleBar from "../lib/TitleBar.svelte";
  import AddProjectDialog from "./AddProjectDialog.svelte";
  import NewLaravelDialog from "./NewLaravelDialog.svelte";
  import Inspector from "./Inspector.svelte";

  let projects = $state<Project[]>([]);
  let scanning = $state(false);
  let query = $state("");
  let selectedPath = $state<string | null>(null);
  let searchInput: HTMLInputElement;
  let phpUpdates = $state(0);
  /** Muda quando um PHP é instalado/removido, para o painel recalcular a versão. */
  let phpVersionTick = $state(0);
  let status = $state<ServerStatus | null>(null);
  // Versão nova do Larafim: barra no topo até a pessoa dispensar aquela versão.
  let update = $state<UpdateStatus | null>(null);
  let dismissedUpdate = $state<string | null>(null);
  try {
    dismissedUpdate = localStorage.getItem("larafim:update-dismissed");
  } catch {
    // sem armazenamento: só não lembra a escolha
  }
  function dismissUpdate() {
    dismissedUpdate = update?.latest ?? null;
    try {
      if (dismissedUpdate) localStorage.setItem("larafim:update-dismissed", dismissedUpdate);
    } catch {
      // idem
    }
  }
  $effect(() => {
    updates.status().then((s) => (update = s)).catch(() => {});
    return onEvent<UpdateStatus>("app:update", (s) => (update = s));
  });

  // Aberta pelo painel da bandeja em "Novo projeto Laravel": já começa no diálogo.
  let creating = $state(!!(window as { __LARAFIM_NEW_LARAVEL__?: boolean }).__LARAFIM_NEW_LARAVEL__);
  /** Diálogo de adicionar/revisar projeto. */
  let proposal = $state<FolderProposal | null>(null);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? projects.filter((p) => p.name.toLowerCase().includes(q) || p.domain.includes(q)) : projects;
  });

  /** Grupos: cada pasta raiz (na ordem configurada) e, por último, os avulsos. */
  const groups = $derived.by(() => {
    const list: { key: string; label: string; avulso: boolean; items: Project[] }[] = [];
    for (const root of app.config?.roots ?? []) {
      list.push({ key: root, label: root, avulso: false, items: filtered.filter((p) => p.root === root) });
    }
    const loose = filtered.filter((p) => p.root === null || !(app.config?.roots ?? []).includes(p.root));
    if (loose.length) list.push({ key: "__avulsos", label: t("Projetos avulsos"), avulso: true, items: loose });
    return list;
  });

  /** Plural simples: chaves "{n} item" / "{n} itens" (cada idioma traduz as duas). */
  const newCount = $derived(projects.filter((p) => p.status === "new").length);
  const selected = $derived(projects.find((p) => p.path === selectedPath) ?? null);

  function apply(data: ProjectsOverview) {
    projects = data.projects;
    if (!projects.some((p) => p.path === selectedPath)) selectedPath = projects[0]?.path ?? null;
  }

  async function scan() {
    scanning = true;
    try {
      apply(await projectsApi.list());
    } catch (e) {
      app.error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function run(action: Promise<ProjectsOverview>) {
    try {
      apply(await action);
      app.error = null;
    } catch (e) {
      app.error = String(e);
    }
  }

  async function fixHosts() {
    try {
      status = await server.fixHosts();
    } catch (e) {
      app.error = asServerError(e).message;
    }
  }

  async function addRoot() {
    const dir = await pickFolder(t("Escolha a pasta onde ficam seus projetos"));
    if (!dir || !app.config || app.config.roots.includes(dir)) return;
    await app.update({ roots: [...app.config.roots, dir] });
  }

  async function addProject() {
    const dir = await pickFolder(t("Escolha a pasta do projeto"));
    if (!dir) return;
    await review(dir);
  }

  /** Abre o diálogo de revisão para uma pasta (avulsa ou projeto novo). */
  async function review(dir: string) {
    try {
      const p = await projectsApi.inspectFolder(dir);
      if (p.already_imported) {
        selectedPath = projects.find((x) => x.path.toLowerCase() === dir.toLowerCase())?.path ?? selectedPath;
        app.error = t("Este projeto já está no Larafim.");
        return;
      }
      proposal = p;
    } catch (e) {
      app.error = String(e);
    }
  }

  // Reescaneia quando as pastas mudam (inclusive pela janela de configurações).
  $effect(() => {
    app.config?.roots.join("|");
    if (app.config) scan();
  });

  $effect(() => {
    php.overview().then((o) => (phpUpdates = o.updates)).catch(() => {});
    const stops = [
      onEvent<number>("php:updates", (n) => {
        phpUpdates = n;
        phpVersionTick++;
      }),
      onEvent<ProjectsOverview>("projects-changed", apply),
      onEvent("main:new-laravel", () => (creating = true)),
      onEvent<ServerStatus>("server-status", (s) => (status = s)),
      onEvent<ServerError>("server-error", (e) => (app.error = asServerError(e).message)),
    ];
    server.status().then((s) => (status = s)).catch(() => {});
    return () => stops.forEach((s) => s());
  });

  function onKey(e: KeyboardEvent) {
    if (proposal) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchInput?.focus();
    }
    if (e.key === "F5") scan();
  }

  function moveSelection(step: number) {
    const flat = groups.flatMap((g) => g.items);
    const i = flat.findIndex((p) => p.path === selectedPath);
    const next = flat[Math.min(flat.length - 1, Math.max(0, i + step))];
    if (next) {
      selectedPath = next.path;
      document.getElementById(`row-${CSS.escape(next.path)}`)?.scrollIntoView({ block: "nearest" });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="window">
  <TitleBar title="Larafim" />

  <div class="toolbar">
    <button class="btn subtle" onclick={addProject}>
      <Glyph name="add" />
      {t("Adicionar projeto")}
    </button>
    <button class="btn subtle" onclick={addRoot}>
      <Glyph name="folder" />
      {t("Adicionar pasta")}
    </button>
    <button class="btn subtle" onclick={() => (creating = true)}>
      <Glyph name="package" />
      {t("Novo Laravel")}
    </button>

    <div class="grow" data-tauri-drag-region></div>

    <label class="search">
      <Glyph name="search" size={14} />
      <input class="input" placeholder={t("Buscar projetos")} bind:value={query} bind:this={searchInput} />
    </label>
    <button class="btn subtle icon" title={t("Atualizar lista (F5)")} onclick={scan} disabled={scanning}>
      <span class:spin={scanning}><Glyph name="refresh" /></span>
    </button>

    <span class="divider"></span>

    <button
      class="btn subtle icon badge-host"
      title={phpUpdates
        ? phpUpdates === 1
          ? t("Versões do PHP — 1 atualização disponível")
          : t("Versões do PHP — {n} atualizações disponíveis", { n: phpUpdates })
        : t("Versões do PHP")}
      onclick={() => api.openSettings("php")}
    >
      <Glyph name="code" />
      {#if phpUpdates}<span class="badge-dot"></span>{/if}
    </button>
    <button class="btn subtle icon" title={t("Serviços")} onclick={() => api.openSettings("services")}>
      <Glyph name="database" />
    </button>
    <button class="btn subtle icon" title={t("Configurações")} onclick={() => api.openSettings("general")}>
      <Glyph name="settings" />
    </button>
  </div>

  {#if app.error}
    <div class="error" role="alert">
      <Glyph name="info" />
      <span>{app.error.charAt(0).toUpperCase() + app.error.slice(1)}</span>
      <button class="btn subtle" onclick={() => (app.error = null)}>{t("Fechar")}</button>
    </div>
  {/if}

  {#if update?.available && update.latest && dismissedUpdate !== update.latest}
    <div class="newbar updatebar">
      <Glyph name="refresh" />
      <span>{t("Versão {v} do Larafim disponível.", { v: update.latest })}</span>
      <button class="btn accent" onclick={() => updates.openLink("update")}>{t("Baixar")}</button>
      <button class="btn subtle" onclick={() => api.openSettings("about")}>{t("Detalhes")}</button>
      <button class="btn subtle icon" title={t("Lembrar depois")} onclick={dismissUpdate}><Glyph name="close" size={12} /></button>
    </div>
  {/if}

  <div class="body">
    <section class="list" aria-label={t("Projetos")}>
      {#if !app.config}
        <div class="empty caption">{t("Carregando…")}</div>
      {:else if app.config.roots.length === 0 && projects.length === 0}
        <div class="empty">
          <Glyph name="folderOpen" size={40} />
          <h2>{t("Adicione sua pasta de projetos")}</h2>
          <p class="caption">
            {t("Escolha onde ficam seus projetos, por exemplo")} <span class="mono">D:\www</span>{t(". O Larafim lista o que encontrar e não liga nada sozinho. Também dá para adicionar um projeto avulso, de qualquer pasta.")}
          </p>
          <div class="empty-actions">
            <button class="btn accent" onclick={addRoot}>{t("Escolher pasta de projetos")}</button>
            <button class="btn" onclick={addProject}>{t("Adicionar projeto avulso")}</button>
          </div>
        </div>
      {:else}
        {#if newCount > 0}
          <div class="newbar">
            <Glyph name="info" />
            <span>
              {newCount === 1
                ? t("1 projeto novo encontrado. Importe para configurar e deixar online.")
                : t("{n} projetos novos encontrados. Importe para configurar e deixar online.", { n: newCount })}
            </span>
            <button class="btn" onclick={() => run(projectsApi.importAll())}>{t("Importar todos")}</button>
          </div>
        {/if}

        <div class="columns section-title">
          <span></span>
          <span>{t("Nome")}</span>
          <span>{t("Tipo")}</span>
          <span>PHP</span>
          <span>{t("Endereço")}</span>
        </div>

        <div
          class="rows"
          role="listbox"
          tabindex="0"
          aria-label={t("Lista de projetos")}
          onkeydown={(e) => {
            if (e.key === "ArrowDown") (e.preventDefault(), moveSelection(1));
            if (e.key === "ArrowUp") (e.preventDefault(), moveSelection(-1));
          }}
        >
          {#each groups as group (group.key)}
            <div class="group">
              <Glyph name={group.avulso ? "pin" : "folder"} size={12} />
              <span class="root" class:mono={!group.avulso} title={group.label}>{group.label}</span>
              <span class="count">{group.items.length}</span>
            </div>
            {#each group.items as p (p.path)}
              <div
                id="row-{p.path}"
                class="row"
                class:selected={p.path === selectedPath}
                class:pending={p.status !== "imported"}
                role="option"
                tabindex="-1"
                aria-selected={p.path === selectedPath}
                onclick={() => (selectedPath = p.path)}
                onkeydown={() => {}}
              >
                <span class="dot" class:on={p.online} title={p.online ? t("Online") : t("Offline")}></span>
                <span class="name">
                  <span class="label">{p.name}</span>
                  {#if p.status === "new"}<span class="tag new">{t("Novo")}</span>{/if}
                  {#if p.status === "missing"}<span class="tag missing">{t("Pasta não encontrada")}</span>{/if}
                </span>
                <span class="cell">{kindLabel[p.kind]}{p.kind !== "node" && p.has_node ? " + Node" : ""}</span>
                <span class="cell" title={p.php_pin ? t("Versão fixada") : t("Pedida pelo projeto")}>
                  {p.kind === "node" ? "—" : (p.php_pin ?? p.php_request ?? "auto")}
                </span>
                <span class="cell url">{projectUrl(app.config, p.domain)}</span>
              </div>
            {:else}
              <div class="group-empty caption">
                {query ? t("Nada encontrado nesta pasta.") : t("Nenhum projeto encontrado nesta pasta.")}
              </div>
            {/each}
          {/each}
        </div>
      {/if}
    </section>

    <Inspector
      project={selected}
      tick={phpVersionTick}
      onchange={apply}
      onreview={(path) => review(path)}
    />
  </div>

  <footer class="statusbar caption">
    <span class="dot" class:on={status?.running}></span>
    {#if status?.running}
      <span>{t("Servidor ligado · porta {port}", { port: status.port })}</span>
      <span class="sep"></span>
      <span>{plural(status.online.length, "{n} online", "{n} online")}</span>
      {#if status.node_running}
        <span class="sep"></span>
        <span>{t("Node ativo: {n}", { n: status.node_running })}</span>
      {/if}
      {#if status.php_running.length}
        <span class="sep"></span>
        <span title={t("Versões do PHP com processos rodando agora")}>{t("PHP ativo: {list}", { list: status.php_running.join(", ") })}</span>
      {/if}
      <button class="link" onclick={() => server.stopAll()}>{t("Desligar todos")}</button>
    {:else}
      <span>{t("Servidor desligado")}</span>
    {/if}
    {#if status?.hosts_outdated}
      <span class="sep"></span>
      <button class="link warn" onclick={() => fixHosts()} title={t("Atualiza o arquivo hosts do Windows (pede permissão)")}>
        {t("Endereços .{tld} desatualizados — corrigir", { tld: app.config?.tld ?? "" })}
      </button>
    {/if}
    <span class="sep"></span>
    <span>{plural(projects.filter((p) => p.status === "imported").length, "{n} importado", "{n} importados")}</span>
    {#if newCount}<span class="sep"></span><span>{plural(newCount, "{n} novo", "{n} novos")}</span>{/if}
    <span class="sep"></span>
    <span>{t("Domínio .{tld}", { tld: app.config?.tld ?? "" })}</span>
    <span class="grow"></span>
    {#if app.saving}<span>{t("Salvando…")}</span>{/if}
    <span>Larafim {app.info?.version}</span>
  </footer>
</div>

{#if creating}
  <NewLaravelDialog
    onclose={() => (creating = false)}
    ondone={async (path) => {
      creating = false;
      await scan();
      selectedPath = projects.find((p) => p.path.toLowerCase() === path.toLowerCase())?.path ?? selectedPath;
    }}
  />
{/if}

{#if proposal}
  <AddProjectDialog
    {proposal}
    onclose={() => (proposal = null)}
    onsaved={(data, path) => {
      apply(data);
      selectedPath = path;
      proposal = null;
    }}
  />
{/if}

<style>
  .window {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 48px;
    padding: 0 8px 8px;
    flex: none;
  }
  .grow {
    flex: 1;
    align-self: stretch;
  }
  .divider {
    width: 1px;
    height: 20px;
    margin: 0 6px;
    background: var(--stroke-strong);
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    margin-right: 4px;
  }
  .search :global(.glyph) {
    position: absolute;
    left: 10px;
    color: var(--text-3);
  }
  .search input {
    width: 240px;
    padding-left: 32px;
  }
  .badge-host {
    position: relative;
  }
  .badge-dot {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--win-bg);
  }
  .spin {
    display: inline-flex;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .error {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 8px 8px;
    padding: 6px 6px 6px 12px;
    border-radius: var(--r-card);
    background: color-mix(in srgb, var(--err) 14%, var(--card));
    border: 1px solid var(--stroke);
    user-select: text;
  }
  .error span:not(:global(.glyph)) {
    flex: 1;
  }
  .error :global(.glyph) {
    color: var(--err);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    background: var(--layer);
    border-top: 1px solid var(--stroke);
    border-top-left-radius: 8px;
    border-left: 1px solid var(--stroke);
  }

  .list {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .newbar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 10px 12px 2px;
    padding: 6px 6px 6px 12px;
    border-radius: var(--r-card);
    border: 1px solid var(--stroke);
    background: var(--accent-soft);
    flex: none;
  }
  .updatebar {
    margin: 10px 12px 0;
  }
  .newbar span {
    flex: 1;
  }
  .newbar :global(.glyph) {
    color: var(--accent-text);
  }

  .columns,
  .row {
    display: grid;
    grid-template-columns: 20px minmax(180px, 1.5fr) minmax(90px, 0.7fr) 64px minmax(160px, 1.4fr);
    align-items: center;
    gap: 12px;
    padding: 0 16px 0 20px;
  }
  .columns {
    height: 36px;
    flex: none;
    border-bottom: 1px solid var(--stroke);
  }

  .rows {
    flex: 1;
    overflow: auto;
    padding: 4px 6px 12px;
    outline: none;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 8px 14px 0;
    color: var(--text-3);
  }
  .root {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    padding: 0 6px;
    border-radius: 8px;
    background: var(--subtle-hover);
  }
  .group-empty {
    padding: 6px 14px 10px 36px;
  }

  .row {
    position: relative;
    height: 36px;
    padding: 0 10px 0 14px;
    border-radius: var(--r-ctrl);
  }
  .row:hover {
    background: var(--subtle-hover);
  }
  .row.selected {
    background: var(--selected);
  }
  .row.selected::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    width: 3px;
    height: 16px;
    border-radius: 2px;
    background: var(--accent);
    transform: translateY(-50%);
  }
  .rows:focus-visible .row.selected {
    outline: 2px solid var(--text);
    outline-offset: -2px;
  }
  .row.pending .label,
  .row.pending .cell {
    color: var(--text-3);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .label {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    flex: none;
    font-size: 11px;
    line-height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    font-weight: 600;
  }
  .tag.new {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .tag.missing {
    background: color-mix(in srgb, var(--warn) 16%, var(--card));
    color: var(--warn);
  }
  .cell {
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .url {
    font-family: var(--mono);
    font-size: 12px;
  }

  .empty {
    margin: auto;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 8px;
    color: var(--text-3);
  }
  .empty h2 {
    margin: 8px 0 0;
    font: 600 16px/22px var(--font-display);
    color: var(--text);
  }
  .empty p {
    margin: 0 0 12px;
  }
  .empty-actions {
    display: flex;
    gap: 8px;
  }

  .statusbar {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 28px;
    padding: 0 12px;
    flex: none;
    background: var(--layer);
    border-top: 1px solid var(--stroke);
  }
  .link {
    border: 0;
    padding: 0 4px;
    background: none;
    font: inherit;
    color: var(--accent-text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .link.warn {
    color: var(--warn);
  }
  .sep {
    width: 1px;
    height: 12px;
    background: var(--stroke-strong);
  }
</style>
