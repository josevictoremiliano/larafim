<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { projectUrl } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import { projectsApi, type Project, type ProjectsOverview } from "../lib/projects";
  import { server } from "../lib/server";
  import { app } from "../lib/state.svelte";

  // Domínios extras do projeto: o mesmo site responde em mais de um nome
  // (white-label/multi-tenant por host, ex.: finder.noveck.local).
  let {
    project,
    onchange,
    showTitle = true,
  }: {
    project: Project;
    onchange: (data: ProjectsOverview) => void;
    /** Dentro de uma seção recolhível o título já aparece no cabeçalho dela. */
    showTitle?: boolean;
  } = $props();

  const tld = $derived(app.config?.tld ?? "local");

  let adding = $state(false);
  let draft = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let copied = $state<string | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  // Troca de projeto: fecha o editor e limpa mensagens.
  $effect(() => {
    void project.path;
    adding = false;
    draft = "";
    error = null;
  });

  /** Mesmo tratamento do backend: minúsculas, sem o TLD, só [a-z0-9.-]. */
  function normalize(text: string): string {
    let v = text.trim().toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");
    v = v.replace(/^https?:\/\//, "").split(/[/:]/)[0];
    const suffix = `.${tld}`;
    if (v.endsWith(suffix)) v = v.slice(0, -suffix.length);
    return v
      .replace(/[^a-z0-9.-]+/g, "-")
      .replace(/\.{2,}/g, ".")
      .replace(/^[.-]+|[.-]+$/g, "");
  }

  const preview = $derived(normalize(draft));
  const draftProblem = $derived.by(() => {
    if (!preview) return null;
    if (["db", "mail", "s3"].includes(preview.split(".")[0]))
      return t('domínio extra "{a}" reservado para um serviço do Larafim', { a: preview });
    if (preview === project.domain) return t("Esse já é o domínio principal.");
    if (project.aliases.includes(preview)) return t("Esse domínio já está na lista.");
    if (preview.split(".").some((l) => !l || l.length > 63 || l.startsWith("-") || l.endsWith("-")))
      return t("Use letras, números, hífen e ponto.");
    return null;
  });

  async function commit(aliases: string[]) {
    busy = true;
    error = null;
    try {
      onchange(await projectsApi.setAliases(project.path, aliases));
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  function openEditor() {
    adding = true;
    draft = "";
    error = null;
    queueMicrotask(() => input?.focus());
  }

  async function add() {
    if (!preview || draftProblem || busy) return;
    if (await commit([...project.aliases, preview])) {
      draft = "";
      adding = false;
    }
  }

  async function remove(alias: string) {
    await commit(project.aliases.filter((a) => a !== alias));
  }

  function urlOf(alias: string) {
    return app.config ? projectUrl(app.config, alias) : `https://${alias}.${tld}`;
  }

  async function copy(alias: string) {
    await navigator.clipboard.writeText(urlOf(alias));
    copied = alias;
    setTimeout(() => (copied = copied === alias ? null : copied), 1500);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") add();
    if (e.key === "Escape") adding = false;
  }
</script>

<div class="head">
  {#if showTitle}<div class="section-title">{t("Domínios extras")}</div>{:else}<span></span>{/if}
  {#if !adding}
    <button class="btn subtle icon" title={t("Adicionar domínio")} disabled={busy} onclick={openEditor}>
      <Glyph name="add" />
    </button>
  {/if}
</div>

{#if project.aliases.length === 0 && !adding}
  <button class="empty" onclick={openEditor}>
    <Glyph name="globe" size={16} />
    <span class="empty-text">
      <span>{t("Responder em outros endereços")}</span>
      <span class="caption">
        {t("Útil para white-label: o mesmo projeto abre em {example}.", { example: `${project.domain}.cliente.${tld}` })}
      </span>
    </span>
  </button>
{/if}

<ul class="list">
  <li class="row main" title={t("Domínio principal")}>
    <Glyph name="home" size={14} />
    <span class="host mono">{project.domain}<span class="tld">.{tld}</span></span>
    <span class="badge caption">{t("principal")}</span>
  </li>
  {#each project.aliases as alias (alias)}
    <li class="row">
      <Glyph name="link" size={14} />
      <span class="host mono" title={urlOf(alias)}>{alias}<span class="tld">.{tld}</span></span>
      <span class="actions">
        <button
          class="btn subtle icon"
          title={project.online ? t("Abrir no navegador") : t("Ligue o projeto para abrir")}
          disabled={!project.online}
          onclick={() => server.openUrl(urlOf(alias))}
        >
          <Glyph name="openExternal" />
        </button>
        <button class="btn subtle icon" title={copied === alias ? t("Copiado!") : t("Copiar endereço")} onclick={() => copy(alias)}>
          <Glyph name={copied === alias ? "check" : "copy"} />
        </button>
        <button class="btn subtle icon danger" title={t("Remover domínio")} disabled={busy} onclick={() => remove(alias)}>
          <Glyph name="delete" />
        </button>
      </span>
    </li>
  {/each}
</ul>

{#if adding}
  <div class="editor">
    <span class="domain-input">
      <input
        bind:this={input}
        class="input mono"
        bind:value={draft}
        onkeydown={onKey}
        placeholder={`${project.domain}.cliente`}
        spellcheck="false"
        autocomplete="off"
        aria-label={t("Novo domínio extra")}
        aria-invalid={!!draftProblem}
      />
      <span class="caption">.{tld}</span>
    </span>
    {#if draftProblem}
      <p class="caption err">{draftProblem}</p>
    {:else if preview && preview !== draft.trim()}
      <p class="caption">{t("Será salvo como {host}", { host: `${preview}.${tld}` })}</p>
    {/if}
    <div class="editor-actions">
      <span class="caption grow">{t("O Windows pode pedir permissão para atualizar o arquivo hosts.")}</span>
      <button class="btn" disabled={busy} onclick={() => (adding = false)}>{t("Cancelar")}</button>
      <button class="btn accent" disabled={busy || !preview || !!draftProblem} onclick={add}>
        {busy ? t("Salvando…") : t("Adicionar")}
      </button>
    </div>
  </div>
{/if}

{#if error}<p class="caption err" role="alert">{error}</p>{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
  }
  .empty {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    border: 1px dashed var(--stroke);
    border-radius: var(--r-ctrl);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.12s;
  }
  .empty:hover {
    background: var(--layer);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--stroke));
  }
  .empty:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .empty-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--stroke);
    border-radius: var(--r-ctrl);
    overflow: hidden;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
    padding: 0 6px 0 10px;
  }
  .row + .row {
    border-top: 1px solid var(--stroke);
  }
  .row.main {
    background: var(--layer);
  }
  .row :global(.glyph) {
    flex: none;
    color: var(--text-2);
  }
  .host {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    user-select: text;
  }
  .tld {
    color: var(--text-2);
  }
  .badge {
    padding: 1px 6px;
    margin-right: 4px;
    border-radius: 999px;
    border: 1px solid var(--stroke);
  }
  .actions {
    display: flex;
    gap: 2px;
    opacity: 0.55;
    transition: opacity 0.12s;
  }
  .row:hover .actions,
  .actions:focus-within {
    opacity: 1;
  }
  .danger:not(:disabled):hover {
    color: var(--err);
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px;
    border-radius: var(--r-ctrl);
    background: var(--layer);
    border: 1px solid var(--stroke);
  }
  .domain-input {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .domain-input input {
    flex: 1;
    min-width: 0;
    font-size: 12px;
  }
  .domain-input input[aria-invalid="true"] {
    border-color: var(--err);
  }
  .editor-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  p {
    margin: 0;
  }
  .err {
    color: var(--err);
  }
</style>
