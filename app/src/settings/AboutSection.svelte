<script lang="ts">
  import { locale, t } from "../lib/i18n.svelte";
  import { api, onEvent } from "../lib/api";
  import Glyph from "../lib/Glyph.svelte";
  import Logo from "../lib/Logo.svelte";
  import Toggle from "../lib/Toggle.svelte";
  import { app } from "../lib/state.svelte";
  import { updates, type ProjectLink, type UpdateStatus } from "../lib/updates";
  import SettingRow from "./SettingRow.svelte";

  // Sobre: versão, autor, links do projeto e atualizações.

  let status = $state<UpdateStatus | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    updates.status().then((s) => (status = s)).catch(() => {});
    return onEvent<UpdateStatus>("app:update", (s) => (status = s));
  });

  async function check() {
    error = null;
    try {
      status = await updates.check();
    } catch (e) {
      error = String(e);
    }
  }

  // Baixa, confere a assinatura e instala; o app fecha e reabre sozinho.
  async function install() {
    error = null;
    try {
      await updates.install();
    } catch (e) {
      error = String(e);
    }
  }

  const open = (link: ProjectLink) => updates.openLink(link).catch((e) => (error = String(e)));

  const when = (secs: number | null) =>
    secs ? new Date(secs * 1000).toLocaleString(locale(), { dateStyle: "short", timeStyle: "short" }) : null;

  const updateTitle = $derived.by(() => {
    if (!status) return t("Atualizações");
    if (status.installing && status.latest)
      return status.progress === null
        ? t("Baixando a versão {v}…", { v: status.latest })
        : t("Baixando a versão {v}… {p}%", { v: status.latest, p: status.progress });
    if (status.checking) return t("Procurando atualizações…");
    if (status.available && status.latest) return t("Versão {v} disponível", { v: status.latest });
    if (status.checked_at && !status.error && !status.latest) return t("Nenhuma versão publicada ainda");
    if (status.checked_at && !status.error) return t("Você está na versão mais recente");
    return t("Atualizações");
  });

  const updateDescription = $derived.by(() => {
    if (status?.installing) return t("O Larafim fecha, instala a versão nova e abre de novo sozinho.");
    if (status?.error) return t("Não foi possível verificar: {e}", { e: status.error });
    const checked = when(status?.checked_at ?? null);
    return checked ? t("Última verificação: {when}", { when: checked }) : t("Ainda não verificado nesta sessão");
  });

  const links: { kind: ProjectLink; icon: "code" | "info" | "bug" | "shield" | "logs"; title: () => string; description: () => string }[] = [
    { kind: "repo", icon: "code", title: () => t("Código-fonte"), description: () => "github.com/josevictoremiliano/larafim" },
    { kind: "docs", icon: "logs", title: () => t("Documentação"), description: () => t("Guias de uso, desenvolvimento e segurança") },
    { kind: "issues", icon: "bug", title: () => t("Reportar um problema"), description: () => t("Abra uma issue no GitHub") },
    { kind: "security", icon: "shield", title: () => t("Segurança"), description: () => t("Reporte falhas de segurança de forma privada") },
    { kind: "license", icon: "info", title: () => t("Licença"), description: () => t("MIT: livre para usar, modificar e distribuir") },
  ];
</script>

<div class="hero card">
  <Logo size={64} />
  <div class="who">
    <strong>Larafim</strong>
    <span class="caption">{t("Versão {v}", { v: app.info?.version ?? status?.current ?? "" })}</span>
    <span class="caption">{t("PHP, Node e bancos locais para seus projetos no Windows.")}</span>
    <span class="caption author">
      {t("Feito por")}
      <button class="link" onclick={() => open("author")}>José Victor Emiliano</button>
    </span>
  </div>
</div>

{#if error}
  <div class="infobar err"><Glyph name="info" /><span>{error}</span></div>
{/if}

<h2 class="subhead">{t("Atualizações")}</h2>
{#if status?.store}
  <SettingRow
    icon="refresh"
    title={t("Atualizado pela Microsoft Store")}
    description={t("A Store instala as versões novas do Larafim automaticamente.")}
  />
{:else}
  <SettingRow icon="refresh" title={updateTitle} description={updateDescription}>
    {#if status?.available}
      <button class="btn" onclick={() => open("update")}><Glyph name="openExternal" /> {t("Novidades")}</button>
      <button class="btn accent" disabled={status.installing} onclick={install}>{t("Atualizar e reiniciar")}</button>
    {/if}
    <button class="btn" disabled={status?.checking || status?.installing} onclick={check}>{t("Verificar atualizações")}</button>
  </SettingRow>
  {#if app.config}
    <SettingRow
      icon="globe"
      title={t("Verificar automaticamente")}
      description={t("Consulta o GitHub ao abrir e a cada 6 horas. Só avisa: nada é baixado sem você pedir.")}
    >
      <span class="caption">{app.config.check_updates ? t("Ativado") : t("Desativado")}</span>
      <Toggle
        label={t("Verificar automaticamente")}
        checked={app.config.check_updates}
        onchange={(v) => app.update({ check_updates: v })}
      />
    </SettingRow>
  {/if}
{/if}

<h2 class="subhead">{t("Projeto")}</h2>
{#each links as l (l.kind)}
  <SettingRow icon={l.icon} title={l.title()} description={l.description()}>
    <button class="btn" onclick={() => open(l.kind)}><Glyph name="openExternal" /> {t("Abrir")}</button>
  </SettingRow>
{/each}

<h2 class="subhead">{t("Este computador")}</h2>
<SettingRow icon="folder" title={t("Pasta de dados")} description={app.info?.data_dir}>
  <button class="btn" onclick={() => api.openDataDir()}>{t("Abrir")}</button>
</SettingRow>

<style>
  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 20px;
  }
  .who {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .who strong {
    font: 600 20px/28px var(--font-display);
  }
  .author {
    margin-top: 6px;
  }
  .link {
    border: 0;
    padding: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
  }
  .link:hover {
    text-decoration: underline;
  }
  .infobar.err {
    margin-top: 8px;
    background: color-mix(in srgb, var(--err) 12%, var(--card));
    user-select: text;
  }
  .subhead {
    margin: 28px 0 8px;
    font: 600 14px/20px var(--font);
  }
  .btn :global(.glyph) {
    margin-right: 4px;
  }
</style>
