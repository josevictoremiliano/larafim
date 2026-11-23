<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { onEvent } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import {
    confirm,
    php,
    stageFraction,
    stageLabel,
    type PhpDone,
    type PhpExtension,
    type PhpOverview,
    type PhpProgress,
    type PhpStage,
  } from "../lib/php";
  import ProgressBar from "../lib/ProgressBar.svelte";
  import { toolsApi, type XdebugInfo } from "../lib/tools";
  import Toggle from "../lib/Toggle.svelte";
  import SettingRow from "./SettingRow.svelte";

  let overview = $state<PhpOverview | null>(null);
  let loading = $state(true);
  let progress = $state<Record<string, PhpStage>>({});
  let errors = $state<Record<string, PhpDone>>({});
  let loadError = $state<string | null>(null);

  // Subpágina de extensões de uma versão instalada.
  let detail = $state<string | null>(null);
  let extensions = $state<PhpExtension[]>([]);
  let extFilter = $state("");
  let extSaved = $state(false);

  async function load(refresh = false) {
    loading = true;
    try {
      overview = await php.overview(refresh);
      loadError = null;
    } catch (e) {
      loadError = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
    const stops = [
      onEvent<PhpProgress>("php:progress", (p) => {
        const { minor, ...stage } = p;
        progress[minor] = stage as PhpStage;
      }),
      onEvent<PhpDone>("php:done", (d) => {
        delete progress[d.minor];
        if (d.ok || d.code === "cancelled") delete errors[d.minor];
        else errors[d.minor] = d;
        load();
      }),
    ];
    return () => stops.forEach((stop) => stop());
  });

  async function install(minor: string) {
    delete errors[minor];
    progress[minor] = { stage: "preparing" };
    try {
      await php.install(minor);
    } catch (e) {
      delete progress[minor];
      errors[minor] = { minor, ok: false, version: null, error: String(e), code: "error" };
    }
  }

  async function remove(minor: string, version: string) {
    const ok = await confirm(
      t("Remover o PHP {v}? Os arquivos desta versão (inclusive o php.ini) serão apagados. Seus projetos não são afetados.", { v: version }),
      t("Remover PHP"),
      t("Remover"),
    );
    if (!ok) return;
    try {
      await php.remove(minor);
      await load();
    } catch (e) {
      errors[minor] = { minor, ok: false, version: null, error: String(e), code: "error" };
    }
  }

  let xdebug = $state<XdebugInfo | null>(null);
  let xdebugBusy = $state(false);
  let xdebugError = $state<string | null>(null);
  let copied = $state(false);

  async function openDetail(version: string) {
    detail = version;
    extFilter = "";
    xdebugError = null;
    extensions = await php.extensions(version);
    xdebug = await toolsApi.xdebugInfo(version).catch(() => null);
    deprecations = await php.deprecations(version).catch(() => null);
  }

  // Avisos de obsolescência (E_DEPRECATED): só muda quando a pessoa pede.
  let deprecations = $state<boolean | null>(null);
  async function setDeprecations(show: boolean) {
    if (!detail) return;
    deprecations = await php.setDeprecations(detail, show);
    extSaved = true;
  }

  async function setXdebug(enabled: boolean) {
    if (!detail) return;
    xdebugBusy = true;
    xdebugError = null;
    try {
      xdebug = await toolsApi.xdebugSet(detail, enabled);
    } catch (e) {
      xdebugError = String(e);
    } finally {
      xdebugBusy = false;
    }
  }

  async function toggleExt(name: string, enabled: boolean) {
    if (!detail) return;
    extensions = await php.setExtension(detail, name, enabled);
    extSaved = true;
  }

  const visibleExts = $derived(extensions.filter((e) => e.name.includes(extFilter.trim().toLowerCase())));
</script>

{#if detail}
  <!-- Subpágina: extensões -->
  <h1 class="crumbs">
    <button class="crumb" onclick={() => (detail = null)}>PHP</button>
    <Glyph name="chevronRight" size={14} />
    <span>{detail}</span>
  </h1>

  <div class="detail-actions">
    <button class="btn" onclick={() => php.openIni(detail!)}><Glyph name="code" /> {t("Abrir php.ini")}</button>
    <button class="btn" onclick={() => php.openFolder(detail!)}><Glyph name="folderOpen" /> {t("Abrir pasta")}</button>
    <span class="grow"></span>
    <label class="search">
      <Glyph name="search" size={14} />
      <input class="input" placeholder={t("Filtrar extensões")} bind:value={extFilter} />
    </label>
  </div>

  <div class="xdebug card">
    <div class="xd-head">
      <Glyph name="bug" size={18} />
      <div class="xd-text">
        <strong>Xdebug</strong>
        <span class="caption">
          {xdebugBusy ? t("Baixando e configurando…") : xdebug?.enabled ? t("Ligado · porta 9003 · ativa quando a IDE pede (trigger)") : t("Desligado (deixa o PHP mais rápido)")}
        </span>
      </div>
      <Toggle label="Xdebug" checked={xdebug?.enabled ?? false} disabled={xdebugBusy} onchange={setXdebug} />
    </div>
    {#if xdebug?.enabled}
      <p class="caption">
        {t("No VS Code (extensão PHP Debug), use este")} <span class="mono">launch.json</span> {t("e inicie a depuração. No navegador, use a extensão \"Xdebug helper\" para ligar por página.")}
      </p>
      <div class="code">
        <pre>{xdebug.launch_json}</pre>
        <button
          class="btn subtle icon"
          title={copied ? t("Copiado!") : t("Copiar")}
          onclick={() => navigator.clipboard.writeText(xdebug!.launch_json).then(() => ((copied = true), setTimeout(() => (copied = false), 1500)))}
        >
          <Glyph name={copied ? "check" : "copy"} />
        </button>
      </div>
    {/if}
    {#if xdebugError}<p class="caption xd-err">{xdebugError}</p>{/if}
  </div>

  {#if deprecations !== null}
    <div class="xdebug card">
      <div class="xd-head">
        <Glyph name="info" size={18} />
        <div class="xd-text">
          <strong>{t("Avisos de obsolescência")}</strong>
          <span class="caption">
            {deprecations
              ? t("Aparecem na página (E_DEPRECATED). Útil para preparar o código para o próximo PHP.")
              : t("Escondidos da página; o Laravel continua registrando no log dele.")}
          </span>
        </div>
        <Toggle label={t("Avisos de obsolescência")} checked={deprecations} onchange={setDeprecations} />
      </div>
    </div>
  {/if}

  {#if extSaved}
    <div class="infobar">
      <Glyph name="info" />
      <span>{t("Salvo no php.ini. Vale na próxima vez que o PHP iniciar (terminal ou projeto reiniciado).")}</span>
    </div>
  {/if}

  <div class="ext-list card">
    {#each visibleExts as ext (ext.name)}
      <div class="ext">
        <span class="mono">{ext.name}</span>
        {#if ext.zend}<span class="tag">zend</span>{/if}
        <span class="grow"></span>
        <span class="caption">{ext.enabled ? t("Ligada") : t("Desligada")}</span>
        <Toggle label={t("Extensão {name}", { name: ext.name })} checked={ext.enabled} onchange={(v) => toggleExt(ext.name, v)} />
      </div>
    {:else}
      <div class="ext caption">{t("Nenhuma extensão encontrada.")}</div>
    {/each}
  </div>
{:else}
  <h1>PHP</h1>
  <p class="lead caption">
    {t("Versões oficiais para Windows (NTS x64), baixadas de downloads.php.net e conferidas pelo SHA-256. Cada projeto usa a versão mais nova instalada que atende ao que ele pede.")}
  </p>

  {#if loadError || overview?.catalog_error}
    <div class="infobar warn">
      <Glyph name="info" />
      <span>
        Não foi possível consultar o catálogo de versões ({overview?.catalog_error ?? loadError}).
        {#if overview?.rows.length}Mostrando só o que já está instalado.{/if}
      </span>
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

      {#each overview.rows as row (row.minor)}
        {@const p = progress[row.minor]}
        {@const err = errors[row.minor]}
        <div class="tr">
          <div class="version">
            <strong>{row.minor}</strong>
            {#if row.latest}<span class="caption">{t("Última: {v}", { v: row.latest })}</span>{/if}
          </div>

          <div class="installed">
            {#if row.installed}
              <span class="ok"><Glyph name="check" size={12} /> {row.installed}</span>
              {#if row.update_available}
                <span class="badge">{t("{v} disponível", { v: row.latest ?? "" })}</span>
              {/if}
            {:else}
              <span class="caption">—</span>
            {/if}
          </div>

          <div class="actions">
            {#if p}
              <div class="progress">
                <span class="caption">{stageLabel(p)}</span>
                <ProgressBar value={stageFraction(p)} />
              </div>
              {#if p.stage === "downloading" || p.stage === "preparing"}
                <button class="btn subtle icon" title={t("Cancelar")} onclick={() => php.cancel(row.minor)}>
                  <Glyph name="close" size={12} />
                </button>
              {/if}
            {:else if row.installed}
              {#if row.update_available}
                <button class="btn accent" onclick={() => install(row.minor)}>{t("Atualizar")}</button>
              {/if}
              <button class="btn" onclick={() => openDetail(row.installed!)}>{t("Extensões")}</button>
              <button class="btn subtle icon" title={t("Abrir pasta")} onclick={() => php.openFolder(row.installed!)}>
                <Glyph name="folderOpen" />
              </button>
              <button class="btn subtle icon" title={t("Remover")} onclick={() => remove(row.minor, row.installed!)}>
                <Glyph name="delete" />
              </button>
            {:else if row.latest}
              <button class="btn" title={row.size ? t("Download de {size}", { size: row.size }) : undefined} onclick={() => install(row.minor)}>
                {t("Instalar")}
              </button>
            {/if}
          </div>

          {#if err}
            <div class="row-error">
              <Glyph name="info" size={14} />
              <span>{err.error}</span>
              {#if err.code === "vc_redist"}
                <button class="btn" onclick={() => php.openVcRedist()}>{t("Baixar Visual C++")}</button>
              {/if}
              <button class="btn subtle" onclick={() => install(row.minor)}>{t("Tentar de novo")}</button>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <div class="extras">
      <SettingRow
        icon="package"
        title="Composer"
        description={t("Baixado junto com a primeira versão do PHP. Também funciona no terminal.")}
      >
        <span class="caption">{overview.composer ? t("Instalado") : t("Não instalado")}</span>
      </SettingRow>
    </div>
  {:else if loading}
    <div class="loading"><ProgressBar /></div>
  {/if}
{/if}

<style>
  h1 {
    margin: 0 0 16px;
    font: 600 28px/36px var(--font-display);
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .crumbs :global(.glyph) {
    color: var(--text-3);
  }
  .crumb {
    border: 0;
    padding: 0;
    background: none;
    font: inherit;
    color: var(--text-2);
  }
  .crumb:hover {
    color: var(--text);
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
  .infobar.warn :global(.glyph) {
    color: var(--warn);
  }

  .table {
    overflow: hidden;
  }
  .thead,
  .tr {
    display: grid;
    grid-template-columns: 120px 1fr minmax(260px, auto);
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
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--r-ctrl);
    background: color-mix(in srgb, var(--err) 10%, var(--card));
    font-size: 12px;
    user-select: text;
  }
  .row-error span {
    flex: 1;
  }
  .row-error :global(.glyph) {
    color: var(--err);
  }

  .extras {
    margin-top: 16px;
  }
  .loading {
    width: 200px;
  }

  .detail-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .grow {
    flex: 1;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
  }
  .search :global(.glyph) {
    position: absolute;
    left: 10px;
    color: var(--text-3);
  }
  .search input {
    width: 200px;
    padding-left: 32px;
  }
  .ext-list {
    overflow: hidden;
  }
  .xdebug {
    padding: 12px 16px;
    margin-bottom: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .xd-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .xd-text {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .xdebug p {
    margin: 0;
  }
  .code {
    position: relative;
  }
  .code pre {
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--r-ctrl);
    background: var(--layer);
    border: 1px solid var(--stroke);
    font: 12px/18px var(--mono);
    user-select: text;
  }
  .code .btn {
    position: absolute;
    top: 6px;
    right: 6px;
  }
  .xd-err {
    color: var(--err);
  }
  .ext {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 0 16px;
  }
  .ext + .ext {
    border-top: 1px solid var(--stroke);
  }
  .tag {
    font-size: 11px;
    padding: 0 6px;
    border-radius: 8px;
    background: var(--subtle-hover);
    color: var(--text-2);
  }
</style>
