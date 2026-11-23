// Projetos: cadastro (importar, editar, remover, ignorar) e dados de exemplo.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, mockConfig, mockEmit, type ProjectKind } from "./api";

export type ProjectStatus = "new" | "imported" | "missing";

export interface Project {
  path: string;
  name: string;
  /** Pasta raiz; `null` = projeto avulso. */
  root: string | null;
  kind: ProjectKind;
  status: ProjectStatus;
  domain: string;
  /** Domínios extras, sem o TLD (`finder.noveck` → finder.noveck.local). */
  aliases: string[];
  docroot: string;
  /** Minor do PHP fixado; `null` = automático. */
  php_pin: string | null;
  /** O que o projeto pede (composer.json / .php-version). */
  php_request: string | null;
  has_node: boolean;
  node_enabled: boolean;
  node_command: string | null;
  node_request: string | null;
  /** Major do Node fixado; `null` = automático. */
  node_pin: string | null;
  /** Banco gerenciado: "mariadb", "sqlite" ou null. */
  database: string | null;
  db_version: string | null;
  db_port: number | null;
  mail: string | null;
  storage: string | null;
  workers: string[];
  /** Porta do Reverb escolhida pelo Larafim; `null` = ainda não ligado. */
  reverb_port: number | null;
  /** Filas do worker (`--queue=`); vazio = descobrir ao ligar. */
  queues: string[];
  /** Conexão do worker de fila: "redis" (Redis do Larafim) ou `null` (a do .env). */
  queue_connection: string | null;
  online: boolean;
}

/** O que é gravado em projects.toml. */
export interface ProjectConfig {
  path: string;
  domain: string;
  php: string | null;
  docroot: string;
  node: boolean;
  node_command: string | null;
  node_version: string | null;
  database: string | null;
  db_version: string | null;
  db_port: number | null;
  online: boolean;
}

export interface ProjectsOverview {
  projects: Project[];
  ignored: string[];
}

export interface FolderProposal {
  config: ProjectConfig;
  view_kind: ProjectKind;
  recognized: boolean;
  php_request: string | null;
  has_node: boolean;
  docroots: string[];
  already_imported: boolean;
}

export function toConfig(p: Project): ProjectConfig {
  return {
    path: p.path,
    domain: p.domain,
    php: p.php_pin,
    docroot: p.docroot,
    node: p.node_enabled,
    node_command: p.node_command,
    node_version: p.node_pin,
    database: p.database,
    db_version: p.db_version,
    db_port: p.db_port,
    online: p.online,
  };
}

/** Normaliza o texto digitado para um rótulo de domínio válido. */
export function slugDomain(text: string): string {
  return text
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

// ---------- Dados de exemplo (navegador) ----------
const sample = (
  name: string,
  kind: ProjectKind,
  status: ProjectStatus,
  php_request: string | null,
  node: boolean,
  root: string | null = "D:\\www",
): Project => ({
  path: `${root ?? "C:\\Projetos"}\\${name}`,
  name,
  root,
  kind,
  status,
  domain: name,
  aliases: [],
  docroot: kind === "node" ? "" : "public",
  php_pin: null,
  php_request,
  has_node: node,
  node_enabled: kind === "node",
  node_command: node ? "npm run dev" : null,
  node_request: node ? "22" : null,
  node_pin: null,
  database: null,
  db_version: null,
  db_port: null,
  mail: null,
  storage: null,
  workers: [],
  reverb_port: null,
  queues: [],
  queue_connection: null,
  online: false,
});

let mockProjects: Project[] = [
  sample("api-pagamentos", "laravel", "imported", "^8.2", false),
  sample("blog-pessoal", "php", "new", "8.1", false),
  sample("loja-virtual", "laravel", "imported", "^8.3", true),
  sample("painel-admin", "laravel", "new", "^8.4", true),
  sample("site-institucional", "node", "imported", null, true),
  sample("sistema-legado", "php", "imported", "^7.4", false, null),
  { ...sample("prototipo-antigo", "php", "missing", null, false), docroot: "" },
];
let mockIgnored: string[] = ["D:\\www\\testes-velhos"];

/** Usado pelos dados de exemplo do servidor. */
export function mockSetProjectOnline(path: string, online: boolean) {
  mockProjects = mockProjects.map((p) => (p.path === path ? { ...p, online } : p));
  mockEmit("projects-changed", mockOverview());
}

function mockOverview(): ProjectsOverview {
  const projects = mockConfig.roots.length ? mockProjects : mockProjects.filter((p) => p.root === null);
  return { projects: projects.map((p) => ({ ...p })), ignored: [...mockIgnored] };
}

function mockChange(fn: () => void): ProjectsOverview {
  fn();
  const data = mockOverview();
  mockEmit("projects-changed", data);
  return data;
}

const mock = {
  list: async () => mockOverview(),
  import: async (path: string) =>
    mockChange(() => (mockProjects = mockProjects.map((p) => (p.path === path ? { ...p, status: "imported" } : p)))),
  importAll: async () =>
    mockChange(() => (mockProjects = mockProjects.map((p) => (p.status === "new" ? { ...p, status: "imported" } : p)))),
  save: async (config: ProjectConfig) => {
    if (mockProjects.some((p) => p.path !== config.path && p.domain === config.domain)) {
      throw `o domínio "${config.domain}" já é usado por outro projeto`;
    }
    return mockChange(() => {
      const exists = mockProjects.some((p) => p.path === config.path);
      const updated = (p: Project): Project => ({
        ...p,
        domain: config.domain,
        php_pin: config.php,
        docroot: config.docroot,
        node_enabled: config.node,
        node_command: config.node_command,
        node_pin: config.node_version,
        status: "imported",
      });
      mockProjects = exists
        ? mockProjects.map((p) => (p.path === config.path ? updated(p) : p))
        : [...mockProjects, updated(sample(config.path.split("\\").pop()!, "laravel", "imported", "^8.3", false, null))];
    });
  },
  setAliases: async (path: string, aliases: string[]) => {
    const taken = mockProjects.find((p) => p.path !== path && aliases.some((a) => a === p.domain || p.aliases.includes(a)));
    if (taken) throw `o domínio já é usado por outro projeto`;
    return mockChange(() => (mockProjects = mockProjects.map((p) => (p.path === path ? { ...p, aliases } : p))));
  },
  remove: async (path: string) =>
    mockChange(() => {
      const p = mockProjects.find((x) => x.path === path);
      mockProjects = mockProjects.filter((x) => x.path !== path);
      if (p?.root) mockIgnored.push(path);
    }),
  ignore: async (path: string) =>
    mockChange(() => {
      mockProjects = mockProjects.filter((x) => x.path !== path);
      mockIgnored.push(path);
    }),
  unignore: async (path: string) =>
    mockChange(() => {
      mockIgnored = mockIgnored.filter((x) => x !== path);
      mockProjects.push(sample(path.split("\\").pop()!, "php", "new", null, false));
    }),
  inspectFolder: async (path: string): Promise<FolderProposal> => ({
    config: {
      path,
      domain: slugDomain(path.split("\\").pop() ?? "projeto"),
      php: null,
      docroot: "public",
      node: false,
      node_command: "npm run dev",
      node_version: null,
      database: null,
      db_version: null,
      db_port: null,
      online: false,
    },
    view_kind: "laravel",
    recognized: true,
    php_request: "^8.3",
    has_node: true,
    docroots: ["", "public"],
    already_imported: false,
  }),
  docroots: async (_path: string) => ["", "public"],
};

export const projectsApi = inTauri
  ? {
      list: () => invoke<ProjectsOverview>("list_projects"),
      import: (path: string) => invoke<ProjectsOverview>("import_project", { path }),
      importAll: () => invoke<ProjectsOverview>("import_all"),
      save: (config: ProjectConfig) => invoke<ProjectsOverview>("save_project", { config }),
      setAliases: (path: string, aliases: string[]) =>
        invoke<ProjectsOverview>("set_project_aliases", { path, aliases }),
      remove: (path: string) => invoke<ProjectsOverview>("remove_project", { path }),
      ignore: (path: string) => invoke<ProjectsOverview>("ignore_project", { path }),
      unignore: (path: string) => invoke<ProjectsOverview>("unignore_project", { path }),
      inspectFolder: (path: string) => invoke<FolderProposal>("inspect_folder", { path }),
      docroots: (path: string) => invoke<string[]>("project_docroots", { path }),
    }
  : mock;
