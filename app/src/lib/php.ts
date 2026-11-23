// Comandos de PHP (instalar, atualizar, extensões) e dados de exemplo para o navegador.
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { inTauri, mockEmit } from "./api";
import { locale, t } from "./i18n.svelte";

export interface PhpRow {
  minor: string;
  installed: string | null;
  previous: string | null;
  latest: string | null;
  size: string | null;
  update_available: boolean;
  installing: boolean;
}

export interface PhpOverview {
  rows: PhpRow[];
  composer: boolean;
  catalog_error: string | null;
  updates: number;
}

export interface PhpExtension {
  name: string;
  enabled: boolean;
  zend: boolean;
}

export interface PhpMatch {
  installed: string | null;
  available: string[];
  suggest: string | null;
}

export type PhpStage =
  | { stage: "preparing" }
  | { stage: "downloading"; received: number; total: number | null }
  | { stage: "extracting" }
  | { stage: "configuring" }
  | { stage: "verifying" };

export type PhpProgress = PhpStage & { minor: string };

export interface PhpDone {
  minor: string;
  ok: boolean;
  version: string | null;
  error: string | null;
  code: "vc_redist" | "cancelled" | "network" | "error" | null;
}

/** Texto curto da etapa, para mostrar ao lado da barra de progresso. */
export function stageLabel(p: PhpStage): string {
  switch (p.stage) {
    case "preparing":
      return t("Preparando…");
    case "downloading": {
      const mb = (n: number) => (n / 1024 / 1024).toLocaleString(locale(), { maximumFractionDigits: 1 });
      return p.total ? t("Baixando {a} de {b} MB", { a: mb(p.received), b: mb(p.total) }) : t("Baixando {a} MB", { a: mb(p.received) });
    }
    case "extracting":
      return t("Extraindo…");
    case "configuring":
      return "Configurando php.ini…";
    case "verifying":
      return "Verificando…";
  }
}

/** Fração 0–1 da barra (ou `null` para barra indeterminada). */
export function stageFraction(p: PhpStage): number | null {
  if (p.stage === "downloading" && p.total) return p.received / p.total;
  return null;
}

export async function confirm(message: string, title: string, okLabel: string): Promise<boolean> {
  if (!inTauri) return window.confirm(message);
  return ask(message, { title, kind: "warning", okLabel, cancelLabel: t("Cancelar") });
}

// ---------- Dados de exemplo (navegador) ----------
const mockRows: PhpRow[] = [
  ["8.6", "8.6.0", "34.1MB"],
  ["8.5", "8.5.13", "33.0MB"],
  ["8.4", "8.4.28", "31.4MB"],
  ["8.3", "8.3.37", "29.9MB"],
  ["8.2", "8.2.36", "29.0MB"],
  ["8.1", "8.1.34", "28.7MB"],
  ["8.0", "8.0.30", "27.5MB"],
  ["7.4", "7.4.33", "25.0MB"],
].map(([minor, latest, size]) => ({
  minor,
  latest,
  size,
  installed: null,
  previous: null,
  update_available: false,
  installing: false,
}));
mockRows[2] = { ...mockRows[2], installed: "8.4.27", update_available: true };
mockRows[3] = { ...mockRows[3], installed: "8.3.37" };

let mockDeprecations = false;
let mockExts: PhpExtension[] = ["bz2", "curl", "fileinfo", "gd", "gmp", "intl", "mbstring", "mysqli", "opcache", "openssl", "pdo_mysql", "pdo_pgsql", "pdo_sqlite", "sodium", "xsl", "zip"].map(
  (name) => ({ name, enabled: !["bz2", "gmp", "pdo_pgsql", "xsl"].includes(name), zend: name === "opcache" }),
);

const mock = {
  overview: async (_refresh = false): Promise<PhpOverview> => ({
    rows: mockRows.map((r) => ({ ...r })),
    composer: true,
    catalog_error: null,
    updates: mockRows.filter((r) => r.update_available).length,
  }),
  install: async (minor: string) => {
    const row = mockRows.find((r) => r.minor === minor)!;
    row.installing = true;
    const total = 31_400_000;
    let received = 0;
    const timer = setInterval(() => {
      received += 2_500_000;
      if (received < total) {
        mockEmit("php:progress", { minor, stage: "downloading", received, total });
        return;
      }
      clearInterval(timer);
      mockEmit("php:progress", { minor, stage: "configuring" });
      setTimeout(() => {
        Object.assign(row, { installing: false, installed: row.latest, update_available: false });
        mockEmit("php:done", { minor, ok: true, version: row.latest, error: null, code: null });
      }, 600);
    }, 120);
  },
  cancel: async (_minor: string) => {},
  remove: async (minor: string) => {
    Object.assign(mockRows.find((r) => r.minor === minor)!, { installed: null, update_available: false });
  },
  extensions: async (_version: string) => mockExts.map((e) => ({ ...e })),
  setExtension: async (_version: string, name: string, enabled: boolean) => {
    mockExts = mockExts.map((e) => (e.name === name ? { ...e, enabled } : e));
    return mockExts.map((e) => ({ ...e }));
  },
  deprecations: async (_version: string) => mockDeprecations,
  setDeprecations: async (_version: string, show: boolean) => (mockDeprecations = show),
  openIni: async (_version: string) => {},
  openFolder: async (_version: string) => {},
  match: async (constraint: string | null, pinned: string | null = null): Promise<PhpMatch> => {
    const available = mockRows.filter((r) => r.installed).map((r) => r.minor);
    if (pinned) {
      const row = mockRows.find((r) => r.minor === pinned && r.installed);
      return { installed: row?.installed ?? null, available, suggest: row ? null : pinned };
    }
    const c = constraint ?? "";
    const pick = c.startsWith("^8.4") || c.startsWith("^8.2") || c.startsWith("^8.3") ? "8.4" : c === "8.1" ? null : "8.4";
    const row = mockRows.find((r) => r.minor === pick && r.installed);
    return { installed: row?.installed ?? null, available, suggest: row ? null : "8.1" };
  },
  openVcRedist: async () => {},
};

export const php = inTauri
  ? {
      overview: (refresh = false) => invoke<PhpOverview>("php_overview", { refresh }),
      install: (minor: string) => invoke<void>("php_install", { minor }),
      cancel: (minor: string) => invoke<void>("php_cancel", { minor }),
      remove: (minor: string) => invoke<void>("php_remove", { minor }),
      extensions: (version: string) => invoke<PhpExtension[]>("php_extensions", { version }),
      setExtension: (version: string, name: string, enabled: boolean) =>
        invoke<PhpExtension[]>("php_set_extension", { version, name, enabled }),
      deprecations: (version: string) => invoke<boolean>("php_deprecations", { version }),
      setDeprecations: (version: string, show: boolean) => invoke<boolean>("php_set_deprecations", { version, show }),
      openIni: (version: string) => invoke<void>("php_open_ini", { version }),
      openFolder: (version: string) => invoke<void>("php_open_folder", { version }),
      match: (constraint: string | null, pinned: string | null = null) =>
        invoke<PhpMatch>("php_match", { constraint, pinned }),
      openVcRedist: () => invoke<void>("open_vc_redist"),
    }
  : mock;
