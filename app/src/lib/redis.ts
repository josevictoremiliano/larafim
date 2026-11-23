// Redis opcional (filas e cache) no Docker ou no WSL2.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, mockEmit } from "./api";
import type { DiffLine } from "./services";

export type RedisBackend = "native" | "docker" | "wsl";

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
    distros: ["Ubuntu"],
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
  queuePreview: async (_path: string): Promise<QueueRedisPreview> => ({
    configured: !!mockCfg,
    diff: [
      { key: "QUEUE_CONNECTION", before: "database", after: "redis" },
      { key: "REDIS_PORT", before: "6379", after: String(mockCfg?.port ?? 6379) },
    ],
    needs_predis: false,
  }),
  queueConnection: async (_path: string, _connection: "redis" | "database") => {},
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
      queuePreview: (path: string) => invoke<QueueRedisPreview>("project_queue_redis_preview", { path }),
      queueConnection: (path: string, connection: "redis" | "database") =>
        invoke<void>("project_queue_connection", { path, connection }),
      installPredis: (path: string) => invoke<void>("project_install_predis", { path }),
    }
  : mock;
