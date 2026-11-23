<script lang="ts" module>
  // Ícones nativos do Windows 11 (fonte Segoe Fluent Icons, já instalada no sistema).
  const glyphs = {
    add: "",
    refresh: "",
    search: "",
    settings: "",
    folder: "",
    folderOpen: "",
    globe: "",
    mail: "",
    copy: "",
    delete: "",
    info: "",
    code: "",
    terminal: "",
    shield: "",
    link: "",
    openExternal: "",
    database: "",
    server: "",
    brush: "",
    home: "",
    play: "",
    minimize: "",
    maximize: "",
    restore: "",
    close: "",
    chevronDown: "",
    chevronRight: "",
    package: "",
    pin: "",
    check: "",
    logs: "",
    bug: "",
    mailOpen: "",
    power: "",
    apps: "",
    stop: "",
    flash: "",
    layers: "",
  } as const;

  export type GlyphName = keyof typeof glyphs;
</script>

<script lang="ts">
  let { name, size = 16 }: { name: GlyphName; size?: number } = $props();
</script>

<span class="glyph" style:font-size="{size}px" aria-hidden="true">{glyphs[name]}</span>

<style>
  .glyph {
    font-family: var(--icons);
    font-style: normal;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
  }
</style>
