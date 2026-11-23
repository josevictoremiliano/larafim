<script lang="ts">
  import type { Snippet } from "svelte";
  import Glyph, { type GlyphName } from "../lib/Glyph.svelte";

  // Seção recolhível do painel do projeto (como os "expanders" do Windows 11).
  // Lembra aberta/fechada por seção, para todos os projetos.
  let {
    id,
    icon,
    title,
    summary = "",
    open: initial = false,
    children,
  }: { id: string; icon: GlyphName; title: string; summary?: string; open?: boolean; children: Snippet } = $props();

  const key = $derived(`larafim:section:${id}`);

  function load(): boolean {
    try {
      const v = localStorage.getItem(key);
      return v === null ? initial : v === "1";
    } catch {
      return initial;
    }
  }

  // svelte-ignore state_referenced_locally
  let open = $state(load());

  function toggle() {
    open = !open;
    try {
      localStorage.setItem(key, open ? "1" : "0");
    } catch {
      // Sem armazenamento: só não lembra.
    }
  }
</script>

<div class="expander" class:open>
  <button class="head" aria-expanded={open} onclick={toggle}>
    <Glyph name={icon} size={16} />
    <span class="title">{title}</span>
    {#if summary && !open}<span class="summary caption" title={summary}>{summary}</span>{/if}
    <span class="chevron"><Glyph name="chevronDown" size={12} /></span>
  </button>
  {#if open}
    <div class="body">{@render children()}</div>
  {/if}
</div>

<style>
  .expander {
    border: 1px solid var(--stroke);
    border-radius: var(--r-card);
    background: var(--card);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 40px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-card);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .head:hover {
    background: var(--layer);
  }
  .head:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .head > :global(.glyph:first-child) {
    color: var(--text-2);
  }
  .title {
    font-weight: 600;
    flex: none;
  }
  .summary {
    flex: 1;
    min-width: 0;
    text-align: right;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chevron {
    margin-left: auto;
    display: inline-flex;
    color: var(--text-2);
    transition: transform 0.15s;
  }
  .summary + .chevron {
    margin-left: 0;
  }
  .open .chevron {
    transform: rotate(180deg);
  }
  .open .head {
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 12px 12px;
    border-top: 1px solid var(--stroke);
  }
</style>
