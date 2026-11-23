<script lang="ts">
  import { untrack } from "svelte";
  import { locale, t } from "../lib/i18n.svelte";
  import { api, onEvent } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import { confirm } from "../lib/php";
  import ProgressBar from "../lib/ProgressBar.svelte";
  import {
    services,
    type ConnectionInfo,
    type ConnectionInput,
    type DetectedDb,
    type ServiceInfo,
    type ServicesOverview,
  } from "../lib/services";
  import ConnectionDialog from "./ConnectionDialog.svelte";
  import AddServiceDialog, { type AddChoice } from "./AddServiceDialog.svelte";
  import { toolsApi, type MailpitStatus } from "../lib/tools";
  import { pg, type PgOverview } from "../lib/pg";
  import { s3, type S3Status } from "../lib/s3";
  import { redis, type RedisStatus } from "../lib/redis";

  // Serviços: lista agrupada à esquerda, detalhes do selecionado à direita.

  // ---------- Estado de cada serviço ----------
  let overview = $state<ServicesOverview | null>(null);
  let pgView = $state<PgOverview | null>(null);
  let mail = $state<MailpitStatus | null>(null);
  let s3st = $state<S3Status | null>(null);
  let redisSt = $state<RedisStatus | null>(null);
  let redisPort = $state(6379);
  let connections = $state<ConnectionInfo[]>([]);
  let detected = $state<DetectedDb[]>([]);

  let busy = $state(false);
  let error = $state<string | null>(null);
  /** Downloads em andamento: MariaDB por versão ("11.8"), PostgreSQL com "pg" na frente. */
  let progress = $state<Record<string, { received: number; total: number | null } | "starting">>({});
  let s3progress = $state<{ received: number; total: number | null } | null>(null);
  let redisProgress = $state<{ received: number; total: number | null } | null>(null);
  let mailInstalling = $state(false);

  let port = $state(3306);
  let pgPort = $state(5432);
  let editing = $state<{ input: ConnectionInput; hasPassword: boolean } | null>(null);
  let adding = $state(false);

  async function load(refresh = false) {
    try {
      overview = await services.overview(refresh);
      port = overview.port;
    } catch (e) {
      error = String(e);
    }
  }
  async function loadPg(refresh = false) {
    try {
      pgView = await pg.overview(refresh);
      pgPort = pgView.port;
    } catch (e) {
      error = String(e);
    }
  }
  async function loadMail() {
    mail = await toolsApi.mailpitStatus().catch(() => null);
  }
  async function loadS3() {
    s3st = await s3.status().catch(() => null);
  }
  async function loadRedis() {
    redisSt = await redis.status().catch(() => null);
    if (redisSt?.config) redisPort = redisSt.config.port;
  }
  async function loadConnections() {
    connections = await services.connections().catch(() => []);
    detected = await services.detect().catch(() => []);
  }
  function loadAll() {
    load();
    loadPg();
    loadMail();
    loadS3();
    loadRedis();
    loadConnections();
    loadInfo();
  }

  $effect(() => {
    load();
    loadPg();
    loadMail();
    loadS3();
    loadRedis();
    loadConnections();
    const stops = [
      onEvent("services-changed", loadAll),
      onEvent<{ received: number; total: number | null }>("s3:progress", (p) => (s3progress = p)),
      onEvent<{ received: number; total: number | null }>("redis:progress", (p) => (redisProgress = p)),
      onEvent<{ major: string; received: number; total: number | null }>("pg:progress", (p) => {
        progress["pg" + p.major] = { received: p.received, total: p.total };
      }),
      onEvent<{ major: string; ok: boolean; error: string | null }>("pg:done", (d) => {
        delete progress["pg" + d.major];
        if (!d.ok) error = d.error;
        loadPg();
        loadInfo();
      }),
      onEvent<{ major: string; received: number; total: number | null }>("maria:progress", (p) => {
        progress[p.major] = { received: p.received, total: p.total };
      }),
      onEvent<{ major: string; ok: boolean; error: string | null }>("maria:done", (d) => {
        delete progress[d.major];
        if (!d.ok) error = d.error;
        load();
        loadInfo();
      }),
    ];
    return () => stops.forEach((s) => s());
  });

  async function run(action: () => Promise<unknown>) {
    busy = true;
    error = null;
    try {
      await action();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      loadAll();
    }
  }

  const mb = (n: number) => (n / 1024 / 1024).toLocaleString(locale(), { maximumFractionDigits: 0 });
  type Progress = { received: number; total: number | null } | "starting";
  const progressText = (p: Progress) =>
    p === "starting"
      ? t("Preparando…")
      : p.total
        ? t("Baixando {a} de {b} MB", { a: mb(p.received), b: mb(p.total) })
        : t("Baixando {a} MB", { a: mb(p.received) });
  const progressValue = (p: Progress) => (p === "starting" || !p.total ? null : p.received / p.total);

  // ---------- Lista ----------
  const mariaInstalled = $derived(overview?.rows.filter((r) => r.installed) ?? []);
  const pgInstalled = $derived(pgView?.rows.filter((r) => r.installed) ?? []);
  const mariaDownload = $derived(Object.entries(progress).find(([k]) => !k.startsWith("pg"))?.[1] ?? null);
  const pgDownload = $derived(Object.entries(progress).find(([k]) => k.startsWith("pg"))?.[1] ?? null);
  const s3Download = $derived<Progress | null>(s3progress ?? (s3st?.installing ? "starting" : null));
  const redisDownload = $derived<Progress | null>(redisProgress ?? (redisSt?.installing ? "starting" : null));

  type Kind = "mariadb" | "postgres" | "own" | "conn" | "mailpit" | "s3" | "redis";
  interface Item {
    id: string;
    kind: Kind;
    name: string;
    sub: string;
    running: boolean;
    /** Serviço de fora: o Larafim não liga nem desliga. */
    external?: boolean;
    download?: Progress | null;
  }
  interface Group {
    title: string;
    items: Item[];
  }

  const portSub = (running: boolean, version: string | null | undefined, p: number) =>
    running ? t("{v} · porta {port}", { v: version ?? "", port: p }) : t("Desligado · porta {port}", { port: p });

  const groups = $derived.by<Group[]>(() => {
    const db: Item[] = [];
    if (overview && (mariaInstalled.length || mariaDownload)) {
      db.push({
        id: "mariadb",
        kind: "mariadb",
        name: "MariaDB",
        sub: portSub(overview.status.running, overview.status.version, overview.port),
        running: overview.status.running,
        download: mariaDownload,
      });
    }
    if (pgView && (pgInstalled.length || pgDownload)) {
      db.push({
        id: "postgres",
        kind: "postgres",
        name: "PostgreSQL",
        sub: portSub(pgView.status.running, pgView.status.version, pgView.port),
        running: pgView.status.running,
        download: pgDownload,
      });
    }
    for (const o of overview?.own ?? []) {
      db.push({
        id: "own:" + o.path,
        kind: "own",
        name: t("MariaDB de {project}", { project: o.project }),
        sub: portSub(o.running, o.version, o.port),
        running: o.running,
      });
    }
    for (const c of connections) {
      db.push({
        id: "conn:" + c.id,
        kind: "conn",
        name: c.name,
        sub: `${c.kind === "mysql" ? "MySQL" : "MariaDB"} · ${c.host}:${c.port}`,
        running: false,
        external: true,
      });
    }
    const out: Group[] = [];
    if (db.length) out.push({ title: t("Bancos de dados"), items: db });
    if (mail?.installed || mailInstalling) {
      out.push({
        title: t("E-mail"),
        items: [
          {
            id: "mailpit",
            kind: "mailpit",
            name: "Mailpit",
            sub: mail?.installed
              ? (mail.running ? t("{v} · SMTP {smtp}", { v: mail.version ?? "", smtp: mail.smtp }) : t("Desligado · SMTP {smtp}", { smtp: mail.smtp }))
              : "",
            running: !!mail?.running,
            download: mailInstalling ? "starting" : null,
          },
        ],
      });
    }
    if (s3st?.installed || s3Download) {
      out.push({
        title: t("Storage"),
        items: [
          {
            id: "s3",
            kind: "s3",
            name: t("Storage S3"),
            sub: s3st?.installed
              ? (s3st.running ? t("RustFS {v} · API {endpoint}", { v: s3st.version ?? "", endpoint: s3st.endpoint.replace(/^https?:\/\//, "") }) : t("Desligado · RustFS {v}", { v: s3st.version ?? "" }))
              : "",
            running: !!s3st?.running,
            download: s3Download,
          },
        ],
      });
    }
    if (redisSt?.config) {
      const c = redisSt.config;
      const where = c.backend === "native" ? "Windows" : c.backend === "docker" ? "Docker" : `WSL · ${c.distro ?? ""}`;
      out.push({
        title: t("Filas e cache"),
        items: [
          {
            id: "redis",
            kind: "redis",
            name: "Redis",
            sub: redisSt.running ? t("{v} · porta {port}", { v: where, port: c.port }) : t("Desligado · porta {port}", { port: c.port }),
            running: redisSt.running,
            download: redisDownload,
          },
        ],
      });
    }
    return out;
  });
  const items = $derived(groups.flatMap((g) => g.items));

  let selectedId = $state<string | null>(null);
  const selected = $derived(items.find((i) => i.id === selectedId) ?? items[0] ?? null);

  function startStop(item: Item, start: boolean) {
    const path = item.id.slice(4);
    const actions: Record<Exclude<Kind, "conn">, [() => Promise<unknown>, () => Promise<unknown>]> = {
      mariadb: [services.start, services.stop],
      postgres: [pg.start, pg.stop],
      own: [() => services.ownStart(path), () => services.ownStop(path)],
      mailpit: [toolsApi.mailpitStart, async () => toolsApi.mailpitStop()],
      s3: [s3.start, s3.stop],
      redis: [redis.start, redis.stop],
    };
    if (item.kind === "conn") return;
    run(actions[item.kind][start ? 0 : 1]);
  }

  // ---------- Detalhes ----------
  let info = $state<ServiceInfo | null>(null);
  let infoFor = "";
  let logBox = $state<HTMLPreElement>();
  let copied = $state(false);

  async function loadInfo() {
    const item = selected;
    if (!item || item.download) {
      info = null;
      return;
    }
    const [service, project] =
      item.kind === "own" ? ["own", item.id.slice(4)] : item.kind === "conn" ? [item.id, null] : [item.kind, null];
    try {
      const got = await services.info(service, project);
      if (selected?.id === item.id) {
        info = got;
        infoFor = item.id;
      }
    } catch {
      if (selected?.id === item.id) info = null;
    }
  }

  $effect(() => {
    const id = selected?.id;
    const running = selected?.running;
    void selected?.download;
    untrack(() => {
      if (id !== infoFor) info = null;
      loadInfo();
    });
    if (!running) return;
    const timer = setInterval(loadInfo, 3000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    void info?.log;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });

  const envText = $derived(info?.env.map(([k, v]) => `${k}=${v}`).join("\n") ?? "");
  async function copyEnv() {
    await navigator.clipboard.writeText(envText + "\n");
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
  /** Só o fim do caminho ("…\data\mariadb\11.8"); o completo fica na dica. */
  const shortPath = (p: string) => {
    const parts = p.split(/[\\/]/).filter(Boolean);
    return parts.length > 3 ? "…\\" + parts.slice(-3).join("\\") : p;
  };

  const ownOf = (item: Item) => overview?.own.find((o) => "own:" + o.path === item.id) ?? null;
  const connOf = (item: Item) => connections.find((c) => "conn:" + c.id === item.id) ?? null;

  function commitPort() {
    if (!overview) return;
    const p = Number(port);
    if (!Number.isInteger(p) || p < 1024 || p > 65535) {
      port = overview.port;
      return;
    }
    if (p !== overview.port) run(() => services.configure(overview!.selected, p));
  }
  function commitRedis(backend: "native" | "docker" | "wsl", distro: string | null, p: number) {
    if (!Number.isInteger(p) || p < 1024 || p > 65535) {
      redisPort = redisSt?.config?.port ?? 6379;
      return;
    }
    run(async () => {
      await redis.configure(backend, backend === "wsl" ? (distro ?? redisSt?.distros[0] ?? null) : null, p);
      if (backend === "native" && !redisSt?.native_installed) await installRedis();
    });
  }
  async function installRedis() {
    redisProgress = { received: 0, total: null };
    try {
      await redis.install();
    } finally {
      redisProgress = null;
    }
  }
  async function removeRedis() {
    const ok = await confirm(
      t("Tirar o Redis do Larafim? Os dados (volume do Docker ou dump.rdb) ficam guardados."),
      t("Remover Redis"),
      t("Remover"),
    );
    if (ok) run(redis.remove);
  }

  function commitPgPort() {
    if (!pgView) return;
    const p = Number(pgPort);
    if (!Number.isInteger(p) || p < 1024 || p > 65535) {
      pgPort = pgView.port;
      return;
    }
    if (p !== pgView.port) run(() => pg.configure(pgView!.selected, p));
  }

  // ---------- Instalar / remover versões ----------
  async function install(major: string) {
    progress[major] = "starting";
    try {
      await services.install(major);
    } catch (e) {
      delete progress[major];
      error = String(e);
    }
  }
  async function pgInstall(major: string) {
    progress["pg" + major] = "starting";
    try {
      await pg.install(major);
    } catch (e) {
      delete progress["pg" + major];
      error = String(e);
    }
  }
  async function remove(major: string) {
    const ok = await confirm(
      t("Remover os programas do MariaDB {v}? Os bancos (dados) ficam guardados e voltam se você reinstalar.", { v: major }),
      t("Remover MariaDB"),
      t("Remover"),
    );
    if (ok) run(() => services.remove(major));
  }
  async function pgRemove(major: string) {
    const ok = await confirm(
      t("Remover os programas do PostgreSQL {v}? Os bancos (dados) ficam guardados e voltam se você reinstalar.", { v: major }),
      t("Remover PostgreSQL"),
      t("Remover"),
    );
    if (ok) run(() => pg.remove(major));
  }

  // ---------- Bancos que já existem ----------
  function useDetected(d: DetectedDb) {
    editing = {
      hasPassword: false,
      input: {
        id: null,
        name: t("{kind} (porta {port})", { kind: d.kind === "mysql" ? "MySQL" : "MariaDB", port: d.port }),
        kind: d.kind,
        host: "127.0.0.1",
        port: d.port,
        user: "root",
        password: null,
        client_dir: d.client_dir,
      },
    };
  }
  function newConnection() {
    editing = {
      hasPassword: false,
      input: { id: null, name: "", kind: "mysql", host: "127.0.0.1", port: 3306, user: "root", password: null, client_dir: null },
    };
  }
  function edit(c: ConnectionInfo) {
    editing = {
      hasPassword: c.has_password,
      input: { id: c.id, name: c.name, kind: c.kind, host: c.host, port: c.port, user: c.user, password: null, client_dir: c.client_dir },
    };
  }
  async function removeConnection(c: ConnectionInfo) {
    const ok = await confirm(t('Tirar "{name}" do Larafim? O servidor e os bancos dele não são alterados.', { name: c.name }), t("Remover conexão"), t("Remover"));
    if (ok) run(() => services.removeConnection(c.id));
  }

  // ---------- Adicionar serviço ----------
  async function onAdd(c: AddChoice) {
    adding = false;
    error = null;
    if (c.kind === "existing") return newConnection();
    if (c.kind === "mariadb") {
      selectedId = "mariadb";
      try {
        if (overview && c.port !== overview.port) await services.configure(overview.selected, c.port);
      } catch (e) {
        error = String(e);
        return;
      }
      install(c.major);
    } else if (c.kind === "postgres") {
      selectedId = "postgres";
      try {
        if (pgView && c.port !== pgView.port) await pg.configure(pgView.selected, c.port);
      } catch (e) {
        error = String(e);
        return;
      }
      pgInstall(c.major);
    } else if (c.kind === "mailpit") {
      selectedId = "mailpit";
      mailInstalling = true;
      run(async () => {
        try {
          await toolsApi.mailpitInstall();
        } finally {
          mailInstalling = false;
        }
      });
    } else if (c.kind === "redis") {
      selectedId = "redis";
      run(async () => {
        await redis.configure(c.backend, c.distro, c.port);
        if (c.backend === "native") await installRedis();
      });
    } else if (c.kind === "s3") {
      selectedId = "s3";
      s3progress = { received: 0, total: null };
      run(async () => {
        try {
          await s3.install();
        } finally {
          s3progress = null;
        }
      });
    }
  }
</script>

<header class="page-head">
  <div>
    <h1>{t("Serviços")}</h1>
    <p class="caption">{t("Ficam desligados e só sobem quando algum projeto precisa — ou quando você liga aqui.")}</p>
  </div>
  <button class="btn" disabled={!overview || !pgView} onclick={() => (adding = true)}><Glyph name="add" /> {t("Adicionar serviço")}</button>
</header>

{#if error}
  <div class="infobar err"><Glyph name="info" /><span>{error}</span></div>
{/if}

{#each detected.filter((d) => !d.known) as d (d.port)}
  <div class="infobar found">
    <Glyph name="database" />
    <span>
      {t("Encontrei um")} <strong>{d.kind === "mysql" ? "MySQL" : "MariaDB"}</strong> {t("rodando na porta {port}", { port: d.port })}
      <span class="caption">({d.process}{d.hint ? ` · ${d.hint}` : ""})</span>
    </span>
    <button class="btn accent" onclick={() => useDetected(d)}>{t("Usar este")}</button>
  </div>
{/each}

{#if !overview || !pgView}
  <div class="loading"><ProgressBar /></div>
{:else if !items.length}
  <div class="card empty">
    <Glyph name="database" size={32} />
    <strong>{t("Nenhum serviço ainda")}</strong>
    <span class="caption">{t("Adicione um banco de dados, a caixa de e-mails local ou o storage S3.")}</span>
    <button class="btn accent" onclick={() => (adding = true)}><Glyph name="add" /> {t("Adicionar serviço")}</button>
  </div>
{:else}
  <div class="split">
    <div class="list">
      {#each groups as g (g.title)}
        <div class="section-title group">{g.title}</div>
        <div class="card rows">
          {#each g.items as item (item.id)}
            <div
              class="item"
              class:selected={selected?.id === item.id}
              role="button"
              tabindex="0"
              onclick={() => (selectedId = item.id)}
              onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (selectedId = item.id)}
            >
              <span class="dot" class:on={item.running} class:ext={item.external}></span>
              <div class="text">
                <strong>{item.name}</strong>
                {#if item.download}
                  <span class="caption">{progressText(item.download)}</span>
                  <ProgressBar value={progressValue(item.download)} />
                {:else}
                  <span class="caption">{item.sub}</span>
                {/if}
              </div>
              {#if item.external}
                <span class="tag">{t("Externo")}</span>
              {:else if !item.download}
                <button
                  class="btn"
                  disabled={busy}
                  onclick={(e) => {
                    e.stopPropagation();
                    startStop(item, !item.running);
                  }}
                >
                  {item.running ? t("Desligar") : t("Ligar")}
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>

    {#if selected}
      {@const item = selected}
      <aside class="card detail">
        <h2>{item.name}</h2>
        <span class="caption status">
          {#if item.download}
            {t("Instalando…")}
          {:else if item.external}
            {t("Servidor de fora: o Larafim não liga nem desliga")}
          {:else}
            <span class="dot inline" class:on={item.running}></span>
            {item.running ? t("Rodando") : t("Desligado")}
          {/if}
        </span>

        <!-- Configuração -->
        <div class="section-title">{t("Configuração")}</div>
        {#if item.kind === "mariadb" && overview}
          <div class="kv">
            <span>{t("Versão em uso")}</span>
            <div class="select">
              <select
                value={overview.selected ?? ""}
                disabled={busy || !mariaInstalled.length}
                onchange={(e) => run(() => services.configure(e.currentTarget.value || null, overview!.port))}
              >
                <option value="">{t("Mais nova instalada")}</option>
                {#each mariaInstalled as r (r.major)}<option value={r.major}>MariaDB {r.major}</option>{/each}
              </select>
            </div>
          </div>
          <div class="kv">
            <span>{t("Porta")}</span>
            <input class="input port" type="number" min="1024" max="65535" bind:value={port} onblur={commitPort} onkeydown={(e) => e.key === "Enter" && commitPort()} />
          </div>
          <div class="kv"><span>{t("Usuário")}</span><span class="mono">root</span></div>
        {:else if item.kind === "postgres" && pgView}
          <div class="kv">
            <span>{t("Versão em uso")}</span>
            <div class="select">
              <select
                value={pgView.selected ?? ""}
                disabled={busy || !pgInstalled.length}
                onchange={(e) => run(() => pg.configure(e.currentTarget.value || null, pgView!.port))}
              >
                <option value="">{t("Mais nova instalada")}</option>
                {#each pgInstalled as r (r.major)}<option value={r.major}>PostgreSQL {r.major}</option>{/each}
              </select>
            </div>
          </div>
          <div class="kv">
            <span>{t("Porta")}</span>
            <input class="input port" type="number" min="1024" max="65535" bind:value={pgPort} onblur={commitPgPort} onkeydown={(e) => e.key === "Enter" && commitPgPort()} />
          </div>
          <div class="kv"><span>{t("Usuário")}</span><span class="mono">postgres</span></div>
        {:else if item.kind === "own"}
          {@const o = ownOf(item)}
          {#if o}
            <div class="kv"><span>{t("Projeto")}</span><span>{o.project}</span></div>
            <div class="kv"><span>{t("Versão")}</span><span>MariaDB {o.version}</span></div>
            <div class="kv"><span>{t("Porta")}</span><span class="mono">{o.port}</span></div>
          {/if}
        {:else if item.kind === "conn"}
          {@const c = connOf(item)}
          {#if c}
            <div class="kv"><span>{t("Tipo")}</span><span>{c.kind === "mysql" ? "MySQL" : "MariaDB"}</span></div>
            <div class="kv"><span>{t("Servidor")}</span><span class="mono">{c.host}:{c.port}</span></div>
            <div class="kv"><span>{t("Usuário")}</span><span class="mono">{c.user}</span></div>
            {#if c.used_by.length}
              <div class="kv"><span>{t("Usado por")}</span><span class="ellipsis" title={c.used_by.join(", ")}>{c.used_by.join(", ")}</span></div>
            {/if}
          {/if}
        {:else if item.kind === "mailpit" && mail}
          <div class="kv"><span>{t("Versão")}</span><span>{mail.version ?? "—"}</span></div>
          <div class="kv"><span>SMTP</span><span class="mono">{mail.smtp}</span></div>
          <div class="kv"><span>{t("Caixa de entrada")}</span><span class="mono ellipsis">{mail.ui_url.replace(/^https?:\/\//, "")}</span></div>
        {:else if item.kind === "s3" && s3st}
          <div class="kv"><span>{t("Versão")}</span><span>{s3st.version ?? "—"}</span></div>
          <div class="kv"><span>API</span><span class="mono">{s3st.endpoint.replace(/^https?:\/\//, "")}</span></div>
          <div class="kv"><span>{t("Chave de acesso")}</span><span class="mono">{s3st.access_key}</span></div>
        {:else if item.kind === "redis" && redisSt?.config}
          {@const rc = redisSt.config}
          <div class="kv">
            <span>{t("Rodar no")}</span>
            <div class="select">
              <select value={rc.backend} disabled={busy} onchange={(e) => commitRedis(e.currentTarget.value as "native" | "docker" | "wsl", rc.distro, rc.port)}>
                <option value="native">{t("Windows (nativo)")}</option>
                <option value="docker">Docker</option>
                <option value="wsl" disabled={!redisSt.distros.length}>WSL2</option>
              </select>
            </div>
          </div>
          {#if rc.backend === "wsl"}
            <div class="kv">
              <span>{t("Distribuição")}</span>
              <div class="select">
                <select value={rc.distro ?? ""} disabled={busy} onchange={(e) => commitRedis("wsl", e.currentTarget.value, rc.port)}>
                  {#each redisSt.distros as d (d)}<option value={d}>{d}</option>{/each}
                </select>
              </div>
            </div>
          {:else if rc.backend === "native"}
            <div class="kv"><span>{t("Versão")}</span><span>{redisSt.native_version ?? "—"}</span></div>
          {:else}
            <div class="kv"><span>{t("Imagem")}</span><span class="mono">{redisSt.image}</span></div>
          {/if}
          <div class="kv">
            <span>{t("Porta")}</span>
            <input
              class="input port"
              type="number"
              min="1024"
              max="65535"
              bind:value={redisPort}
              onblur={() => Number(redisPort) !== rc.port && commitRedis(rc.backend, rc.distro, Number(redisPort))}
              onkeydown={(e) => e.key === "Enter" && Number(redisPort) !== rc.port && commitRedis(rc.backend, rc.distro, Number(redisPort))}
            />
          </div>
        {/if}
        {#if info?.data_dir}
          <div class="kv">
            <span>{t("Pasta de dados")}</span>
            <span class="caption mono ellipsis" title={info.data_dir}>{shortPath(info.data_dir)}</span>
          </div>
        {/if}

        <!-- Ações -->
        <div class="section-title">{t("Ações")}</div>
        {#if item.kind === "mariadb" || item.kind === "postgres" || item.kind === "own" || item.kind === "conn"}
          <div class="kv">
            <span>Adminer</span>
            <button
              class="btn"
              disabled={busy}
              onclick={() =>
                run(() =>
                  item.kind === "own"
                    ? services.openAdminer(null, null, item.id.slice(4))
                    : item.kind === "conn"
                      ? services.openAdminer(null, item.id.slice(5))
                      : services.openAdminer(null, item.kind === "postgres" ? "postgres" : null),
                )}
            >
              {t("Abrir")}
            </button>
          </div>
        {/if}
        {#if item.kind === "mailpit"}
          <div class="kv"><span>{t("Caixa de entrada")}</span><button class="btn" disabled={busy} onclick={() => run(toolsApi.openMailpit)}>{t("Abrir")}</button></div>
        {/if}
        {#if item.kind === "s3"}
          <div class="kv"><span>{t("Console")}</span><button class="btn" disabled={busy} onclick={() => run(s3.open)}>{t("Abrir")}</button></div>
        {/if}
        {#if item.kind === "redis"}
          {#if redisSt?.config?.backend === "native"}
            <div class="kv">
              <span>{redisSt.native_installed ? t("Atualizar") : t("Instalar")}</span>
              <button class="btn" disabled={busy} onclick={() => run(installRedis)}>{redisSt.native_installed ? t("Atualizar") : t("Instalar")}</button>
            </div>
          {/if}
          <div class="kv"><span>Redis</span><button class="btn" disabled={busy} onclick={removeRedis}>{t("Remover")}</button></div>
        {/if}
        {#if info?.data_dir}
          <div class="kv"><span>{t("Pasta de dados")}</span><button class="btn" disabled={busy} onclick={() => run(() => api.openFolder(info!.data_dir!))}>{t("Abrir")}</button></div>
        {/if}
        {#if item.kind === "conn"}
          {@const c = connOf(item)}
          {#if c}
            <div class="kv">
              <span>{t("Conexão")}</span>
              <span class="btns">
                <button class="btn" onclick={() => edit(c)}>{t("Editar")}</button>
                <button class="btn" onclick={() => removeConnection(c)}>{t("Remover")}</button>
              </span>
            </div>
          {/if}
        {/if}

        {#if (item.kind === "mariadb" && overview?.status.running && overview.databases.length) || (item.kind === "postgres" && pgView?.status.running && pgView.databases.length)}
          {@const dbs = item.kind === "mariadb" ? overview!.databases : pgView!.databases}
          <div class="section-title">{t("Bancos")}</div>
          <div class="chips">
            {#each dbs as db (db)}
              <button class="chip" title={t("Abrir no Adminer")} onclick={() => run(() => services.openAdminer(db, item.kind === "postgres" ? "postgres" : null))}>
                <Glyph name="database" size={12} />
                {db}
              </button>
            {/each}
          </div>
        {/if}

        <!-- Versões -->
        {#if (item.kind === "mariadb" && overview) || (item.kind === "postgres" && pgView)}
          {@const isPg = item.kind === "postgres"}
          {@const rows = isPg ? pgView!.rows : overview!.rows}
          <div class="section-title">{t("Versões")}</div>
          {#if (isPg ? pgView!.catalog_error : overview!.catalog_error)}
            <p class="caption">{t("Não foi possível ver as versões: {error}", { error: (isPg ? pgView!.catalog_error : overview!.catalog_error) ?? "" })}</p>
          {/if}
          <div class="versions">
            {#each rows as row (row.major)}
              {@const p = progress[(isPg ? "pg" : "") + row.major]}
              {@const eol = "eol" in row ? (row.eol as string | null) : null}
              <div class="ver">
                <div class="text">
                  <span>{isPg ? "PostgreSQL" : "MariaDB"} {row.major}</span>
                  <span class="caption">
                    {row.installed ?? row.latest ?? "—"}{eol ? " · LTS " + t("até {year}", { year: eol.slice(0, 4) }) : ""}
                  </span>
                </div>
                {#if p}
                  <div class="progress">
                    <span class="caption">{progressText(p)}</span>
                    <ProgressBar value={progressValue(p)} />
                  </div>
                {:else if row.installed}
                  <span class="ok"><Glyph name="check" size={12} /></span>
                  {#if row.update_available}
                    <button class="btn accent" onclick={() => (isPg ? pgInstall(row.major) : install(row.major))}>{t("Atualizar")}</button>
                  {/if}
                  <button class="btn subtle icon" title={t("Remover")} onclick={() => (isPg ? pgRemove(row.major) : remove(row.major))}><Glyph name="delete" /></button>
                {:else if row.latest}
                  <button class="btn" onclick={() => (isPg ? pgInstall(row.major) : install(row.major))}>{t("Instalar")}</button>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Variáveis de ambiente -->
        {#if info?.env.length}
          <div class="section-title row-title">
            <span>{t("Variáveis de ambiente")}</span>
            <button class="btn subtle icon" title={copied ? t("Copiado!") : t("Copiar")} onclick={copyEnv}>
              <Glyph name={copied ? "check" : "copy"} />
            </button>
          </div>
          <pre class="env">{envText}</pre>
          {#if item.kind === "conn"}
            <p class="caption">{t("A senha não aparece aqui: preencha DB_PASSWORD se o servidor tiver uma.")}</p>
          {/if}
        {/if}

        <!-- Log -->
        {#if info && !item.external}
          <div class="section-title row-title">
            <span>{t("Log")}</span>
            <span class="caption mono">{info.log_file ? baseName(info.log_file) : t("saída do processo")}</span>
          </div>
          {#if info.log.length}
            <pre class="log" bind:this={logBox}>{info.log.join("\n")}</pre>
          {:else}
            <p class="caption">{t("Nada no log ainda.")}</p>
          {/if}
        {/if}
      </aside>
    {/if}
  </div>
{/if}

{#if adding && overview && pgView}
  <AddServiceDialog
    maria={overview.rows}
    pg={pgView.rows}
    mariaPort={overview.port}
    pgPort={pgView.port}
    mailInstalled={!!mail?.installed}
    s3Installed={!!s3st?.installed}
    redisStatus={redisSt}
    onclose={() => (adding = false)}
    onadd={onAdd}
  />
{/if}

{#if editing}
  <ConnectionDialog
    initial={editing.input}
    hasPassword={editing.hasPassword}
    onclose={() => (editing = null)}
    onsaved={() => {
      const wasNew = !editing?.input.id;
      editing = null;
      loadConnections().then(() => {
        if (wasNew && connections.length) selectedId = "conn:" + connections[connections.length - 1].id;
      });
    }}
  />
{/if}

<style>
  .page-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 12px;
  }
  .page-head .btn {
    margin-top: 4px;
  }
  h1 {
    margin: 0 0 4px;
    font: 600 28px/36px var(--font-display);
  }
  .page-head p {
    margin: 0;
  }
  .infobar.err {
    margin-bottom: 8px;
    background: color-mix(in srgb, var(--err) 12%, var(--card));
    user-select: text;
  }
  .infobar.found {
    align-items: center;
    margin-bottom: 8px;
  }
  .infobar.found span {
    flex: 1;
  }
  .loading {
    width: 200px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 40px 24px;
    text-align: center;
    color: var(--text-2);
  }
  .empty strong {
    color: var(--text);
    font-size: 16px;
  }
  .empty .btn {
    margin-top: 8px;
  }

  /* Lista + detalhes lado a lado; empilha quando a janela é estreita. */
  .split {
    container-type: inline-size;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 330px;
    gap: 16px;
    align-items: start;
  }
  @media (max-width: 900px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .list {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .group {
    margin: 4px 0 8px;
  }
  .rows + .group {
    margin-top: 20px;
  }
  .rows {
    overflow: hidden;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 60px;
    padding: 8px 12px 8px 16px;
    cursor: default;
  }
  .item + .item {
    border-top: 1px solid var(--stroke);
  }
  .item:hover {
    background: var(--subtle-hover);
  }
  .item.selected {
    background: var(--selected);
  }
  .item.selected::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    width: 3px;
    height: 20px;
    border-radius: 2px;
    background: var(--accent);
    transform: translateY(-50%);
  }
  .item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .item .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .item .text strong {
    font-weight: 600;
  }
  .item .caption {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot.ext {
    background: transparent;
    border: 1.5px solid var(--text-3);
  }
  .tag {
    font-size: 12px;
    padding: 1px 8px;
    border-radius: 10px;
    border: 1px solid var(--stroke);
    color: var(--text-2);
  }

  /* O painel rola sozinho; a lista continua à vista. */
  .detail {
    position: sticky;
    top: 0;
    max-height: calc(100vh - 72px);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px 16px 18px;
    min-width: 0;
  }
  /* Com altura máxima, os blocos não podem encolher (senão o log some). */
  .detail > :global(*) {
    flex-shrink: 0;
  }
  .detail h2 {
    margin: 0;
    font: 600 20px/28px var(--font-display);
    overflow-wrap: anywhere;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }
  .detail .section-title {
    margin: 14px 0 2px;
  }
  .row-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .row-title .caption {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
  }
  .kv {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
  }
  .kv > span:first-child {
    flex: none;
    color: var(--text-2);
  }
  .kv > :last-child {
    min-width: 0;
    text-align: right;
  }
  .kv .select {
    width: 170px;
  }
  .kv .port {
    width: 96px;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .btns {
    display: flex;
    gap: 4px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: 13px;
    border: 1px solid var(--stroke);
    background: var(--layer);
    font-family: var(--mono);
    font-size: 12px;
  }
  .chip:hover {
    background: var(--subtle-hover);
  }
  .versions {
    display: flex;
    flex-direction: column;
  }
  .ver {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
  }
  .ver + .ver {
    border-top: 1px solid var(--stroke);
  }
  .ver .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .ok {
    display: inline-flex;
    color: var(--ok);
    margin-right: 4px;
  }
  .progress {
    width: 150px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  pre {
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--r-ctrl);
    border: 1px solid var(--stroke);
    background: var(--layer);
    font: 12px/1.6 var(--mono);
    overflow: auto;
    user-select: text;
  }
  .log {
    max-height: 220px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 11px;
    line-height: 1.5;
  }
  .detail p {
    margin: 0;
  }
</style>
