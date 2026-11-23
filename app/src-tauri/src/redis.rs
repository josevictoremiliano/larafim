//! Redis opcional (filas e cache): nativo no Windows (redis-server.exe, como
//! o Herd Pro), no Docker ou no WSL2. Sobe sob demanda (projeto com fila no
//! Redis ou o botão em Serviços) e desce ao sair.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use larafim_core::envfile::{self, DiffLine};
use larafim_core::redis::{self as core_redis, CONTAINER, DOCKER_IMAGE, VOLUME};
use larafim_core::{RedisConfig, t};
use larafim_runtimes::redis as rt_redis;
use larafim_server::DevSpec;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::server::ServerState;

type CmdResult<T> = Result<T, String>;

pub const REDIS_KEY: &str = "service:redis";

pub fn redis_config(app: &AppHandle) -> Option<RedisConfig> {
    app.state::<AppState>().config.lock().unwrap().redis.clone()
}

pub fn redis_running(app: &AppHandle) -> bool {
    app.state::<ServerState>().dev.is_running(REDIS_KEY)
}

/// Roda um programa sem janela, com tempo limite. `None` = não rodou ou passou do tempo.
pub(crate) fn run_hidden(exe: &str, args: &[&str], timeout: Duration) -> Option<(bool, Vec<u8>)> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(256 * 1024).read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        if let Ok(Some(s)) = child.try_wait() {
            break s;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    Some((status.success(), reader.join().unwrap_or_default()))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// O Docker está instalado e o serviço dele (Docker Desktop) ligado?
fn docker_ready() -> bool {
    run_hidden("docker", &["info", "--format", "{{.ServerVersion}}"], Duration::from_secs(15)).is_some_and(|(ok, _)| ok)
}

fn docker_installed() -> bool {
    run_hidden("docker", &["--version"], Duration::from_secs(10)).is_some_and(|(ok, _)| ok)
}

fn wsl_distros() -> Vec<String> {
    run_hidden("wsl.exe", &["-l", "-q"], Duration::from_secs(15))
        .filter(|(ok, _)| *ok)
        .map(|(_, out)| core_redis::parse_wsl_list(&out))
        .unwrap_or_default()
}

#[derive(Serialize)]
pub struct RedisStatus {
    config: Option<RedisConfig>,
    running: bool,
    docker: bool,
    distros: Vec<String>,
    image: &'static str,
    /// Redis nativo (redis-server.exe) baixado.
    native_installed: bool,
    native_version: Option<String>,
    installing: bool,
}

static INSTALLING: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

#[tauri::command]
pub async fn redis_status(app: AppHandle) -> CmdResult<RedisStatus> {
    let (docker, distros) = blocking(|| (docker_installed(), wsl_distros())).await?;
    let paths = app.state::<AppState>().paths.clone();
    Ok(RedisStatus {
        config: redis_config(&app),
        running: redis_running(&app),
        docker,
        distros,
        image: DOCKER_IMAGE,
        native_installed: rt_redis::redis_installed(&paths),
        native_version: rt_redis::installed_version(&paths),
        installing: *INSTALLING.lock().unwrap(),
    })
}

#[derive(Clone, Serialize)]
struct Progress {
    received: u64,
    total: Option<u64>,
}

/// Baixa o Redis nativo (GitHub, com SHA-256 do release).
#[tauri::command]
pub async fn redis_install(app: AppHandle) -> CmdResult<()> {
    {
        let mut i = INSTALLING.lock().unwrap();
        if *i {
            return Err(t!("já está instalando"));
        }
        *i = true;
    }
    let _ = app.emit("services-changed", ());
    let paths = app.state::<AppState>().paths.clone();
    let emitter = app.clone();
    let last = std::sync::Mutex::new(Instant::now() - Duration::from_secs(1));
    let on_progress = move |received: u64, total: Option<u64>| {
        let mut l = last.lock().unwrap();
        if l.elapsed() >= Duration::from_millis(100) {
            *l = Instant::now();
            let _ = emitter.emit("redis:progress", Progress { received, total });
        }
    };
    // Troca de versão com o servidor ligado: o exe estaria em uso.
    if redis_config(&app).is_some_and(|c| c.backend == "native") {
        stop_redis(&app).await;
    }
    let result = async {
        let client = larafim_runtimes::http_client().map_err(|e| e.to_string())?;
        rt_redis::install_redis(&client, &paths, &on_progress).await.map_err(|e| e.to_string())
    }
    .await;
    *INSTALLING.lock().unwrap() = false;
    let _ = app.emit("services-changed", ());
    result.map(|_| ())
}

/// Escolhe onde o Redis roda e a porta (desliga o atual se mudar).
#[tauri::command]
pub async fn redis_configure(app: AppHandle, backend: String, distro: Option<String>, port: u16) -> CmdResult<()> {
    let cfg = RedisConfig { backend, distro: distro.filter(|d| !d.is_empty()), port };
    cfg.validate().map_err(|e| e.to_string())?;
    if redis_config(&app).as_ref() != Some(&cfg) {
        stop_redis(&app).await;
    }
    let state = app.state::<AppState>();
    {
        let mut c = state.config.lock().unwrap();
        c.redis = Some(cfg);
        c.save(&state.paths.config_file()).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("services-changed", ());
    Ok(())
}

/// Tira o Redis do Larafim (o volume do Docker e o `dump.rdb` ficam).
#[tauri::command]
pub async fn redis_remove(app: AppHandle) -> CmdResult<()> {
    stop_redis(&app).await;
    let state = app.state::<AppState>();
    {
        let mut c = state.config.lock().unwrap();
        c.redis = None;
        c.save(&state.paths.config_file()).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("services-changed", ());
    Ok(())
}

/// Liga o Redis e espera a porta responder.
pub async fn start_redis(app: &AppHandle) -> Result<(), String> {
    if redis_running(app) {
        return Ok(());
    }
    let cfg = redis_config(app).ok_or_else(|| t!("adicione o Redis em Configurações › Serviços"))?;
    cfg.validate().map_err(|e| e.to_string())?;
    if let Err(owner) = larafim_server::port::check_free(cfg.port) {
        let who = owner.map(|o| o.name).unwrap_or_else(|| t!("outro programa"));
        return Err(t!("a porta {port} (Redis) está ocupada por {who}", port = cfg.port, who = who));
    }
    let data = app.state::<AppState>().paths.data_dir().join("redis");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    // Linha de comando só com valores fixos ou validados (porta e nome da distribuição).
    let paths = app.state::<AppState>().paths.clone();
    let (command, cwd) = match cfg.backend.as_str() {
        "native" => {
            if !rt_redis::redis_installed(&paths) {
                return Err(t!("o Redis não está instalado (Configurações › Serviços)"));
            }
            // Dados em data\redis (pasta de trabalho); só em 127.0.0.1.
            (
                format!(
                    "\"{}\" --port {} --bind 127.0.0.1 --protected-mode no --dir . --appendonly yes",
                    rt_redis::redis_server_exe(&paths).display(),
                    cfg.port
                ),
                data.clone(),
            )
        }
        "docker" => {
            if !blocking(docker_ready).await? {
                return Err(t!("o Docker não está respondendo: abra o Docker Desktop e tente de novo"));
            }
            // Sobra de uma sessão anterior (o Larafim caiu, por exemplo).
            blocking(|| run_hidden("docker", &["rm", "-f", CONTAINER], Duration::from_secs(30))).await?;
            (
                format!(
                    "docker run --rm --name {CONTAINER} -p 127.0.0.1:{}:6379 -v {VOLUME}:/data {DOCKER_IMAGE} redis-server --appendonly yes",
                    cfg.port
                ),
                data.clone(),
            )
        }
        _ => {
            let distro = cfg.distro.clone().unwrap_or_default();
            let d = distro.clone();
            let has = blocking(move || {
                run_hidden("wsl.exe", &["-d", &d, "--exec", "which", "redis-server"], Duration::from_secs(60)).is_some_and(|(ok, _)| ok)
            })
            .await?;
            if !has {
                return Err(t!(
                    "o redis-server não está instalado no {distro}. Instale pelo repositório oficial (packages.redis.io): sudo apt-get install redis",
                    distro = distro
                ));
            }
            // Em primeiro plano (sem --daemonize): o Larafim controla o processo.
            // Só em 127.0.0.1: o WSL2 repassa para o Windows sem abrir para a rede.
            (format!("wsl.exe -d {distro} --exec redis-server --port {} --bind 127.0.0.1 --protected-mode no", cfg.port), data.clone())
        }
    };
    app.state::<ServerState>()
        .dev
        .start(DevSpec { key: REDIS_KEY.into(), cwd, command, node_dir: data, env: Vec::new() })
        .map_err(|e| e.to_string())?;
    // Na primeira vez o Docker baixa a imagem: espera mais.
    let started = Instant::now();
    while tokio::net::TcpStream::connect(("127.0.0.1", cfg.port)).await.is_err() {
        if !redis_running(app) || started.elapsed() > Duration::from_secs(180) {
            let log = app.state::<ServerState>().dev.logs(REDIS_KEY).join(" | ");
            stop_redis(app).await;
            let tail: String = log.chars().rev().take(400).collect::<String>().chars().rev().collect();
            return Err(t!("o Redis não iniciou: {a}", a = tail));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tracing::info!(backend = cfg.backend, port = cfg.port, "Redis rodando");
    let _ = app.emit("services-changed", ());
    Ok(())
}

/// Desliga o processo e o servidor de verdade (o contêiner ou o redis-server
/// do WSL não morrem só com o fim do `docker`/`wsl.exe`).
pub async fn stop_redis(app: &AppHandle) {
    let was = redis_running(app);
    if let Some(cfg) = redis_config(app).filter(|_| was) {
        let cli = rt_redis::redis_cli_exe(&app.state::<AppState>().paths);
        // Primeiro o servidor fecha direito (grava os dados); depois o processo.
        let _ = blocking(move || match cfg.backend.as_str() {
            "native" => run_hidden(&cli.display().to_string(), &["-p", &cfg.port.to_string(), "shutdown"], Duration::from_secs(20)),
            "docker" => run_hidden("docker", &["stop", CONTAINER], Duration::from_secs(30)),
            _ => {
                let (d, p) = (cfg.distro.unwrap_or_default(), cfg.port.to_string());
                run_hidden("wsl.exe", &["-d", &d, "--exec", "redis-cli", "-p", &p, "shutdown", "nosave"], Duration::from_secs(20))
            }
        })
        .await;
    }
    app.state::<ServerState>().dev.stop(REDIS_KEY);
    let _ = app.emit("services-changed", ());
}

#[tauri::command]
pub async fn redis_start(app: AppHandle) -> CmdResult<()> {
    start_redis(&app).await
}

#[tauri::command]
pub async fn redis_stop(app: AppHandle) {
    stop_redis(&app).await;
}

// ---------- Fila do projeto no Redis ----------

/// `composer require predis/predis` no projeto, com o PHP dele (a pessoa
/// confirma antes; o Larafim não traz a extensão phpredis para Windows).
#[tauri::command]
pub async fn project_install_predis(app: AppHandle, path: std::path::PathBuf) -> CmdResult<()> {
    if core_redis::has_predis(&path) {
        return Ok(());
    }
    if !path.join("composer.json").is_file() {
        return Err(t!("o projeto não tem composer.json"));
    }
    let paths = app.state::<AppState>().paths.clone();
    let composer = larafim_runtimes::extras::composer_path(&paths);
    if !composer.is_file() {
        return Err(t!("o Composer ainda não foi baixado (instale um PHP pelo Larafim)"));
    }
    let view = crate::server::views(&app.state::<AppState>())
        .into_iter()
        .find(|v| larafim_core::store::same_path(&v.path, &path))
        .ok_or_else(|| t!("projeto não encontrado"))?;
    let installed = larafim_runtimes::php::list_installed(&paths.php_dir());
    let php = crate::server::php_for(&view, &installed).0.ok_or_else(|| t!("nenhum PHP instalado atende a este projeto"))?;
    let exe = php.dir.join("php.exe");
    let (app2, dir) = (app.clone(), path.clone());
    let args = vec![
        composer.display().to_string(),
        "require".into(),
        "predis/predis".into(),
        "--no-interaction".into(),
        "--no-ansi".into(),
    ];
    let (ok, out) = blocking(move || crate::tools::run_install(&app2, &dir, &exe, &args, &[("COMPOSER_NO_INTERACTION", "1".into())])).await?;
    tracing::info!(path = %path.display(), ok, "composer require predis/predis");
    if !ok || !core_redis::has_predis(&path) {
        let tail: String = out.trim().chars().rev().take(500).collect::<String>().chars().rev().collect();
        return Err(t!("não foi possível instalar o predis/predis: {a}", a = tail));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct QueueRedisPreview {
    configured: bool,
    diff: Vec<DiffLine>,
    /// Projeto sem `predis/predis` (no Windows não há a extensão phpredis).
    needs_predis: bool,
}

#[tauri::command]
pub fn project_queue_redis_preview(app: AppHandle, path: std::path::PathBuf) -> CmdResult<QueueRedisPreview> {
    let Some(cfg) = redis_config(&app) else {
        return Ok(QueueRedisPreview { configured: false, diff: Vec::new(), needs_predis: false });
    };
    let base = std::fs::read_to_string(envfile::env_path(&path)).unwrap_or_default();
    Ok(QueueRedisPreview {
        configured: true,
        diff: envfile::apply(&base, &core_redis::env_changes(cfg.port)).1,
        needs_predis: !core_redis::has_predis(&path),
    })
}

/// Fila do projeto no Redis do Larafim (`connection = "redis"`) ou de volta
/// ao banco (`database`). Ajusta o `.env` com cópia e reinicia o worker.
#[tauri::command]
pub async fn project_queue_connection(app: AppHandle, path: std::path::PathBuf, connection: String) -> CmdResult<()> {
    let changes = match connection.as_str() {
        "redis" => {
            let cfg = redis_config(&app).ok_or_else(|| t!("adicione o Redis em Configurações › Serviços"))?;
            if !core_redis::has_predis(&path) {
                return Err(t!("o projeto precisa do pacote predis/predis: rode composer require predis/predis"));
            }
            start_redis(&app).await?;
            core_redis::env_changes(cfg.port)
        }
        "database" => vec![envfile::EnvChange::set("QUEUE_CONNECTION", "database")],
        other => return Err(t!("conexão de fila desconhecida: {a}", a = other.escape_debug())),
    };
    if envfile::env_path(&path).exists() {
        envfile::write(&path, &changes).map_err(|e| e.to_string())?;
        crate::tools::env_changed(&app, &path);
    }
    crate::tools::update_project(&app, &path, |p| p.queue_connection = (connection == "redis").then(|| connection.clone()))?;
    crate::tools::restart_queue_if_running(&app, &path)?;
    tracing::info!(path = %path.display(), connection, "conexão da fila alterada");
    Ok(())
}

/// Algum projeto online usa o Redis?
pub fn wanted_by(view: &larafim_core::ProjectView) -> bool {
    view.queue_connection.as_deref() == Some("redis")
}

pub fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.state::<AppState>().paths.data_dir().join("redis")
}
