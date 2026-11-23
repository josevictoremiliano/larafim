<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { onEvent } from "../lib/api";
  import { pickFolder } from "../lib/folders";
  import Glyph from "../lib/Glyph.svelte";
  import type { DevLog, DevStatus } from "../lib/node";
  import { php } from "../lib/php";
  import { app } from "../lib/state.svelte";
  const LARAVEL_PHP = ["8.3", "8.4", "8.5"];
  import { toolsApi, type Created, type Kit } from "../lib/tools";

  // Assistente: criar um projeto Laravel novo já cadastrado no Larafim.
  let { onclose, ondone }: { onclose: () => void; ondone: (path: string) => void } = $props();

  let name = $state("");
  // svelte-ignore state_referenced_locally
  let root = $state(app.config?.roots[0] ?? "");
  let phpMinor = $state("");
  let kit = $state<Kit>("none");
  let database = $state<"sqlite" | "mariadb">("sqlite");
  let minors = $state<string[]>([]);

  let key = $state<string | null>(null);
  let output = $state<string[]>([]);
  let result = $state<Created | null>(null);
  let error = $state<string | null>(null);
  let box = $state<HTMLPreElement>();

  $effect(() => {
    php.overview().then((o) => {
      // Só os PHPs que o Laravel 13 suporta; o mais novo deles vem selecionado.
      minors = o.rows.filter((r) => r.installed && LARAVEL_PHP.includes(r.minor)).map((r) => r.minor);
      minors.sort((a, b) => Number(b) - Number(a));
      phpMinor = minors[0] ?? "";
    });
    const stops = [
      onEvent<DevLog>("node:log", (l) => {
        if (l.key === key) output = [...output.slice(-499), l.line];
      }),
      onEvent<DevStatus>("node:status", (s) => {
        if (s.key === key && s.state === "exited" && s.exit_code !== 0) {
          error = t("O comando terminou com erro (código {code}). Veja a saída.", { code: s.exit_code ?? "?" });
        }
      }),
      onEvent<Created>("laravel:created", (c) => {
        if (c.key === key) {
          result = c;
          if (!c.ok) error = c.error;
        }
      }),
    ];
    return () => stops.forEach((s) => s());
  });

  $effect(() => {
    void output.length;
    if (box) box.scrollTop = box.scrollHeight;
  });

  const slug = $derived(name.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, ""));

  async function create() {
    error = null;
    output = [];
    try {
      key = await toolsApi.laravelCreate({ name: slug, root, php: phpMinor, kit, database });
    } catch (e) {
      error = String(e);
    }
  }

  async function chooseRoot() {
    const dir = await pickFolder(t("Onde criar o projeto"));
    if (dir) root = dir;
  }

  const running = $derived(key !== null && result === null && !error);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !running && onclose()} />

<div class="smoke" role="presentation" onclick={(e) => e.target === e.currentTarget && !running && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="nl-title">
    <div class="content">
      <h2 id="nl-title">{t("Novo projeto Laravel")}</h2>
      {#if key === null}
        <label class="row"><span>{t("Nome")}</span><input class="input" bind:value={name} placeholder="minha-loja" /></label>
        {#if slug && app.config}
          <p class="caption preview">{t("Endereço:")} <span class="mono">http://{slug}.{app.config.tld}</span></p>
        {/if}
        <div class="row">
          <span>{t("Pasta")}</span>
          <span class="pair">
            <span class="caption dir" title={root}>{root || t("Escolha…")}</span>
            <button class="btn subtle icon" title={t("Escolher pasta")} onclick={chooseRoot}><Glyph name="folderOpen" /></button>
          </span>
        </div>
        <div class="row">
          <span>{t("Versão do PHP")}</span>
          <div class="select">
            <select bind:value={phpMinor}>
              {#each minors as m (m)}<option value={m}>PHP {m}</option>{/each}
            </select>
          </div>
        </div>
        {#if !minors.length}<p class="caption warn">{t("O Laravel 13 precisa do PHP 8.3, 8.4 ou 8.5: instale em Configurações › PHP.")}</p>{/if}
        <div class="row">
          <span>{t("Starter kit")}</span>
          <div class="select">
            <select bind:value={kit}>
              <option value="none">{t("Nenhum (só Laravel)")}</option>
              <option value="react">React</option>
              <option value="vue">Vue</option>
              <option value="livewire">Livewire</option>
            </select>
          </div>
        </div>
        {#if kit !== "none"}<p class="caption preview">{t("Inclui npm install e build (precisa do Node.js do Larafim).")}</p>{/if}
        <div class="row">
          <span>{t("Banco de dados")}</span>
          <div class="select">
            <select bind:value={database}>
              <option value="sqlite">{t("SQLite (padrão do Laravel)")}</option>
              <option value="mariadb">{t("MariaDB compartilhado")}</option>
            </select>
          </div>
        </div>
      {:else}
        <p class="caption">
          {#if result?.ok}
            {t("Pronto! O projeto foi criado e já está na lista.")}
          {:else if error}
            {t("Não deu certo.")}
          {:else}
            {t("Criando")} <span class="mono">{slug}</span>… {t("isso pode levar alguns minutos.")}
          {/if}
        </p>
        <pre bind:this={box}>{output.join("\n") || "Iniciando…"}</pre>
      {/if}
      {#if error}<p class="caption err">{error}</p>{/if}
    </div>
    <div class="footer">
      {#if key === null}
        <button class="btn accent" disabled={!slug || !root || !phpMinor} onclick={create}>{t("Criar")}</button>
        <button class="btn" onclick={onclose}>{t("Cancelar")}</button>
      {:else if result?.ok}
        <button class="btn accent" onclick={() => ondone(result!.path)}>{t("Abrir projeto")}</button>
        <button class="btn" onclick={onclose}>{t("Fechar")}</button>
      {:else}
        <button class="btn" disabled={running} onclick={onclose}>{t("Fechar")}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .smoke {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.3);
  }
  .dialog {
    width: 560px;
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
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 32px;
  }
  .row > .input,
  .row > .select {
    width: 280px;
  }
  .pair {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 280px;
  }
  .dir {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview {
    margin: -4px 0 0;
    text-align: right;
  }
  .warn {
    color: var(--warn);
  }
  .err {
    color: var(--err);
    user-select: text;
  }
  p {
    margin: 0;
  }
  pre {
    margin: 0;
    height: 280px;
    overflow: auto;
    padding: 10px 12px;
    border-radius: var(--r-ctrl);
    background: var(--win-bg);
    border: 1px solid var(--stroke);
    font: 11px/16px var(--mono);
    white-space: pre-wrap;
    user-select: text;
  }
  .footer {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 20px 24px;
    background: var(--win-bg);
    border-top: 1px solid var(--stroke);
  }
</style>
