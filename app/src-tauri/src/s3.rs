//! Storage S3 local (fase 8): RustFS sob demanda. API em 127.0.0.1:9000,
//! painel em s3.<domínio> (proxy para 127.0.0.1:9001), um bucket por projeto.

use larafim_core::t;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use larafim_core::envfile::{self, DiffLine, EnvChange};
use larafim_runtimes::{http_client, s3};
use larafim_server::DevSpec;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::server::ServerState;

type CmdResult<T> = Result<T, String>;

pub const S3_KEY: &str = "service:rustfs";
pub const S3_API_PORT: u16 = 9000;
pub const S3_CONSOLE_PORT: u16 = 9001;
pub const S3_ACCESS: &str = "larafim";

pub fn s3_running(app: &AppHandle) -> bool {
    app.state::<ServerState>().dev.is_running(S3_KEY)
}

pub fn s3_url(app: &AppHandle) -> String {
    // O painel do RustFS fica em /rustfs/console/ (a raiz é a API S3).
    let c = app.state::<AppState>().config.lock().unwrap().clone();
    if c.http_port == 80 {
        format!("http://s3.{}/rustfs/console/", c.tld)
    } else {
        format!("http://s3.{}:{}/rustfs/console/", c.tld, c.http_port)
    }
}

/// 32 caracteres aleatórios (só para o storage local).
fn random_secret() -> String {
    random_token(b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789", 32)
}

/// `len` caracteres aleatórios de `alphabet` (credenciais de serviços locais).
pub(crate) fn random_token(alphabet: &[u8], len: usize) -> String {
    let mut out = String::new();
    while out.len() < len {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        let mut n = h.finish();
        for _ in 0..8 {
            out.push(alphabet[(n % alphabet.len() as u64) as usize] as char);
            n /= alphabet.len() as u64;
        }
    }
    out.truncate(len);
    out
}

/// Chave secreta (criada e guardada com DPAPI na primeira vez).
pub fn secret(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut c = state.config.lock().unwrap();
    if let Some(enc) = &c.s3_secret
        && let Ok(plain) = crate::secret::unprotect(enc)
    {
        return Ok(plain);
    }
    let plain = random_secret();
    c.s3_secret = Some(crate::secret::protect(&plain)?);
    c.save(&state.paths.config_file()).map_err(|e| e.to_string())?;
    Ok(plain)
}

#[derive(Serialize)]
pub struct S3Status {
    installed: bool,
    version: Option<String>,
    running: bool,
    installing: bool,
    console_url: String,
    endpoint: String,
    access_key: String,
}

static INSTALLING: Mutex<bool> = Mutex::new(false);

#[tauri::command]
pub fn s3_status(app: AppHandle) -> S3Status {
    let paths = app.state::<AppState>().paths.clone();
    S3Status {
        installed: s3::rustfs_installed(&paths),
        version: std::fs::read_to_string(s3::rustfs_dir(&paths).join("VERSION")).ok().map(|v| v.trim().to_string()),
        running: s3_running(&app),
        installing: *INSTALLING.lock().unwrap(),
        console_url: s3_url(&app),
        endpoint: format!("http://127.0.0.1:{S3_API_PORT}"),
        access_key: S3_ACCESS.into(),
    }
}

#[derive(Clone, Serialize)]
struct Progress {
    received: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn s3_install(app: AppHandle) -> CmdResult<()> {
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
    let last = Mutex::new(Instant::now() - Duration::from_secs(1));
    let on_progress = move |received: u64, total: Option<u64>| {
        let mut l = last.lock().unwrap();
        if l.elapsed() >= Duration::from_millis(100) {
            *l = Instant::now();
            let _ = emitter.emit("s3:progress", Progress { received, total });
        }
    };
    let result = async {
        let client = http_client().map_err(|e| e.to_string())?;
        s3::install_rustfs(&client, &paths, &on_progress).await.map_err(|e| e.to_string())
    }
    .await;
    *INSTALLING.lock().unwrap() = false;
    let _ = app.emit("services-changed", ());
    result.map(|_| ())
}

/// Liga o RustFS (dados em data\s3) e espera a API responder.
pub async fn start_s3(app: &AppHandle) -> Result<(), String> {
    if s3_running(app) {
        return Ok(());
    }
    let paths = app.state::<AppState>().paths.clone();
    if !s3::rustfs_installed(&paths) {
        return Err(t!("o storage S3 não está instalado (Configurações › Serviços)"));
    }
    for port in [S3_API_PORT, S3_CONSOLE_PORT] {
        if let Err(owner) = larafim_server::port::check_free(port) {
            let who = owner.map(|o| o.name).unwrap_or_else(|| t!("outro programa"));
            return Err(t!("a porta {port} (storage S3) está ocupada por {who}", port = port, who = who));
        }
    }
    let data = paths.data_dir().join("s3");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let exe = s3::rustfs_exe(&paths);
    // Endereços e painel vão por variáveis de ambiente: na linha de comando,
    // um "true" depois de --console-enable vira uma segunda pasta de dados.
    let command = format!("\"{}\" server \"{}\"", exe.display(), data.display());
    app.state::<ServerState>()
        .dev
        .start(DevSpec {
            key: S3_KEY.into(),
            cwd: s3::rustfs_dir(&paths),
            command,
            node_dir: s3::rustfs_dir(&paths),
            env: vec![
                ("PORT".into(), S3_CONSOLE_PORT.to_string()),
                ("RUSTFS_ADDRESS".into(), format!("127.0.0.1:{S3_API_PORT}")),
                ("RUSTFS_CONSOLE_ENABLE".into(), "true".into()),
                ("RUSTFS_CONSOLE_ADDRESS".into(), format!("127.0.0.1:{S3_CONSOLE_PORT}")),
                ("RUSTFS_ACCESS_KEY".into(), S3_ACCESS.into()),
                ("RUSTFS_SECRET_KEY".into(), secret(app)?),
            ],
        })
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    while tokio::net::TcpStream::connect(("127.0.0.1", S3_API_PORT)).await.is_err() {
        if !s3_running(app) || started.elapsed() > Duration::from_secs(30) {
            let log = app.state::<ServerState>().dev.logs(S3_KEY).join(" | ");
            app.state::<ServerState>().dev.stop(S3_KEY);
            return Err(t!("o storage S3 não iniciou: {a}", a = log.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    crate::server::after_change(app, false);
    let _ = app.emit("services-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn s3_start(app: AppHandle) -> CmdResult<()> {
    start_s3(&app).await
}

#[tauri::command]
pub async fn s3_stop(app: AppHandle) {
    app.state::<ServerState>().dev.stop(S3_KEY);
    crate::server::after_change(&app, false);
    let _ = app.emit("services-changed", ());
}

#[tauri::command]
pub async fn open_s3(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    start_s3(&app).await?;
    crate::server::ensure_hosts(&app).await.map_err(|e| e.message)?;
    crate::server::refresh(&app).await.map_err(|e| e.message)?;
    app.opener().open_url(s3_url(&app), None::<&str>).map_err(|e| e.to_string())
}

// ---------- Storage do projeto ----------

fn project_domain(app: &AppHandle, path: &Path) -> Result<String, String> {
    app.state::<AppState>()
        .projects
        .lock()
        .unwrap()
        .find(path)
        .map(|p| p.domain.clone())
        .ok_or_else(|| t!("importe o projeto antes"))
}

pub(crate) fn changes(bucket: &str, secret: &str) -> Vec<EnvChange> {
    vec![
        EnvChange::set("FILESYSTEM_DISK", "s3"),
        EnvChange::set("AWS_ACCESS_KEY_ID", S3_ACCESS),
        EnvChange::set("AWS_SECRET_ACCESS_KEY", secret),
        EnvChange::set("AWS_DEFAULT_REGION", "us-east-1"),
        EnvChange::set("AWS_BUCKET", bucket),
        EnvChange::set("AWS_ENDPOINT", &format!("http://127.0.0.1:{S3_API_PORT}")),
        EnvChange::set("AWS_USE_PATH_STYLE_ENDPOINT", "true"),
    ]
}

/// O Laravel precisa deste pacote para o disco "s3".
fn has_s3_package(path: &Path) -> bool {
    path.join("vendor").join("league").join("flysystem-aws-s3-v3").is_dir()
}

#[derive(Serialize)]
pub struct StoragePreview {
    diff: Vec<DiffLine>,
    bucket: String,
    /// Projeto Laravel sem `league/flysystem-aws-s3-v3` instalado.
    needs_package: bool,
}

#[tauri::command]
pub fn project_storage_preview(app: AppHandle, path: PathBuf) -> CmdResult<StoragePreview> {
    let bucket = s3::bucket_name(&project_domain(&app, &path)?);
    let base = std::fs::read_to_string(envfile::env_path(&path)).unwrap_or_default();
    let (_, mut diff) = envfile::apply(&base, &changes(&bucket, "segredo"));
    for d in diff.iter_mut().filter(|d| d.key == "AWS_SECRET_ACCESS_KEY") {
        for v in [&mut d.before, &mut d.after].into_iter().flatten() {
            if !v.is_empty() {
                *v = "••••••".into();
            }
        }
    }
    Ok(StoragePreview { diff, bucket, needs_package: path.join("artisan").is_file() && !has_s3_package(&path) })
}

/// Liga o projeto ao S3 local (cria o bucket e ajusta o .env) ou desvincula.
#[tauri::command]
pub async fn project_storage_apply(app: AppHandle, path: PathBuf, kind: String) -> CmdResult<()> {
    if kind == "s3" {
        start_s3(&app).await?;
        let bucket = s3::bucket_name(&project_domain(&app, &path)?);
        let secret = secret(&app)?;
        let client = http_client().map_err(|e| e.to_string())?;
        s3::create_bucket(&client, S3_API_PORT, S3_ACCESS, &secret, &bucket).await.map_err(|e| e.to_string())?;
        if envfile::env_path(&path).exists() {
            envfile::write(&path, &changes(&bucket, &secret)).map_err(|e| e.to_string())?;
            crate::tools::env_changed(&app, &path);
        }
    } else if kind != "none" {
        return Err(t!("storage desconhecido: {kind}", kind = kind));
    }
    crate::tools::update_project(&app, &path, |p| p.storage = (kind == "s3").then_some(kind.clone()))?;
    tracing::info!(path = %path.display(), kind, "storage do projeto alterado");
    Ok(())
}
