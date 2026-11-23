// Redis opcional (filas e cache) no Windows, Docker ou WSL2.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, mockEmit } from "./api";
import { mockSetProjectQueueConnection } from "./projects";
import type { DiffLine } from "./services";

export type RedisBackend = "native" | "docker" | "wsl";
export type QueueConnection = "redis" | "none";

export interface RedisConfig {
  backend: RedisBackend;
  distro: string | null;
  port: number;
}

export interface RedisStatus {
  config: RedisConfig | null;
  running: boolean;
  /** Docker instalado (o Docker Desktop pode estar fechado). */
  docker: boolean;
  /** Distribuições do WSL encontradas. */
  distros: string[];
  image: string;
  /** Redis nativo (redis-server.exe) baixado. */
  native_installed: boolean;
  native_version: string | null;
  installing: boolean;
}

export interface QueueRedisPreview {
  configured: boolean;
  diff: DiffLine[];
  needs_predis: boolean;
  has_snapshot: boolean;
}

// ---------- Dados de exemplo (navegador) ----------
let mockCfg: RedisConfig | null = null;
let mockRunning = false;
let mockNative = false;
const changed = () => mockEmit("services-changed", null);

const mock = {
  status: async (): Promise<RedisStatus> => ({
    config: mockCfg,
    running: mockRunning,
    docker: true,
    distros: [],
    image: "redis:7.4-alpine",
    native_installed: mockNative,
    native_version: mockNative ? "8.10.3" : null,
    installing: false,
  }),
  install: async () => void ((mockNative = true), changed()),
  configure: async (backend: RedisBackend, distro: string | null, port: number) =>
    void ((mockCfg = { backend, distro, port }), changed()),
  remove: async () => void ((mockCfg = null), (mockRunning = false), changed()),
  start: async () => void ((mockRunning = true), changed()),
  stop: async () => void ((mockRunning = false), changed()),
  queuePreview: async (_path: string, connection: QueueConnection): Promise<QueueRedisPreview> => ({
    configured: !!mockCfg,
    diff: connection === "none" ? [{ key: "QUEUE_CONNECTION", before: "redis", after: "sync" }] : [
      { key: "QUEUE_CONNECTION", before: "sync", after: "redis" },
      { key: "REDIS_PORT", before: "6379", after: String(mockCfg?.port ?? 6379) },
    ],
    needs_predis: false,
    has_snapshot: true,
  }),
  queueConnection: async (path: string, connection: QueueConnection) =>
    mockSetProjectQueueConnection(path, connection === "redis" ? connection : null),
  installPredis: async (_path: string) => {},
};

export const redis = inTauri
  ? {
      status: () => invoke<RedisStatus>("redis_status"),
      install: () => invoke<void>("redis_install"),
      configure: (backend: RedisBackend, distro: string | null, port: number) =>
        invoke<void>("redis_configure", { backend, distro, port }),
      remove: () => invoke<void>("redis_remove"),
      start: () => invoke<void>("redis_start"),
      stop: () => invoke<void>("redis_stop"),
      queuePreview: (path: string, connection: QueueConnection) =>
        invoke<QueueRedisPreview>("project_queue_redis_preview", { path, connection }),
      queueConnection: (path: string, connection: QueueConnection) =>
        invoke<void>("project_queue_connection", { path, connection }),
      installPredis: (path: string) => invoke<void>("project_install_predis", { path }),
    }
  : mock;
