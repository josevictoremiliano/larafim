<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../lib/i18n.svelte";
  import { api, inTauri, onEvent } from "../lib/api";
  import Glyph, { type GlyphName } from "../lib/Glyph.svelte";
  import { app } from "../lib/state.svelte";
  import { php, type PhpRow } from "../lib/php";
  import { server, type ServerStatus } from "../lib/server";
  import { toolsApi, type MailpitStatus } from "../lib/tools";
  import { updates, type UpdateStatus } from "../lib/updates";

  // Painel da bandeja (clique esquerdo no ícone): atalhos do dia a dia.

  let status = $state<ServerStatus | null>(null);
  let mail = $state<MailpitStatus | null>(null);
  let phpRows = $state<PhpRow[]>([]);
  let error = $state<string | null>(null);
  let panel = $state<HTMLDivElement>();

  document.documentElement.classList.add("panel-window");

  let update = $state<UpdateStatus | null>(null);

  async function load() {
    updates.status().then((u) => (update = u)).catch(() => {});
    const [s, m, p] = await Promise.all([
      server.status().catch(() => null),
      toolsApi.mailpitStatus().catch(() => null),
      php.overview(false).catch(() => null),
    ]);
    status = s;
    mail = m;
    phpRows = (p?.rows ?? []).filter((r) => r.installed);
  }

  $effect(() => {
    load();
    const stops = [
      onEvent("tray:shown", () => ((error = null), load())),
      onEvent<ServerStatus>("server-status", (s) => (status = s)),
      onEvent("services-changed", load),
      onEvent<UpdateStatus>("app:update", (u) => (update = u)),
    ];
    return () => stops.forEach((s) => s());
  });

  // A janela tem a altura do conteúdo (o backend reposiciona junto ao ícone).
  $effect(() => {
    if (!panel || !inTauri) return;
    const fit = () => invoke("tray_fit", { height: Math.ceil(panel!.getBoundingClientRect().height) }).catch(() => {});
    const ro = new ResizeObserver(fit);
    ro.observe(panel);
    return () => ro.disconnect();
  });

  /** Ação que fecha o painel. */
  async function go(action: () => Promise<unknown>) {
    error = null;
    try {
      await action();
      if (inTauri) await invoke("tray_action", { action: "hide" });
    } catch (e) {
      error = String(e);
    }
  }
  const backend = (action: string) => () => (inTauri ? invoke("tray_action", { action }) : Promise.resolve());

  const online = $derived(status?.online ?? []);
  const currentPhp = $derived(app.config?.default_php ?? null);
  const host = (url: string) => url.replace(/^https?:\/\//, "");

  interface Item {
    icon: GlyphName;
    label: string;
    hint?: string;
    run: () => Promise<unknown>;
  }
  const groups = $derived<Item[][]>([
    ...(update?.available && update.latest
      ? [[{ icon: "refresh" as GlyphName, label: t("Atualizar o Larafim"), hint: update.latest, run: () => updates.install().catch(() => api.openSettings("about")) }]]
      : []),
    [
      { icon: "apps", label: t("Abrir Larafim"), run: backend("main") },
      { icon: "add", label: t("Novo projeto Laravel"), run: backend("new-laravel") },
    ],
    [
      {
        icon: "mail",
        label: t("E-mail"),
        hint: mail?.installed ? (mail.running ? t("ligado") : t("desligado")) : t("não instalado"),
        run: () => (mail?.installed ? toolsApi.openMailpit() : api.openSettings("services")),
      },
      { icon: "logs", label: t("Visualizador de logs"), run: () => api.openLogs() },
      { icon: "database", label: t("Serviços"), run: () => api.openSettings("services") },
    ],
  ]);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && backend("hide")()} />

<div class="panel" bind:this={panel}>
  {#each groups as g, i (i)}
    <div class="group">
      {#each g as item (item.label)}
        <button class="item" onclick={() => go(item.run)}>
          <span class="tile"><Glyph name={item.icon} /></span>
          <span class="label">{item.label}</span>
          {#if item.hint}<span class="hint">{item.hint}</span>{/if}
        </button>
      {/each}
    </div>
  {/each}

  <div class="group">
    <div class="group-title">
      <span>{online.length ? t("Online agora") : t("Nenhum projeto online")}</span>
      {#if online.length}
        <button class="mini" title={t("Desligar todos os projetos")} onclick={() => go(server.stopAll)}>
          <Glyph name="stop" size={12} />
          {t("Desligar todos")}
        </button>
      {/if}
    </div>
    {#each online as site (site.path)}
      <button class="item" title={site.url} onclick={() => go(() => server.openUrl(site.url))}>
        <span class="tile"><span class="dot on"></span></span>
        <span class="label ellipsis">{site.name}</span>
        <span class="hint ellipsis mono">{host(site.url)}</span>
      </button>
    {/each}
  </div>

  {#if phpRows.length}
    <div class="group php">
      <div class="group-title"><span>{t("PHP do terminal")}</span></div>
      <button class="item radio" class:on={currentPhp === null} onclick={() => app.update({ default_php: null })}>
        <span class="tile bare"><span class="bullet"></span></span>
        <span class="label">{t("Automático")}</span>
        <span class="hint">{t("o mais novo")}</span>
      </button>
      {#each phpRows as r (r.minor)}
        <button class="item radio" class:on={currentPhp === r.minor} onclick={() => app.update({ default_php: r.minor })}>
          <span class="tile bare"><span class="bullet"></span></span>
          <span class="label">{t("Usar PHP {v}", { v: r.minor })}</span>
          <span class="hint mono">{r.installed}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if error}<p class="error">{error}</p>{/if}

  <div class="footer">
    <span class="brand">Larafim</span>
    <span class="grow"></span>
    <button class="square" title={t("Configurações…")} onclick={() => go(() => api.openSettings("general"))}><Glyph name="settings" /></button>
    <button class="square" title={t("Sair do Larafim")} onclick={backend("quit")}><Glyph name="power" /></button>
  </div>
</div>

<style>
  /* Fundo translúcido do Windows (acrílico): a página não pinta o fundo. */
  :global(html.panel-window),
  :global(html.panel-window body) {
    background: transparent;
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
  }
  .group {
    display: flex;
    flex-direction: column;
    padding: 4px;
    border-radius: 8px;
    border: 1px solid var(--stroke);
    background: color-mix(in srgb, var(--card) 82%, transparent);
  }
  .group-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
    padding: 0 8px;
    font-size: 12px;
    color: var(--text-2);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 4px 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    text-align: left;
    color: var(--text);
  }
  .item:hover {
    background: var(--subtle-hover);
  }
  .item:active {
    background: var(--subtle-press);
  }
  .tile {
    flex: none;
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid var(--stroke);
    background: var(--ctrl);
  }
  .tile.bare {
    border-color: transparent;
    background: transparent;
  }
  .label {
    flex: 1;
    min-width: 0;
  }
  .hint {
    flex: none;
    max-width: 50%;
    font-size: 12px;
    color: var(--text-3);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .radio {
    min-height: 36px;
  }
  .bullet {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .radio.on {
    background: var(--selected);
    font-weight: 600;
  }
  .radio.on .bullet {
    background: var(--text);
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text-2);
    font-size: 12px;
  }
  .mini:hover {
    background: var(--subtle-hover);
    color: var(--text);
  }
  .error {
    margin: 0;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    background: color-mix(in srgb, var(--err) 14%, var(--card));
    user-select: text;
  }
  .footer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 2px 0 6px;
  }
  .brand {
    font-size: 12px;
    color: var(--text-2);
  }
  .grow {
    flex: 1;
  }
  .square {
    width: 40px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid var(--stroke);
    background: color-mix(in srgb, var(--card) 82%, transparent);
    color: var(--text);
  }
  .square:hover {
    background: var(--ctrl-hover);
  }
</style>
