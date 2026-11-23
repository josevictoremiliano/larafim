// Ferramentas: Mailpit, logs, workers do Laravel, Xdebug e projeto Laravel novo.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, mockEmit } from "./api";
import type { DevInfo } from "./node";
import type { DiffLine } from "./services";

export interface MailpitStatus {
  installed: boolean;
  version: string | null;
  running: boolean;
  ui_url: string;
  smtp: string;
}

export interface LogFile {
  name: string;
  size: number;
  modified: number;
}

export interface LogChunk {
  text: string;
  offset: number;
  reset: boolean;
}

export interface XdebugInfo {
  downloaded: boolean;
  enabled: boolean;
  launch_json: string;
}

export type Worker = "queue" | "schedule" | "reverb";
export type Kit = "none" | "react" | "vue" | "livewire";

/** Modelo do "Novo Laravel" (a lista vem do Rust). */
export interface LaravelTemplate {
  id: string;
  laravel: string;
  kit: Kit;
  /** "React 19"; vazio no "só Laravel". */
  label: string;
  package: string;
  php_min: string;
  php_max: string;
  /** Starter kit de um branch em desenvolvimento (dev-main). */
  preview: boolean;
}

/** "8.3" → 803, para comparar minors. */
export const minorNum = (v: string) => {
  const [a, b] = v.split(".").map(Number);
  return Number.isInteger(a) && Number.isInteger(b) ? a * 100 + b : NaN;
};

export const templateAccepts = (t: LaravelTemplate, php: string) =>
  minorNum(t.php_min) <= minorNum(php) && minorNum(php) <= minorNum(t.php_max);

export interface ReverbPreview {
  installed: boolean;
  port: number;
  diff: DiffLine[];
  from_example: boolean;
  no_env: boolean;
}

export interface NewLaravel {
  name: string;
  root: string;
  php: string;
  /** Id de um `LaravelTemplate`. */
  template: string;
  database: "sqlite" | "mariadb";
}

export interface Created {
  key: string;
  path: string;
  ok: boolean;
  error: string | null;
}

export const workerKey = (path: string, w: Worker) => `${path.toLowerCase()}|${w}`;

// ---------- Dados de exemplo (navegador) ----------
let mailRunning = false;
let xdebugOn = false;
let mockLog = [
  "[2026-11-20 18:00:01] local.INFO: Pedido 42 criado  ",
  '[2026-11-20 18:00:02] local.DEBUG: Cache da vitrine renovado {"chaves":12}',
  "[2026-11-20 18:00:03] local.WARNING: Imagem do produto 7 não encontrada, usando a padrão  ",
  "[2026-11-20 18:00:04] local.INFO: Pedido 43 criado  ",
  '[2026-11-20 18:00:05] local.ERROR: cURL error 7: Failed to connect to frete.exemplo port 443 {"userId":3,"exception":"[object] (Illuminate\\\\Http\\\\Client\\\\ConnectionException(code: 0): cURL error 7: Failed to connect to frete.exemplo port 443 at C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Http\\\\Client\\\\PendingRequest.php:1621)',
  "[stacktrace]",
  "#0 C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Support\\\\helpers.php(338): Illuminate\\\\Http\\\\Client\\\\PendingRequest->Illuminate\\\\Http\\\\Client\\\\{closure}()",
  "#1 C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Http\\\\Client\\\\PendingRequest.php(927): retry()",
  "#2 C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Http\\\\Client\\\\PendingRequest.php(784): Illuminate\\\\Http\\\\Client\\\\PendingRequest->send()",
  "#3 C:\\\\Projetos\\\\loja-virtual\\\\app\\\\Services\\\\Frete.php(21): Illuminate\\\\Http\\\\Client\\\\PendingRequest->get()",
  "#4 C:\\\\Projetos\\\\loja-virtual\\\\app\\\\Http\\\\Controllers\\\\CarrinhoController.php(48): App\\\\Services\\\\Frete->calcular()",
  "#5 C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Routing\\\\Controller.php(54): App\\\\Http\\\\Controllers\\\\CarrinhoController->frete()",
  "#6 C:\\\\Projetos\\\\loja-virtual\\\\vendor\\\\laravel\\\\framework\\\\src\\\\Illuminate\\\\Routing\\\\ControllerDispatcher.php(43): Illuminate\\\\Routing\\\\Controller->callAction()",
  "#7 C:\\\\Projetos\\\\loja-virtual\\\\public\\\\index.php(17): Illuminate\\\\Foundation\\\\Application->handleRequest()",
  "#8 {main}",
  '"} ',
  "[2026-11-20 18:00:06] local.INFO: Pedido 44 criado  ",
  "",
].join("\n");

const mock = {
  mailpitStatus: async (): Promise<MailpitStatus> => ({
    installed: true,
    version: "v1.34.0",
    running: mailRunning,
    ui_url: "http://mail.offline",
    smtp: "127.0.0.1:1025",
  }),
  mailpitInstall: async () => {},
  mailpitStart: async () => void ((mailRunning = true), mockEmit("services-changed", null)),
  mailpitStop: async () => void ((mailRunning = false), mockEmit("services-changed", null)),
  openMailpit: async () => void window.open("about:blank"),
  mailPreview: async (_path: string, _kind: string): Promise<DiffLine[]> => [
    { key: "MAIL_MAILER", before: "log", after: "smtp" },
    { key: "MAIL_HOST", before: null, after: "127.0.0.1" },
    { key: "MAIL_PORT", before: null, after: "1025" },
  ],
  mailApply: async (_path: string, _kind: string) => {},
  logFiles: async (_path: string): Promise<LogFile[]> => [{ name: "laravel.log", size: mockLog.length, modified: 1795212000 }],
  logRead: async (_path: string, _name: string, offset: number | null): Promise<LogChunk> =>
    offset === null ? { text: mockLog, offset: mockLog.length, reset: true } : { text: "", offset: offset, reset: false },
  logClear: async (_path: string, _name: string) => void (mockLog = ""),
  logOpen: async (_path: string, _name: string) => {},
  workerSet: async (_path: string, _w: Worker, _enabled: boolean) => {},
  workerRestart: async (_path: string, _w: Worker) => {},
  processInfo: async (key: string): Promise<DevInfo> => ({
    status: { key, state: "running", port: null, exit_code: null, command: "php artisan queue:work" },
    logs: ["INFO  Processing jobs from the [default] queue."],
  }),
  hasReverb: async (_path: string) => true,
  installReverb: async (_path: string) => {},
  queuesDiscover: async (_path: string) => ["default", "whatsapp-inbound", "whatsapp", "chat"],
  queuesSet: async (_path: string, text: string) => text.split(",").map((q) => q.trim()).filter(Boolean),
  reverbPreview: async (_path: string): Promise<ReverbPreview> => ({
    installed: true,
    port: 8081,
    diff: [
      { key: "BROADCAST_CONNECTION", before: "log", after: "reverb" },
      { key: "REVERB_HOST", before: "localhost", after: "127.0.0.1" },
      { key: "REVERB_PORT", before: "8080", after: "8081" },
      { key: "REVERB_SERVER_PORT", before: null, after: "8081" },
    ],
    from_example: false,
    no_env: false,
  }),
  xdebugInfo: async (_v: string): Promise<XdebugInfo> => ({
    downloaded: xdebugOn,
    enabled: xdebugOn,
    launch_json: '{\n  "name": "Larafim: ouvir Xdebug",\n  "type": "php",\n  "request": "launch",\n  "port": 9003\n}',
  }),
  xdebugSet: async (v: string, enabled: boolean) => ((xdebugOn = enabled), mock.xdebugInfo(v)),
  laravelTemplates: async (): Promise<LaravelTemplate[]> => {
    const tpl = (id: string, laravel: string, kit: Kit, label: string, version: string, php_min: string, preview = false) => ({
      id, laravel, kit, label, package: `laravel/${kit === "none" ? "laravel" : `${kit}-starter-kit`}:${version}`, php_min, php_max: "8.5", preview,
    });
    return [
      tpl("13", "13", "none", "", "^13.0", "8.3"),
      tpl("13-react", "13", "react", "React 19", "dev-main", "8.3", true),
      tpl("13-vue", "13", "vue", "Vue 3.5", "dev-main", "8.3", true),
      tpl("13-livewire", "13", "livewire", "Livewire 4", "dev-main", "8.3", true),
      tpl("12", "12", "none", "", "^12.0", "8.2"),
      tpl("12-react", "12", "react", "React 19", "^1.0", "8.2"),
      tpl("12-vue", "12", "vue", "Vue 3.5", "^1.0", "8.2"),
      tpl("12-livewire", "12", "livewire", "Livewire 3", "^1.0", "8.2"),
    ];
  },
  laravelCreate: async (input: NewLaravel) => {
    const key = `create:d:\\www\\${input.name}`;
    const lines = ["Creating a \"laravel/laravel\" project at \"./" + input.name + "\"", "Installing dependencies", "  - Installing laravel/framework (v12.40.1)", "Application key set successfully."];
    lines.forEach((line, i) => setTimeout(() => mockEmit("node:log", { key, line }), 300 * (i + 1)));
    setTimeout(() => mockEmit("laravel:created", { key, path: `D:\\www\\${input.name}`, ok: true, error: null }), 1600);
    return key;
  },
};

export const toolsApi = inTauri
  ? {
      mailpitStatus: () => invoke<MailpitStatus>("mailpit_status"),
      mailpitInstall: () => invoke<void>("mailpit_install"),
      mailpitStart: () => invoke<void>("mailpit_start"),
      mailpitStop: () => invoke<void>("mailpit_stop"),
      openMailpit: () => invoke<void>("open_mailpit"),
      mailPreview: (path: string, kind: string) => invoke<DiffLine[]>("project_mail_preview", { path, kind }),
      mailApply: (path: string, kind: string) => invoke<void>("project_mail_apply", { path, kind }),
      logFiles: (path: string) => invoke<LogFile[]>("log_files", { path }),
      logRead: (path: string, name: string, offset: number | null) => invoke<LogChunk>("log_read", { path, name, offset }),
      logClear: (path: string, name: string) => invoke<void>("log_clear", { path, name }),
      logOpen: (path: string, name: string) => invoke<void>("log_open", { path, name }),
      workerSet: (path: string, worker: Worker, enabled: boolean) => invoke<void>("worker_set", { path, worker, enabled }),
      workerRestart: (path: string, worker: Worker) => invoke<void>("worker_restart", { path, worker }),
      processInfo: (key: string) => invoke<DevInfo>("process_info", { key }),
      hasReverb: (path: string) => invoke<boolean>("has_reverb", { path }),
      installReverb: (path: string) => invoke<void>("project_install_reverb", { path }),
      reverbPreview: (path: string) => invoke<ReverbPreview>("reverb_preview", { path }),
      queuesDiscover: (path: string) => invoke<string[]>("queues_discover", { path }),
      queuesSet: (path: string, text: string) => invoke<string[]>("queues_set", { path, text }),
      xdebugInfo: (version: string) => invoke<XdebugInfo>("xdebug_info", { version }),
      xdebugSet: (version: string, enabled: boolean) => invoke<XdebugInfo>("xdebug_set", { version, enabled }),
      laravelTemplates: () => invoke<LaravelTemplate[]>("laravel_templates"),
      laravelCreate: (input: NewLaravel) => invoke<string>("laravel_create", { input }),
    }
  : mock;
