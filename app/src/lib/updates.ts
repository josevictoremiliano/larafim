// Versões novas do Larafim (última release no GitHub) e links do projeto.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, mockEmit } from "./api";

export interface UpdateStatus {
  current: string;
  latest: string | null;
  available: boolean;
  published_at: string | null;
  checked_at: number | null;
  checking: boolean;
  error: string | null;
  /** Instalado pela Microsoft Store: as atualizações vêm de lá. */
  store: boolean;
  /** Baixando/instalando a versão nova (o app fecha e reabre sozinho). */
  installing: boolean;
  /** Progresso do download (0–100), quando se sabe o tamanho. */
  progress: number | null;
}

/** Links fixos do projeto; o endereço sai do Rust, nunca da interface. */
export type ProjectLink = "repo" | "issues" | "releases" | "security" | "license" | "docs" | "author" | "update";

let mockStatus: UpdateStatus = {
  current: "0.1.0",
  latest: null,
  available: false,
  published_at: null,
  checked_at: null,
  checking: false,
  error: null,
  store: false,
  installing: false,
  progress: null,
};

const mock = {
  status: async (): Promise<UpdateStatus> => ({ ...mockStatus }),
  check: async (): Promise<UpdateStatus> => {
    await new Promise((r) => setTimeout(r, 600));
    mockStatus = {
      ...mockStatus,
      latest: "0.2.0",
      available: true,
      published_at: "2026-11-21T12:00:00Z",
      checked_at: Math.floor(Date.now() / 1000),
    };
    mockEmit("app:update", mockStatus);
    return { ...mockStatus };
  },
  install: async (): Promise<void> => {
    for (let p = 0; p <= 100; p += 20) {
      mockStatus = { ...mockStatus, installing: true, progress: p };
      mockEmit("app:update", mockStatus);
      await new Promise((r) => setTimeout(r, 300));
    }
    mockStatus = { ...mockStatus, installing: false, progress: null };
    mockEmit("app:update", mockStatus);
  },
  openLink: async (link: ProjectLink) => void window.open(`https://github.com/#${link}`, "_blank"),
};

export const updates = inTauri
  ? {
      status: () => invoke<UpdateStatus>("app_update_status"),
      check: () => invoke<UpdateStatus>("app_update_check"),
      /** Baixa, confere a assinatura e instala; o app fecha e reabre. */
      install: () => invoke<void>("app_update_install"),
      openLink: (kind: ProjectLink) => invoke<void>("open_project_link", { kind }),
    }
  : mock;
