<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { onEvent } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import type { DevLog, DevStatus } from "../lib/node";
  import type { Project } from "../lib/projects";
  import Toggle from "../lib/Toggle.svelte";
  import InstallProgress from "../lib/InstallProgress.svelte";
  import { confirm } from "../lib/php";
  import { toolsApi, workerKey, type Worker } from "../lib/tools";

  // Workers do Laravel: ligam junto com o projeto e reiniciam se caírem.
  let { project, showTitle = true }: { project: Project; showTitle?: boolean } = $props();

  const labels = $derived<Record<Worker, { title: string; hint: string }>>({
    queue: {
      title: t("Fila"),
      hint: project.queues.length ? `queue:work --queue=${project.queues.join(",")}` : "queue:work",
    },
    schedule: { title: t("Agendador"), hint: "schedule:work" },
    reverb: {
      title: "Reverb",
      hint: project.reverb_port ? `ws://127.0.0.1:${project.reverb_port}` : "reverb:start (WebSockets)",
    },
  });

  let reverb = $state(false);
  let status = $state<Record<string, DevStatus | null>>({});
  let logs = $state<Record<string, string[]>>({});
  let open = $state<Worker | null>(null);
  let error = $state<string | null>(null);
  let busy = $state<Worker | null>(null);
  let installingReverb = $state(false);

  // Filas do worker: o Laravel não tem a lista, então o Larafim descobre e a pessoa ajusta.
  let editingQueues = $state(false);
  let queuesText = $state("");
  let queuesBusy = $state(false);

  function editQueues() {
    queuesText = project.queues.join(", ");
    editingQueues = true;
  }
  async function saveQueues() {
    error = null;
    queuesBusy = true;
    try {
      await toolsApi.queuesSet(project.path, queuesText);
      editingQueues = false;
    } catch (e) {
      error = String(e);
    } finally {
      queuesBusy = false;
    }
  }
  async function rediscover() {
    error = null;
    queuesBusy = true;
    try {
      queuesText = (await toolsApi.queuesDiscover(project.path)).join(", ");
    } catch (e) {
      error = String(e);
    } finally {
      queuesBusy = false;
    }
  }

  const workers: Worker[] = ["queue", "schedule", "reverb"];

  // Reconfere ao voltar para a janela: a pessoa pode ter instalado o Reverb no terminal.
  $effect(() => {
    const path = project.path;
    const check = () => toolsApi.hasReverb(path).then((r) => (reverb = r));
    check();
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  });

  $effect(() => {
    const path = project.path;
    for (const w of workers) {
      toolsApi.processInfo(workerKey(path, w)).then((i) => {
        status[w] = i.status;
        logs[w] = i.logs;
      });
    }
    const keys = new Map(workers.map((w) => [workerKey(path, w), w]));
    const stops = [
      onEvent<DevStatus>("node:status", (s) => {
        const w = keys.get(s.key);
        if (w) status[w] = s;
      }),
      onEvent<DevLog>("node:log", (l) => {
        const w = keys.get(l.key);
        if (w) logs[w] = [...(logs[w] ?? []).slice(-299), l.line];
      }),
    ];
    return () => stops.forEach((s) => s());
  });

  async function toggle(w: Worker, enabled: boolean) {
    error = null;
    busy = w;
    try {
      if (w === "reverb" && enabled && !reverb && !(await installReverb())) return;
      if (w === "reverb" && enabled && !(await confirmReverb())) return;
      await toolsApi.workerSet(project.path, w, enabled);
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  // Mostra a porta escolhida e o que muda no .env antes de ligar.
  async function confirmReverb(): Promise<boolean> {
    const pv = await toolsApi.reverbPreview(project.path);
    if (!pv.installed) {
      error = t("O Reverb não está instalado neste projeto.");
      return false;
    }
    const lines = pv.diff.map((d) => `${d.key}: ${d.before ?? "—"} → ${d.after ?? t("(comentada)")}`).join("\n");
    const env = pv.no_env
      ? "\n\n" + t("O projeto não tem .env: só a porta do Reverb será definida.")
      : lines
        ? "\n\n" +
          (pv.from_example ? t("O .env será criado a partir do .env.example.") + "\n" : "") +
          t("Mudanças no .env (com cópia em .env.larafim.bak):") +
          "\n" +
          lines
        : "";
    return confirm(
      t("Ligar o Reverb na porta {port}? Ela fica reservada para este projeto.", { port: pv.port }) + env,
      "Reverb",
      t("Ligar"),
    );
  }

  // Projeto sem Reverb: pergunta se deve instalar (altera composer.json, rotas e .env).
  async function installReverb(): Promise<boolean> {
    const ok = await confirm(
      t("O Reverb não está instalado neste projeto. Instalar agora? (php artisan install:broadcasting --reverb: altera composer.json, composer.lock, rotas e .env)"),
      "Reverb",
      t("Instalar"),
    );
    if (!ok) return false;
    installingReverb = true;
    try {
      await toolsApi.installReverb(project.path);
      reverb = await toolsApi.hasReverb(project.path);
      return reverb;
    } finally {
      installingReverb = false;
    }
  }

  const stateText = (w: Worker) => {
    const s = status[w];
    if (w === "reverb" && installingReverb) return t("instalando o Reverb…");
    if (w === "reverb" && !reverb) return t("não instalado neste projeto");
    if (!project.workers.includes(w)) return t("desligado");
    if (s && s.state !== "exited") return t("rodando");
    if (!project.online) return t("liga com o projeto");
    if (!s) return t("parado");
    return s.exit_code === null ? t("parado") : t("parou (código {code})", { code: s.exit_code });
  };
</script>

{#if showTitle}<div class="section-title">{t("Workers")}</div>{/if}
{#each workers as w (w)}
  {@const running = status[w]?.state === "running" || status[w]?.state === "starting"}
  <div class="worker">
    <span class="dot" class:on={running && project.workers.includes(w)}></span>
    <div class="text">
      <span>{labels[w].title}</span>
      <span class="caption">{labels[w].hint} · {stateText(w)}</span>
    </div>
    <button
      class="btn subtle icon"
      title={t("Ver saída")}
      disabled={!(logs[w]?.length)}
      onclick={() => (open = open === w ? null : w)}
    >
      <Glyph name={open === w ? "chevronDown" : "terminal"} />
    </button>
    {#if w === "queue"}
      <button class="btn subtle icon" title={t("Filas processadas")} onclick={() => (editingQueues ? (editingQueues = false) : editQueues())}>
        <Glyph name="settings" />
      </button>
    {/if}
    <Toggle
      label={t("Worker {name}", { name: labels[w].title })}
      checked={project.workers.includes(w)}
      disabled={busy === w}
      onchange={(v) => toggle(w, v)}
    />
  </div>
  {#if w === "reverb" && installingReverb}
    <InstallProgress path={project.path} label={t("instalando o Reverb…")} />
  {/if}
  {#if w === "queue" && editingQueues}
    <div class="queues">
      <label class="caption" for="queues-input">{t("Filas processadas, em ordem de prioridade (separe com vírgula):")}</label>
      <input
        id="queues-input"
        class="input mono"
        bind:value={queuesText}
        spellcheck="false"
        placeholder="default"
        onkeydown={(e) => e.key === "Enter" && saveQueues()}
      />
      <div class="queue-actions">
        <button class="btn" disabled={queuesBusy} onclick={rediscover}>{t("Detectar de novo")}</button>
        <span class="grow"></span>
        <button class="btn" disabled={queuesBusy} onclick={() => (editingQueues = false)}>{t("Cancelar")}</button>
        <button class="btn accent" disabled={queuesBusy} onclick={saveQueues}>{t("Salvar")}</button>
      </div>
    </div>
  {/if}
  {#if open === w}
    <pre>{(logs[w] ?? []).join("\n")}</pre>
  {/if}
{/each}
{#if error}<p class="caption err">{error}</p>{/if}

<style>
  .section-title {
    margin-top: 4px;
  }
  .worker {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .text .caption {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  pre {
    margin: 0;
    max-height: 180px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: var(--r-ctrl);
    background: var(--layer);
    border: 1px solid var(--stroke);
    font: 11px/16px var(--mono);
    white-space: pre-wrap;
    user-select: text;
  }
  .err {
    margin: 0;
    color: var(--err);
  }
  .queues {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 0 4px 16px;
  }
  .queue-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
</style>
