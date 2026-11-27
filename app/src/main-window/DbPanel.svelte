<script lang="ts">
  import { locale, t } from "../lib/i18n.svelte";
  import Glyph from "../lib/Glyph.svelte";
  import { confirm } from "../lib/php";
  import type { Project } from "../lib/projects";
  import { onEvent } from "../lib/api";
  import {
    services,
    type Backup,
    type ConnectionInfo,
    type DbKind,
    type DbPreview,
    type OwnDb,
    type OwnStatus,
  } from "../lib/services";
  import { pg } from "../lib/pg";

  // Banco de dados do projeto: escolher (com prévia do .env), backup,
  // restaurar e abrir no Adminer.
  let { project }: { project: Project } = $props();

  let preview = $state<{ kind: DbKind; data: DbPreview } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let backups = $state<Backup[]>([]);
  let hasEnvBackup = $state(false);
  let restoreFile = $state("");
  let connections = $state<ConnectionInfo[]>([]);
  /** Força o seletor a voltar ao valor gravado quando a pessoa cancela. */
  let selectKey = $state(0);

  const current = $derived<DbKind>((project.database as DbKind | null) ?? "none");
  const external = $derived(current.startsWith("external:") ? connections.find((c) => `external:${c.id}` === current) : undefined);
  const managed = $derived(
    current === "mariadb" || current === "mariadb-own" || current === "postgres" || current.startsWith("external:"),
  );
  let pgInstalled = $state(false);
  const isOwn = $derived(current === "mariadb-own");

  // MariaDB próprio: versões instaladas, valores do formulário e status.
  let installedMajors = $state<string[]>([]);
  let own = $state<OwnDb>({ version: "", port: 3310 });
  let ownStatus = $state<OwnStatus | null>(null);

  async function loadOwn() {
    const o = await services.overview().catch(() => null);
    installedMajors = o?.rows.filter((r) => r.installed).map((r) => r.major) ?? [];
    ownStatus = o?.own.find((x) => x.path.toLowerCase() === project.path.toLowerCase()) ?? null;
    const p = await pg.overview().catch(() => null);
    pgInstalled = !!p?.rows.some((r) => r.installed);
  }

  $effect(() => {
    void project.path;
    loadOwn();
    return onEvent("services-changed", () => loadOwn());
  });

  /** Recalcula a prévia quando versão/porta do servidor próprio mudam. */
  async function refreshOwnPreview() {
    if (preview?.kind !== "mariadb-own") return;
    preview = { kind: "mariadb-own", data: await services.dbPreview(project.path, "mariadb-own", own) };
  }
  const dbName = $derived(project.domain.replace(/[^a-z0-9]/gi, "_").toLowerCase());

  async function loadExtras() {
    hasEnvBackup = await services.envHasBackup(project.path).catch(() => false);
    connections = await services.connections().catch(() => []);
    backups = managed ? await services.backups(project.path).catch(() => []) : [];
    restoreFile = backups[0]?.file ?? "";
  }

  $effect(() => {
    void project.path;
    void project.database;
    error = null;
    notice = null;
    loadExtras();
  });

  async function choose(kind: DbKind) {
    error = null;
    if (kind === "none") {
      await act(() => services.dbApply(project.path, "none"), t("O Larafim deixou de gerenciar o banco deste projeto (o .env não foi alterado)."));
      return;
    }
    try {
      if (kind === "postgres" && !pgInstalled) {
        error = t("Instale o PostgreSQL em Configurações › Serviços.");
        selectKey++;
        return;
      }
      if (kind === "mariadb-own") {
        await loadOwn();
        own = {
          version: project.db_version ?? installedMajors[installedMajors.length - 1] ?? "",
          port: project.db_port ?? (await services.suggestOwnPort(project.path)),
        };
        if (!own.version) {
          error = t("Instale ao menos uma versão do MariaDB em Configurações › Serviços.");
          selectKey++;
          return;
        }
      }
      preview = { kind, data: await services.dbPreview(project.path, kind, kind === "mariadb-own" ? own : null) };
    } catch (e) {
      error = String(e);
    }
  }

  async function act(fn: () => Promise<unknown>, done: string) {
    busy = true;
    error = null;
    notice = null;
    try {
      await fn();
      notice = done;
      await loadExtras();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function cancelPreview() {
    preview = null;
    selectKey++;
  }

  async function apply() {
    if (!preview) return;
    const kind = preview.kind;
    const ownValue = kind === "mariadb-own" ? { ...own, port: Number(own.port) } : null;
    preview = null;
    await act(
      () => services.dbApply(project.path, kind, ownValue),
      kind === "sqlite" ? t("SQLite configurado no .env.") : t("Banco {db} pronto e .env atualizado.", { db: dbName }),
    );
  }

  async function restore() {
    const b = backups.find((x) => x.file === restoreFile);
    if (!b) return;
    const ok = await confirm(
      t('Restaurar "{file}" no banco {db}? As tabelas que estão no arquivo serão substituídas pelas do backup.', { file: b.name, db: dbName }),
      "Restaurar backup",
      "Restaurar",
    );
    if (ok) act(() => services.restore(project.path, b.file), `Backup ${b.name} restaurado.`);
  }

  async function undoEnv() {
    const ok = await confirm(
      t("Voltar o .env para como estava antes da primeira mudança feita pelo Larafim?"),
      t("Desfazer mudanças no .env"),
      "Desfazer",
    );
    if (ok) act(() => services.envUndo(project.path), ".env restaurado.");
  }

  const when = (s: number) => new Date(s * 1000).toLocaleString(locale(), { dateStyle: "short", timeStyle: "short" });
  const kb = (n: number) => `${Math.max(1, Math.round(n / 1024)).toLocaleString(locale())} KB`;
</script>

<div class="field">
  <span>{t("Banco de dados")}</span>
  {#key selectKey}
    <div class="select">
      <select value={current} disabled={busy} onchange={(e) => choose(e.currentTarget.value as DbKind)}>
        <option value="none">{t("Não gerenciado")}</option>
        <!-- Grupos deixam claro quem divide o servidor com outros projetos. -->
        <optgroup label={t("Só deste projeto")}>
          <option value="sqlite">{t("SQLite (arquivo no projeto)")}</option>
          <option value="mariadb-own">{t("MariaDB próprio")}</option>
        </optgroup>
        <optgroup label={t("Compartilhado entre projetos")}>
          <option value="mariadb">MariaDB</option>
          <option value="postgres">PostgreSQL</option>
        </optgroup>
        {#if connections.length}
          <optgroup label={t("Servidores existentes")}>
            {#each connections as c (c.id)}
              <option value={`external:${c.id}`}>{c.name}</option>
            {/each}
          </optgroup>
        {/if}
      </select>
    </div>
  {/key}
</div>
{#if current === "mariadb" || current === "postgres"}
  <p class="caption hint">{t("Servidor compartilhado com outros projetos; este projeto usa um banco só dele.")}</p>
{/if}
{#if external}
  <p class="caption hint">{external.kind === "mysql" ? "MySQL" : "MariaDB"} · {external.user}@{external.host}:{external.port}</p>
{/if}
{#if isOwn}
  <div class="own">
    <span class="dot" class:on={ownStatus?.running}></span>
    <span class="caption grow">
      {t("MariaDB {v} só deste projeto · porta {port} · {state}", { v: project.db_version ?? "", port: project.db_port ?? "", state: ownStatus?.running ? t("ligado") : t("desligado") })}
    </span>
    {#if ownStatus?.running}
      <button class="btn subtle" disabled={busy} onclick={() => act(() => services.ownStop(project.path), "")}>{t("Desligar")}</button>
    {:else}
      <button class="btn subtle" disabled={busy} onclick={() => act(() => services.ownStart(project.path), "")}>{t("Ligar")}</button>
    {/if}
    <button class="btn subtle icon" title={t("Trocar versão ou porta")} disabled={busy} onclick={() => choose("mariadb-own")}>
      <Glyph name="settings" />
    </button>
  </div>
{/if}

{#if managed}
  <div class="db card">
    <div class="db-head">
      <Glyph name="database" />
      <span class="mono name">{dbName}</span>
      <span class="grow"></span>
      <button
        class="btn subtle icon"
        title={external ? t("Abrir no Adminer (pede a senha desse servidor)") : t("Abrir no Adminer")}
        disabled={busy}
        onclick={() => act(() => services.openAdminer(dbName, current === "postgres" ? "postgres" : (external?.id ?? null), isOwn ? project.path : null), "")}
      >
        <Glyph name="openExternal" />
      </button>
      <button class="btn subtle" disabled={busy} onclick={() => act(() => services.backup(project.path), t("Backup criado."))}>
        {t("Backup")}
      </button>
    </div>
    {#if backups.length}
      <div class="restore">
        <div class="select">
          <select bind:value={restoreFile}>
            {#each backups as b (b.file)}<option value={b.file}>{when(b.modified)} · {kb(b.size)}</option>{/each}
          </select>
        </div>
        <button class="btn" disabled={busy} onclick={restore}>{t("Restaurar")}</button>
        <button class="btn subtle icon" title={t("Abrir pasta dos backups")} onclick={() => services.openBackups(project.path)}>
          <Glyph name="folderOpen" />
        </button>
      </div>
    {:else}
      <p class="caption">{t("Nenhum backup ainda.")}</p>
    {/if}
  </div>
{/if}

{#if hasEnvBackup}
  <button class="link caption" onclick={undoEnv}>{t("Desfazer mudanças do Larafim no .env")}</button>
{/if}
{#if notice}<p class="caption ok">{notice}</p>{/if}
{#if error}<p class="caption err">{error}</p>{/if}

{#if preview}
  <div class="smoke" role="presentation" onclick={(e) => e.target === e.currentTarget && cancelPreview()}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="envdlg">
      <div class="content">
        <h2 id="envdlg">
          {preview.kind === "mariadb"
            ? t("Usar o MariaDB compartilhado")
            : preview.kind === "mariadb-own"
              ? t("MariaDB próprio do projeto")
              : preview.kind === "sqlite"
              ? t("Usar SQLite")
              : preview.kind === "postgres"
                ? t("Usar o PostgreSQL")
              : t("Usar {name}", { name: connections.find((c) => `external:${c.id}` === preview!.kind)?.name ?? t("banco existente") })}
        </h2>
        {#if preview.kind.startsWith("external:")}
          <p class="caption">
            {t("O banco")} <span class="mono">{preview.data.database}</span> {t("será criado nesse servidor (se ainda não existir).")}
          </p>
        {:else if preview.kind === "mariadb"}
          <p class="caption">
            {t("Um servidor para todos os projetos (mais leve), cada um com seu banco. O banco")}
            <span class="mono">{preview.data.database}</span> {t("será criado se ainda não existir.")}
          </p>
        {:else if preview.kind === "postgres"}
          <p class="caption">
            {t("O banco")} <span class="mono">{preview.data.database}</span> {t("será criado no PostgreSQL do Larafim (se ainda não existir), e as extensões")} <span class="mono">pgsql</span> {t("e")} <span class="mono">pdo_pgsql</span> {t("serão ligadas no PHP do projeto.")}
          </p>
        {:else if preview.kind === "mariadb-own"}
          <p class="caption">
            {t("Um servidor só deste projeto, com versão e dados separados. Útil quando o projeto precisa de outra versão do MariaDB. Gasta cerca de 150 MB a mais de memória enquanto estiver ligado.")}
          </p>
          <div class="own-form">
            <label>
              <span>{t("Versão")}</span>
              <div class="select">
                <select bind:value={own.version} onchange={refreshOwnPreview}>
                  {#each installedMajors as m (m)}<option value={m}>MariaDB {m}</option>{/each}
                </select>
              </div>
            </label>
            <label>
              <span>{t("Porta")}</span>
              <input class="input port" type="number" min="1024" max="65535" bind:value={own.port} onchange={refreshOwnPreview} />
            </label>
          </div>
          <p class="caption">{t("Outras versões podem ser instaladas em Configurações › Serviços.")}</p>
        {:else}
          <p class="caption">{t("O arquivo")} <span class="mono">database\database.sqlite</span> {t("será criado se não existir.")}</p>
        {/if}
        {#if preview.data.no_env}
          <p class="caption">{t("O projeto não tem .env: nada será escrito nele.")}</p>
        {:else}
          {#if preview.data.from_example}
            <p class="caption">{t("O .env será criado a partir do .env.example.")}</p>
          {/if}
          <div class="diff card">
            {#each preview.data.diff as d (d.key)}
              <div class="d mono">
                <span class="k">{d.key}</span>
                <span class="before">{d.before ?? "—"}</span>
                <Glyph name="chevronRight" size={10} />
                <span class="after">{d.after === null ? "(comentada)" : d.after === "" ? '""' : d.after}</span>
              </div>
            {:else}
              <div class="d caption">{t("O .env já está assim. Nada muda.")}</div>
            {/each}
          </div>
          <p class="caption">{t("Uma cópia do .env original fica em")} <span class="mono">.env.larafim.bak</span> {t("e dá para desfazer.")}</p>
        {/if}
      </div>
      <div class="footer">
        <button class="btn accent" onclick={apply}>{t("Aplicar")}</button>
        <button class="btn" onclick={cancelPreview}>{t("Cancelar")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
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
  .hint {
    margin: -4px 0 0;
    text-align: right;
  }
  .own {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
  }
  .own .grow {
    flex: 1;
  }
  .own-form {
    display: flex;
    gap: 16px;
    align-items: center;
  }
  .own-form label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .own-form .select {
    width: 160px;
  }
  .own-form .port {
    width: 90px;
  }
  .db {
    padding: 8px 8px 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .db-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex: 1;
  }
  .restore {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .restore .select {
    flex: 1;
    min-width: 0;
  }
  .restore .select select {
    min-width: 0;
  }
  p {
    margin: 0;
  }
  .ok {
    color: var(--ok);
  }
  .err {
    color: var(--err);
    user-select: text;
  }
  .link {
    align-self: flex-start;
    border: 0;
    padding: 0;
    background: none;
    color: var(--accent-text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .smoke {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.3);
  }
  .dialog {
    width: 520px;
    max-width: calc(100vw - 48px);
    border-radius: 8px;
    border: 1px solid var(--stroke);
    background: var(--card);
    box-shadow: var(--overlay-shadow), 0 32px 64px rgb(0 0 0 / 0.19);
    overflow: hidden;
  }
  .content {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h2 {
    margin: 0;
    font: 600 20px/28px var(--font-display);
  }
  .diff {
    padding: 4px 0;
    background: var(--layer);
  }
  .d {
    display: grid;
    grid-template-columns: 130px 1fr 12px 1fr;
    gap: 8px;
    align-items: center;
    padding: 4px 12px;
    font-size: 12px;
  }
  .k {
    font-weight: 600;
  }
  .before {
    color: var(--text-3);
    text-decoration: line-through;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .after {
    color: var(--accent-text);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .footer {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 24px;
    background: var(--win-bg);
    border-top: 1px solid var(--stroke);
  }
</style>
