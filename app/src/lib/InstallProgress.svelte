<script lang="ts">
  // Andamento de uma instalação no projeto (Composer/Artisan): porcentagem
  // estimada pelas etapas do Composer e a última linha da saída.
  import { onEvent } from "./api";
  import ProgressBar from "./ProgressBar.svelte";

  let { path, label }: { path: string; label: string } = $props();

  let percent = $state(0);
  let line = $state("");

  $effect(() => {
    const target = path.toLowerCase();
    return onEvent<{ path: string; line: string; percent: number }>("project-install", (e) => {
      if (e.path.toLowerCase() !== target) return;
      percent = e.percent;
      if (e.line) line = e.line;
    });
  });
</script>

<div class="install">
  <div class="head">
    <span class="caption">{label}</span>
    <span class="caption pct">{percent}%</span>
  </div>
  <ProgressBar value={percent / 100} />
  {#if line}<span class="caption mono line" title={line}>{line}</span>{/if}
</div>

<style>
  .install {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
  }
  .pct {
    font-variant-numeric: tabular-nums;
  }
  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
