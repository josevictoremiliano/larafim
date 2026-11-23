<script lang="ts">
  import { locale, t } from "../lib/i18n.svelte";
  import { onEvent } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import { nodeApi, type NodeDone, type NodeOverview, type NodeProgress } from "../lib/node";
  import { confirm } from "../lib/php";
  import ProgressBar from "../lib/ProgressBar.svelte";

  let overview = $state<NodeOverview | null>(null);
  let loading = $state(true);
  let progress = $state<Record<string, { received: number; total: number | null } | "starting">>({});
  let errors = $state<Record<string, string>>({});

  async function load(refresh = false) {
    loading = true;
    try {
      overview = await nodeApi.overview(refresh);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
    const stops = [
      onEvent<NodeProgress>("node:progress", (p) => (progress[p.major] = { received: p.received, total: p.total })),
      onEvent<NodeDone>("node:done", (d) => {
        delete progress[d.major];
        if (d.ok) delete errors[d.major];
        else if (d.error) errors[d.major] = d.error;
        load();
      }),
    ];
    return () => stops.forEach((s) => s());
  });

  async function install(major: string) {
    delete errors[major];
    progress[major] = "starting";
    try {
      await nodeApi.install(major);
    } catch (e) {
      delete progress[major];
      errors[major] = String(e);
    }
  }

  async function remove(major: string, version: string) {
    const ok = await confirm(t("Remover o Node {v}? Projetos que usam essa versão vão pedir outra.", { v: version }), t("Remover Node.js"), t("Remover"));
    if (!ok) return;
    await nodeApi.remove(major);
    load();
  }

  const mb = (n: number) => (n / 1024 / 1024).toLocaleString(locale(), { maximumFractionDigits: 1 });
</script>

<h1>Node.js</h1>
<p class="lead caption">
  {t("Versões oficiais de nodejs.org (zip para Windows, conferido pelo SHA-256), com npm e npx. Cada projeto usa a do")}
  <span class="mono">.nvmrc</span> / <span class="mono">engines</span>{t(", ou a que você fixar.")}
</p>

{#if overview?.catalog_error}
  <div class="infobar warn">
    <Glyph name="info" />
    <span>{t("Não foi possível consultar as versões ({error}).", { error: overview.catalog_error ?? "" })}</span>
    <button class="btn" onclick={() => load(true)}>{t("Tentar de novo")}</button>
  </div>
{/if}

{#if overview}
  <div class="table card">
    <div class="thead section-title">
      <span>{t("Versão")}</span>
      <span>{t("Instalada")}</span>
      <span class="right">
        <button class="btn subtle check" onclick={() => load(true)} disabled={loading}>
          <Glyph name="refresh" size={12} /> {t("Verificar atualizações")}
        </button>
      </span>
    </div>
    {#each overview.rows as row (row.major)}
      {@const p = progress[row.major]}
      <div class="tr">
        <div class="version">
          <strong>Node {row.major}</strong>
          <span class="caption">{row.lts ? `LTS ${row.lts}` : "Current"}{row.latest ? ` · ${row.latest}` : ""}</span>
        </div>
        <div class="installed">
          {#if row.installed}
            <span class="ok"><Glyph name="check" size={12} /> {row.installed}</span>
            {#if row.update_available}<span class="badge">{t("{v} disponível", { v: row.latest ?? "" })}</span>{/if}
          {:else}
            <span class="caption">—</span>
          {/if}
        </div>
        <div class="actions">
          {#if p}
            <div class="progress">
              <span class="caption">{p === "starting" ? t("Preparando…") : (p.total ? t("Baixando {a} de {b} MB", { a: mb(p.received), b: mb(p.total) }) : t("Baixando {a} MB", { a: mb(p.received) }))}</span>
              <ProgressBar value={p === "starting" || !p.total ? null : p.received / p.total} />
            </div>
            <button class="btn subtle icon" title={t("Cancelar")} onclick={() => nodeApi.cancel(row.major)}>
              <Glyph name="close" size={12} />
            </button>
          {:else if row.installed}
            {#if row.update_available}<button class="btn accent" onclick={() => install(row.major)}>{t("Atualizar")}</button>{/if}
            <button class="btn subtle icon" title={t("Remover")} onclick={() => remove(row.major, row.installed!)}>
              <Glyph name="delete" />
            </button>
          {:else if row.latest}
            <button class="btn" onclick={() => install(row.major)}>{t("Instalar")}</button>
          {/if}
        </div>
        {#if errors[row.major]}
          <div class="row-error"><Glyph name="info" size={14} /><span>{errors[row.major]}</span></div>
        {/if}
      </div>
    {/each}
  </div>
  <p class="tip caption">
    {t("No terminal,")} <span class="mono">node</span>, <span class="mono">npm</span> {t("e")} <span class="mono">npx</span> {t("usam a versão da pasta (Configurações › Terminal). Para forçar:")} <span class="mono">$env:LARAFIM_NODE = "22"</span>.
  </p>
{:else if loading}
  <div class="loading"><ProgressBar /></div>
{/if}

<style>
  h1 {
    margin: 0 0 16px;
    font: 600 28px/36px var(--font-display);
  }
  .lead {
    margin: -8px 0 12px;
  }
  .infobar {
    margin-bottom: 8px;
    align-items: center;
  }
  .infobar span {
    flex: 1;
  }
  .infobar.warn {
    background: color-mix(in srgb, var(--warn) 12%, var(--card));
  }
  .table {
    overflow: hidden;
  }
  .thead,
  .tr {
    display: grid;
    grid-template-columns: 150px 1fr minmax(240px, auto);
    align-items: center;
    gap: 12px;
    padding: 0 12px 0 16px;
  }
  .thead {
    height: 40px;
    background: var(--layer);
    border-bottom: 1px solid var(--stroke);
  }
  .right {
    justify-self: end;
  }
  .check {
    height: 28px;
    font-size: 12px;
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    color: var(--text-2);
  }
  .tr {
    min-height: 56px;
    padding-top: 8px;
    padding-bottom: 8px;
  }
  .tr + .tr {
    border-top: 1px solid var(--stroke);
  }
  .version {
    display: flex;
    flex-direction: column;
  }
  .installed {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .ok :global(.glyph) {
    color: var(--ok);
  }
  .badge {
    font-size: 12px;
    line-height: 20px;
    padding: 0 8px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }
  .progress {
    width: 220px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row-error {
    grid-column: 1 / -1;
    display: flex;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--r-ctrl);
    background: color-mix(in srgb, var(--err) 10%, var(--card));
    font-size: 12px;
  }
  .tip {
    margin-top: 16px;
  }
  .loading {
    width: 200px;
  }
</style>
