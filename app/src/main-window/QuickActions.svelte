<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import Glyph, { type GlyphName } from "../lib/Glyph.svelte";
  import type { Project } from "../lib/projects";
  import { toolsApi, type RunResult } from "../lib/tools";

  // Ações rápidas: um clique roda o comando no projeto (com o PHP dele), sem
  // abrir terminal. A saída da última ação fica logo abaixo.
  let { project }: { project: Project } = $props();

  type Tool = "artisan" | "composer";
  type Action = { tool: Tool; command: string; label: string; icon: GlyphName };

  const artisan = $derived<Action[]>([
    { tool: "artisan", command: "migrate", label: t("Migrar"), icon: "database" },
    { tool: "artisan", command: "migrate --seed", label: t("Migrar com seed"), icon: "database" },
    { tool: "artisan", command: "cache:clear", label: t("Limpar cache"), icon: "brush" },
    { tool: "artisan", command: "optimize:clear", label: t("Limpar otimizações"), icon: "layers" },
  ]);
  const composer = $derived<Action[]>([
    { tool: "composer", command: "install", label: "composer install", icon: "package" },
    { tool: "composer", command: "dump-autoload", label: "dump-autoload", icon: "refresh" },
  ]);

  let running = $state<string | null>(null);
  let last = $state<{ id: string; title: string; result: RunResult | null; error: string | null } | null>(null);
  let showOutput = $state(false);
  let custom = $state("");

  // Outro projeto: limpa o resultado anterior.
  $effect(() => {
    void project.path;
    last = null;
    custom = "";
    showOutput = false;
  });

  const idOf = (tool: Tool, command: string) => `${tool}:${command}`;

  async function run(tool: Tool, command: string) {
    const cmd = command.trim();
    if (!cmd || running) return;
    const id = idOf(tool, cmd);
    const path = project.path;
    running = id;
    const title = tool === "artisan" ? `php artisan ${cmd}` : `composer ${cmd}`;
    try {
      const result = await toolsApi.projectRun(path, tool, cmd);
      if (project.path !== path) return;
      last = { id, title, result, error: null };
      // Falhou: já mostra a saída para ver o motivo.
      showOutput = !result.ok;
    } catch (e) {
      if (project.path !== path) return;
      last = { id, title, result: null, error: String(e) };
      showOutput = false;
    } finally {
      running = null;
    }
  }

  function stateOf(a: Action): "running" | "ok" | "err" | null {
    const id = idOf(a.tool, a.command);
    if (running === id) return "running";
    if (last?.id !== id) return null;
    return last.result?.ok ? "ok" : "err";
  }

  const seconds = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1).replace(".", ",")} s`);
</script>

{#snippet button(a: Action)}
  {@const s = stateOf(a)}
  <button class="action" class:ok={s === "ok"} class:err={s === "err"} disabled={!!running} title={a.tool === "artisan" ? `php artisan ${a.command}` : `composer ${a.command}`} onclick={() => run(a.tool, a.command)}>
    <Glyph name={a.icon} />
    <span class="label">{a.label}</span>
    {#if s === "running"}
      <span class="spinner" aria-label={t("Rodando…")}></span>
    {:else if s === "ok"}
      <Glyph name="check" size={14} />
    {/if}
  </button>
{/snippet}

<div class="actions">
  {#if project.kind === "laravel"}
    <div class="grid">
      {#each artisan as a (a.command)}{@render button(a)}{/each}
    </div>
    <form class="custom" onsubmit={(e) => (e.preventDefault(), run("artisan", custom))}>
      <span class="prefix mono">php artisan</span>
      <input class="input mono" bind:value={custom} placeholder={t("comando")} spellcheck="false" autocomplete="off" aria-label={t("Comando do artisan")} />
      <button class="btn" type="submit" disabled={!!running || !custom.trim()} title={t("Executar")}>
        {#if running === idOf("artisan", custom.trim())}<span class="spinner"></span>{:else}<Glyph name="play" />{/if}
      </button>
    </form>
  {/if}
  <div class="grid">
    {#each composer as a (a.command)}{@render button(a)}{/each}
  </div>

  {#if last}
    <div class="result" class:err={!!last.error || !last.result?.ok}>
      <div class="result-head">
        <Glyph name={last.result?.ok ? "check" : "info"} size={14} />
        <span class="mono title" title={last.title}>{last.title}</span>
        {#if last.result}
          <span class="caption">{last.result.ok ? seconds(last.result.millis) : t("falhou")}</span>
          {#if last.result.output}
            <button class="btn subtle small" onclick={() => (showOutput = !showOutput)}>
              {showOutput ? t("Ocultar") : t("Saída")}
            </button>
          {/if}
        {/if}
        <button class="btn subtle icon small" title={t("Fechar")} onclick={() => (last = null)}><Glyph name="close" size={12} /></button>
      </div>
      {#if last.error}<p class="caption">{last.error}</p>{/if}
      {#if showOutput && last.result?.output}<pre>{last.result.output}</pre>{/if}
    </div>
  {/if}
</div>

<style>
  .actions {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .action {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--stroke);
    border-radius: var(--r-ctrl);
    background: var(--card);
    color: inherit;
    font: inherit;
    font-size: 12px;
    text-align: left;
  }
  .action:hover:not(:disabled) {
    background: var(--layer);
  }
  .action:active:not(:disabled) {
    opacity: 0.8;
  }
  .action:disabled {
    opacity: 0.6;
  }
  .action:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .action > :global(.glyph:first-child) {
    color: var(--text-2);
    flex: none;
  }
  .action.ok > :global(.glyph:last-child) {
    color: var(--ok);
  }
  .action.err {
    border-color: color-mix(in srgb, var(--err) 50%, var(--stroke));
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .custom {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .prefix {
    flex: none;
    font-size: 12px;
    color: var(--text-2);
  }
  .custom input {
    flex: 1;
    min-width: 0;
    font-size: 12px;
  }
  .spinner {
    flex: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--stroke);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 6px 6px 10px;
    border-radius: var(--r-ctrl);
    border: 1px solid var(--stroke);
    background: color-mix(in srgb, var(--ok) 8%, var(--card));
  }
  .result.err {
    background: color-mix(in srgb, var(--err) 10%, var(--card));
  }
  .result-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .result-head > :global(.glyph:first-child) {
    color: var(--ok);
  }
  .result.err .result-head > :global(.glyph:first-child) {
    color: var(--err);
  }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .small {
    height: 24px;
    min-width: 24px;
    padding: 0 6px;
    font-size: 12px;
  }
  .result p {
    margin: 0;
    user-select: text;
  }
  pre {
    margin: 0;
    max-height: 180px;
    overflow: auto;
    padding: 8px;
    border-radius: var(--r-ctrl);
    background: var(--win-bg);
    border: 1px solid var(--stroke);
    font: 11px/16px var(--mono);
    white-space: pre-wrap;
    user-select: text;
  }
</style>
