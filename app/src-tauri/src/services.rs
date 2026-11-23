//! Serviços (fase 6): MariaDB sob demanda, bancos por projeto, `.env`,
//! backups e Adminer em `db.<domínio>`.

use larafim_core::t;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use larafim_core::envfile::{self, DiffLine, EnvChange};
use larafim_core::store::same_path;
use larafim_runtimes::mariadb::{self, MariaEntry};
use larafim_runtimes::{extras, http_client};
use larafim_server::mariadb::{MariaError, MariaService, MariaSetup, MariaStatus, database_name};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

type CmdResult<T> = Result<T, String>;

pub struct ServicesState {
    /// MariaDB compartilhado (um servidor, um banco por projeto).
    pub maria: Arc<MariaService>,
    /// MariaDB próprio de cada projeto que pediu isolamento (chave = pasta).
    own: Mutex<HashMap<String, Arc<MariaService>>>,
    catalog: Mutex<Option<(Instant, Vec<MariaEntry>)>>,
    installing: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Default for ServicesState {
    fn default() -> Self {
        Self {
            maria: Arc::new(MariaService::default()),
            own: Mutex::new(HashMap::new()),
            catalog: Mutex::new(None),
            installing: Mutex::new(HashMap::new()),
        }
    }
}

// ---------- MariaDB próprio do projeto ----------

/// Versão e porta escolhidas para o servidor próprio de um projeto.
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct OwnDb {
    pub version: String,
    pub port: u16,
}

fn own_key(path: &Path) -> String {
    path.display().to_string().to_lowercase()
}

/// Pasta de dados estável por projeto (não muda se o domínio mudar).
fn own_data_dir(app: &AppHandle, path: &Path, major: &str) -> PathBuf {
    // FNV-1a do caminho em minúsculas: curto e sempre igual para a mesma pasta.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in own_key(path).bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).take(24).collect();
    app.state::<AppState>().paths.data_dir().join("mariadb-projetos").join(format!("{name}-{h:016x}-{major}"))
}

pub(crate) fn own_setup(app: &AppHandle, path: &Path, own: &OwnDb) -> Result<MariaSetup, String> {
    let installed = mariadb::list_installed(&app.state::<AppState>().paths.mariadb_dir());
    let pick = installed
        .iter()
        .find(|m| m.major == own.version)
        .ok_or_else(|| t!("o MariaDB {a} não está instalado (Configurações › Serviços)", a = own.version))?;
    Ok(MariaSetup {
        version: pick.version.clone(),
        install_dir: pick.dir.clone(),
        data_dir: own_data_dir(app, path, &pick.major),
        port: own.port,
    })
}

fn own_service(app: &AppHandle, path: &Path) -> Arc<MariaService> {
    app.state::<ServicesState>()
        .own
        .lock()
        .unwrap()
        .entry(own_key(path))
        .or_insert_with(|| Arc::new(MariaService::default()))
        .clone()
}

pub(crate) fn own_of(cfg: &larafim_core::ProjectConfig) -> Option<OwnDb> {
    Some(OwnDb { version: cfg.db_version.clone()?, port: cfg.db_port? })
}

/// Liga o MariaDB próprio de um projeto.
pub async fn start_own(app: &AppHandle, path: &Path) -> Result<(), String> {
    let cfg = project_config(app, path)?;
    let own = own_of(&cfg).ok_or_else(|| t!("o projeto não tem MariaDB próprio configurado"))?;
    let setup = own_setup(app, path, &own)?;
    let r = own_service(app, path).start(&setup).await.map_err(maria_err);
    let _ = app.emit("services-changed", ());
    if r.is_ok() {
        refresh_adminer(app);
        crate::server::after_change(app, false);
    }
    r
}

pub async fn stop_own(app: &AppHandle, path: &Path) {
    let svc = app.state::<ServicesState>().own.lock().unwrap().get(&own_key(path)).cloned();
    if let Some(svc) = svc {
        svc.stop().await;
    }
    let _ = app.emit("services-changed", ());
}

/// Ao sair do Larafim: desliga todos os servidores próprios.
pub async fn stop_all_own(app: &AppHandle) {
    let all: Vec<Arc<MariaService>> = app.state::<ServicesState>().own.lock().unwrap().values().cloned().collect();
    for s in all {
        s.stop().await;
    }
}

/// Algum servidor próprio rodando? (o gateway fica ligado para o Adminer)
pub async fn any_own_running(app: &AppHandle) -> bool {
    let all: Vec<Arc<MariaService>> = app.state::<ServicesState>().own.lock().unwrap().values().cloned().collect();
    for s in all {
        if s.status().await.running {
            return true;
        }
    }
    false
}

/// Portas já reservadas por outros projetos (servidor próprio) e pelo compartilhado.
fn reserved_ports(app: &AppHandle, except: Option<&Path>) -> Vec<u16> {
    let state = app.state::<AppState>();
    let mut ports = vec![state.config.lock().unwrap().db_port];
    ports.extend(
        state
            .projects
            .lock()
            .unwrap()
            .projects
            .iter()
            .filter(|p| p.database.as_deref() == Some("mariadb-own") && except.is_none_or(|e| !same_path(&p.path, e)))
            .filter_map(|p| p.db_port),
    );
    ports
}

/// Reescreve a entrada do Adminer com as portas dos MariaDB do Larafim.
pub fn refresh_adminer(app: &AppHandle) {
    let ports = reserved_ports(app, None);
    let pg = crate::pg::pg_port(app);
    let _ = extras::write_adminer_index(&app.state::<AppState>().paths, &ports, &[pg]);
}

/// Portas de todos os MariaDB do Larafim (compartilhado primeiro).
pub fn maria_ports(app: &AppHandle) -> Vec<u16> {
    reserved_ports(app, None)
}

/// Primeira porta livre a partir da 3310 para um servidor próprio.
#[tauri::command]
pub fn suggest_own_port(app: AppHandle, path: PathBuf) -> u16 {
    let reserved = reserved_ports(&app, Some(&path));
    (3310..3400)
        .find(|p| !reserved.contains(p) && larafim_server::port::check_free(*p).is_ok())
        .unwrap_or(3399)
}

#[tauri::command]
pub async fn own_start(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    start_own(&app, &path).await
}

#[tauri::command]
pub async fn own_stop(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    stop_own(&app, &path).await;
    crate::server::after_change(&app, false);
    Ok(())
}

#[derive(Serialize)]
pub struct OwnStatus {
    project: String,
    path: PathBuf,
    version: String,
    port: u16,
    running: bool,
}

/// Instalação e dados do MariaDB conforme as configurações.
pub fn maria_setup(app: &AppHandle) -> Result<MariaSetup, String> {
    let state = app.state::<AppState>();
    let (version, port) = {
        let c = state.config.lock().unwrap();
        (c.db_version.clone(), c.db_port)
    };
    let installed = mariadb::list_installed(&state.paths.mariadb_dir());
    let pick = match &version {
        Some(major) => installed.iter().find(|m| &m.major == major),
        None => installed.first(),
    }
    .ok_or_else(|| t!("o MariaDB não está instalado (Configurações › Serviços)"))?;
    Ok(MariaSetup {
        version: pick.version.clone(),
        install_dir: pick.dir.clone(),
        data_dir: state.paths.data_dir().join("mariadb").join(&pick.major),
        port,
    })
}

fn maria_err(e: MariaError) -> String {
    match e {
        MariaError::PortBusy(p) => {
            let who = larafim_server::port::listener_on(p)
                .map(|o| match o.hint {
                    Some(h) => format!("{} ({h})", o.name),
                    None => o.name,
                })
                .unwrap_or_else(|| t!("outro programa"));
            t!("A porta {p} já está em uso por {who}. Escolha outra porta para o MariaDB (ex.: {a}).", p = p, who = who, a = p + 1)
        }
        MariaError::Failed(m) => m,
    }
}

/// Liga o MariaDB (usado também ao deixar um projeto online).
pub async fn start_maria(app: &AppHandle) -> Result<(), String> {
    let setup = maria_setup(app)?;
    let svc = app.state::<ServicesState>().maria.clone();
    let r = svc.start(&setup).await.map_err(maria_err);
    let _ = app.emit("services-changed", ());
    if r.is_ok() {
        // O Adminer acompanha a porta atual.
        refresh_adminer(app);
        crate::server::after_change(app, false);
    }
    r
}

#[derive(Serialize)]
pub struct MariaRow {
    major: String,
    latest: Option<String>,
    installed: Option<String>,
    eol: Option<String>,
    update_available: bool,
    installing: bool,
}

#[derive(Serialize)]
pub struct ServicesOverview {
    rows: Vec<MariaRow>,
    catalog_error: Option<String>,
    status: MariaStatus,
    port: u16,
    selected: Option<String>,
    databases: Vec<String>,
    adminer: bool,
    adminer_url: String,
    /// Servidores próprios de projetos.
    own: Vec<OwnStatus>,
}

async fn catalog(state: &ServicesState, force: bool) -> Result<Vec<MariaEntry>, String> {
    if !force
        && let Some((at, c)) = state.catalog.lock().unwrap().as_ref()
        && at.elapsed() < Duration::from_secs(30 * 60)
    {
        return Ok(c.clone());
    }
    let client = http_client().map_err(|e| e.to_string())?;
    let c = mariadb::fetch_catalog(&client).await.map_err(|e| e.to_string())?;
    *state.catalog.lock().unwrap() = Some((Instant::now(), c.clone()));
    Ok(c)
}

pub fn adminer_url(app: &AppHandle) -> String {
    let c = app.state::<AppState>().config.lock().unwrap().clone();
    if c.http_port == 80 { format!("http://db.{}", c.tld) } else { format!("http://db.{}:{}", c.tld, c.http_port) }
}

#[tauri::command]
pub async fn services_overview(app: AppHandle, refresh: Option<bool>) -> CmdResult<ServicesOverview> {
    let state = app.state::<ServicesState>();
    let paths = app.state::<AppState>().paths.clone();
    let cat = catalog(&state, refresh.unwrap_or(false)).await;
    let installed = mariadb::list_installed(&paths.mariadb_dir());
    let (entries, catalog_error) = match cat {
        Ok(c) => (c, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let mut majors: Vec<String> = entries.iter().map(|e| e.major.clone()).collect();
    for m in &installed {
        if !majors.contains(&m.major) {
            majors.push(m.major.clone());
        }
    }
    let installing = state.installing.lock().unwrap().clone();
    let rows = majors
        .into_iter()
        .map(|major| {
            let inst = installed.iter().find(|m| m.major == major).map(|m| m.version.clone());
            let e = entries.iter().find(|e| e.major == major);
            MariaRow {
                update_available: match (&inst, e) {
                    (Some(i), Some(e)) => semver::Version::parse(&e.version).ok() > semver::Version::parse(i).ok(),
                    _ => false,
                },
                latest: e.map(|e| e.version.clone()),
                eol: e.and_then(|e| e.eol.clone()),
                installing: installing.contains_key(&major),
                installed: inst,
                major,
            }
        })
        .collect();
    let status = state.maria.status().await;
    let databases = if status.running { state.maria.databases().await.unwrap_or_default() } else { Vec::new() };
    let app_state = app.state::<AppState>();
    let (port, selected) = {
        let c = app_state.config.lock().unwrap();
        (c.db_port, c.db_version.clone())
    };
    let own_cfgs: Vec<larafim_core::ProjectConfig> = app_state
        .projects
        .lock()
        .unwrap()
        .projects
        .iter()
        .filter(|p| p.database.as_deref() == Some("mariadb-own"))
        .cloned()
        .collect();
    let mut own = Vec::new();
    for p in own_cfgs {
        let svc = state.own.lock().unwrap().get(&own_key(&p.path)).cloned();
        let running = match svc {
            Some(s) => s.status().await.running,
            None => false,
        };
        own.push(OwnStatus {
            project: p.domain.clone(),
            version: p.db_version.clone().unwrap_or_default(),
            port: p.db_port.unwrap_or_default(),
            path: p.path.clone(),
            running,
        });
    }
    Ok(ServicesOverview {
        rows,
        catalog_error,
        status,
        port,
        selected,
        databases,
        adminer: extras::adminer_installed(&paths),
        adminer_url: adminer_url(&app),
        own,
    })
}

#[derive(Clone, Serialize)]
struct Progress {
    major: String,
    received: u64,
    total: Option<u64>,
}

#[derive(Clone, Serialize)]
struct Done {
    major: String,
    ok: bool,
    error: Option<String>,
}

#[tauri::command]
pub async fn maria_install(app: AppHandle, major: String) -> CmdResult<()> {
    let state = app.state::<ServicesState>();
    let entry = catalog(&state, false)
        .await?
        .into_iter()
        .find(|e| e.major == major)
        .ok_or_else(|| t!("o MariaDB {major} não está no catálogo", major = major))?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut i = state.installing.lock().unwrap();
        if i.contains_key(&major) {
            return Err(t!("já está instalando"));
        }
        i.insert(major.clone(), cancel.clone());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let paths = app.state::<AppState>().paths.clone();
        let port = app.state::<AppState>().config.lock().unwrap().db_port;
        let emitter = app.clone();
        let m = major.clone();
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let on_progress = move |received: u64, total: Option<u64>| {
            let mut l = last.lock().unwrap();
            if l.elapsed() >= Duration::from_millis(100) {
                *l = Instant::now();
                let _ = emitter.emit("maria:progress", Progress { major: m.clone(), received, total });
            }
        };
        let result = async {
            let client = http_client()?;
            mariadb::install(&client, &paths, &entry, &cancel, &on_progress).await?;
            // O Adminer vem junto (pequeno; falha não impede o banco).
            let pg = crate::pg::pg_port(&app);
            if let Err(e) = extras::ensure_adminer(&client, &paths, &[port], &[pg]).await {
                tracing::warn!("Adminer não baixado: {e}");
            }
            Ok::<(), larafim_runtimes::Error>(())
        }
        .await;
        app.state::<ServicesState>().installing.lock().unwrap().remove(&major);
        let _ = app.emit("maria:done", Done { major, ok: result.is_ok(), error: result.err().map(|e| e.to_string()) });
        let _ = app.emit("services-changed", ());
    });
    Ok(())
}

#[tauri::command]
pub async fn maria_remove(app: AppHandle, major: String) -> CmdResult<()> {
    let st = app.state::<ServicesState>().maria.status().await;
    if st.running && st.version.as_deref().is_some_and(|v| v.starts_with(&format!("{major}."))) {
        return Err(t!("desligue o MariaDB antes de remover esta versão"));
    }
    let paths = app.state::<AppState>().paths.clone();
    for m in mariadb::list_installed(&paths.mariadb_dir()).into_iter().filter(|m| m.major == major) {
        std::fs::remove_dir_all(&m.dir).map_err(|e| e.to_string())?;
    }
    // Os dados (data\mariadb\<linha>) ficam: reinstalar recupera os bancos.
    let _ = app.emit("services-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn maria_start(app: AppHandle) -> CmdResult<()> {
    start_maria(&app).await
}

#[tauri::command]
pub async fn maria_stop(app: AppHandle) -> CmdResult<()> {
    app.state::<ServicesState>().maria.stop().await;
    let _ = app.emit("services-changed", ());
    crate::server::after_change(&app, false);
    Ok(())
}

/// Troca versão e/ou porta; se estiver rodando, reinicia com a nova configuração.
#[tauri::command]
pub async fn maria_configure(app: AppHandle, version: Option<String>, port: u16) -> CmdResult<()> {
    if port < 1024 {
        return Err(t!("use uma porta acima de 1024 (ex.: 3306 ou 3307)"));
    }
    {
        let state = app.state::<AppState>();
        let mut c = state.config.lock().unwrap();
        c.db_version = version;
        c.db_port = port;
        c.save(&state.paths.config_file()).map_err(|e| e.to_string())?;
        let _ = app.emit("config-changed", &*c);
    }
    if app.state::<ServicesState>().maria.status().await.running {
        start_maria(&app).await?;
    }
    refresh_adminer(&app);
    let _ = app.emit("services-changed", ());
    Ok(())
}

// ---------- Banco por projeto ----------

pub(crate) fn project_config(app: &AppHandle, path: &Path) -> Result<larafim_core::ProjectConfig, String> {
    app.state::<AppState>()
        .projects
        .lock()
        .unwrap()
        .find(path)
        .cloned()
        .ok_or_else(|| t!("importe o projeto antes"))
}

fn save_project_cfg(app: &AppHandle, cfg: &larafim_core::ProjectConfig) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut store = state.projects.lock().unwrap();
    if let Some(p) = store.projects.iter_mut().find(|p| same_path(&p.path, &cfg.path)) {
        *p = cfg.clone();
    }
    store.save(&state.paths.projects_file()).map_err(|e| e.to_string())
}

/// Laravel 11+ tem a conexão "mariadb"; versões antigas usam "mysql".
fn laravel_connection(project: &Path) -> &'static str {
    let major = std::fs::read_to_string(project.join("composer.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|j| j.pointer("/require/laravel~1framework").and_then(|v| v.as_str()).map(String::from))
        .and_then(|c| c.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok());
    match major {
        Some(m) if m >= 11 => "mariadb",
        Some(_) => "mysql",
        None => "mariadb",
    }
}

fn changes_for(app: &AppHandle, project: &Path, domain: &str, kind: &str, own: Option<&OwnDb>) -> Result<Vec<EnvChange>, String> {
    if kind == "mariadb-own" {
        let own = own.ok_or_else(|| t!("escolha a versão e a porta do MariaDB próprio"))?;
        return Ok(vec![
            EnvChange::set("DB_CONNECTION", laravel_connection(project)),
            EnvChange::set("DB_HOST", "127.0.0.1"),
            EnvChange::set("DB_PORT", &own.port.to_string()),
            EnvChange::set("DB_DATABASE", &database_name(domain)),
            EnvChange::set("DB_USERNAME", "root"),
            EnvChange::set("DB_PASSWORD", ""),
        ]);
    }
    if let Some(id) = kind.strip_prefix("external:") {
        let c = crate::dbconn::find(app, id)?;
        let connection = if c.kind == "mysql" { "mysql" } else { laravel_connection(project) };
        return Ok(vec![
            EnvChange::set("DB_CONNECTION", connection),
            EnvChange::set("DB_HOST", &c.host),
            EnvChange::set("DB_PORT", &c.port.to_string()),
            EnvChange::set("DB_DATABASE", &database_name(domain)),
            EnvChange::set("DB_USERNAME", &c.user),
            EnvChange::set("DB_PASSWORD", &crate::secret::unprotect(&c.password_protected)?),
        ]);
    }
    Ok(match kind {
        "postgres" => vec![
            EnvChange::set("DB_CONNECTION", "pgsql"),
            EnvChange::set("DB_HOST", "127.0.0.1"),
            EnvChange::set("DB_PORT", &crate::pg::pg_port(app).to_string()),
            EnvChange::set("DB_DATABASE", &database_name(domain)),
            EnvChange::set("DB_USERNAME", "postgres"),
            EnvChange::set("DB_PASSWORD", ""),
        ],
        "mariadb" => {
            let port = app.state::<AppState>().config.lock().unwrap().db_port;
            vec![
                EnvChange::set("DB_CONNECTION", laravel_connection(project)),
                EnvChange::set("DB_HOST", "127.0.0.1"),
                EnvChange::set("DB_PORT", &port.to_string()),
                EnvChange::set("DB_DATABASE", &database_name(domain)),
                EnvChange::set("DB_USERNAME", "root"),
                EnvChange::set("DB_PASSWORD", ""),
            ]
        }
        "sqlite" => vec![
            EnvChange::set("DB_CONNECTION", "sqlite"),
            EnvChange::unset("DB_HOST"),
            EnvChange::unset("DB_PORT"),
            EnvChange::unset("DB_DATABASE"),
            EnvChange::unset("DB_USERNAME"),
            EnvChange::unset("DB_PASSWORD"),
        ],
        _ => Vec::new(),
    })
}

/// Cliente do banco de um projeto (MariaDB do Larafim ou conexão externa).
async fn client_for_project(
    app: &AppHandle,
    cfg: &larafim_core::ProjectConfig,
) -> Result<larafim_server::dbclient::DbClient, String> {
    match cfg.database.as_deref() {
        Some("mariadb-own") => {
            start_own(app, &cfg.path).await?;
            own_service(app, &cfg.path).client().await.map_err(maria_err)
        }
        Some("mariadb") => {
            start_maria(app).await?;
            app.state::<ServicesState>().maria.client().await.map_err(maria_err)
        }
        Some(k) if k.starts_with("external:") => crate::dbconn::client_for(app, &crate::dbconn::find(app, &k[9..])?),
        _ => Err(t!("o projeto não usa MariaDB nem um banco existente")),
    }
}

#[derive(Serialize)]
pub struct DbPreview {
    diff: Vec<DiffLine>,
    database: Option<String>,
    /// Não há .env: será criado a partir do .env.example.
    from_example: bool,
    /// Não há .env nem .env.example: nada será escrito.
    no_env: bool,
}

#[tauri::command]
pub fn project_db_preview(app: AppHandle, path: PathBuf, kind: String, own: Option<OwnDb>) -> CmdResult<DbPreview> {
    let cfg = project_config(&app, &path)?;
    let env = envfile::env_path(&path);
    let example = path.join(".env.example");
    let base = std::fs::read_to_string(&env).or_else(|_| std::fs::read_to_string(&example)).unwrap_or_default();
    let (_, mut diff) = envfile::apply(&base, &changes_for(&app, &path, &cfg.domain, &kind, own.as_ref())?);
    // A senha não aparece na tela.
    for d in diff.iter_mut().filter(|d| d.key == "DB_PASSWORD") {
        for v in [&mut d.before, &mut d.after].into_iter().flatten() {
            if !v.is_empty() {
                *v = "••••••".into();
            }
        }
    }
    Ok(DbPreview {
        diff,
        database: (kind.starts_with("mariadb") || kind.starts_with("external:") || kind == "postgres")
            .then(|| database_name(&cfg.domain)),
        from_example: !env.exists() && example.exists(),
        no_env: !env.exists() && !example.exists(),
    })
}

/// Liga o projeto ao banco: cria o banco (MariaDB) ou o arquivo (SQLite) e
/// ajusta o `.env` (com cópia de segurança). `kind = "none"` só desvincula.
#[tauri::command]
pub async fn project_db_apply(app: AppHandle, path: PathBuf, kind: String, own: Option<OwnDb>) -> CmdResult<()> {
    let mut cfg = project_config(&app, &path)?;
    let env = envfile::env_path(&path);
    // Saindo do servidor próprio: desliga ele (os dados ficam guardados).
    if cfg.database.as_deref() == Some("mariadb-own") && kind != "mariadb-own" {
        stop_own(&app, &path).await;
    }
    match kind.as_str() {
        "mariadb-own" => {
            let own = own.clone().ok_or_else(|| t!("escolha a versão e a porta do MariaDB próprio"))?;
            if own.port < 1024 || reserved_ports(&app, Some(&path)).contains(&own.port) {
                return Err(t!("a porta {a} já é usada por outro banco do Larafim", a = own.port));
            }
            // Versão/porta mudaram: o servidor antigo do projeto é desligado.
            if own_of(&cfg).is_some_and(|o| o.version != own.version || o.port != own.port) {
                stop_own(&app, &path).await;
            }
            cfg.database = Some(kind.clone());
            cfg.db_version = Some(own.version.clone());
            cfg.db_port = Some(own.port);
            save_project_cfg(&app, &cfg)?;
            start_own(&app, &path).await?;
            own_service(&app, &path).create_database(&database_name(&cfg.domain)).await.map_err(maria_err)?;
        }
        "mariadb" => {
            start_maria(&app).await?;
            app.state::<ServicesState>()
                .maria
                .create_database(&database_name(&cfg.domain))
                .await
                .map_err(maria_err)?;
        }
        "postgres" => {
            crate::pg::client(&app).await?.create_database(&database_name(&cfg.domain)).await?;
            crate::pg::enable_for_project(&app, &path).await;
        }
        "sqlite" => {
            let file = path.join("database").join("database.sqlite");
            if !file.exists() {
                std::fs::create_dir_all(path.join("database")).map_err(|e| e.to_string())?;
                std::fs::write(&file, b"").map_err(|e| e.to_string())?;
            }
        }
        "none" => {}
        k if k.starts_with("external:") => {
            let probe = larafim_core::ProjectConfig { database: Some(k.to_string()), ..cfg.clone() };
            client_for_project(&app, &probe)
                .await?
                .create_database(&database_name(&cfg.domain))
                .await
                .map_err(|e| t!("não foi possível criar o banco no servidor: {e}", e = e))?;
        }
        other => return Err(t!("banco desconhecido: {other}", other = other)),
    }
    if kind != "none" {
        if !env.exists() && path.join(".env.example").exists() {
            std::fs::copy(path.join(".env.example"), &env).map_err(|e| e.to_string())?;
        }
        if env.exists() {
            envfile::write(&path, &changes_for(&app, &path, &cfg.domain, &kind, own.as_ref())?).map_err(|e| e.to_string())?;
            crate::tools::env_changed(&app, &path);
        }
    }
    {
        let state = app.state::<AppState>();
        let mut store = state.projects.lock().unwrap();
        if let Some(p) = store.projects.iter_mut().find(|p| same_path(&p.path, &path)) {
            p.database = (kind != "none").then_some(kind.clone());
            if kind != "mariadb-own" {
                p.db_version = None;
                p.db_port = None;
            }
        }
        store.save(&state.paths.projects_file()).map_err(|e| e.to_string())?;
        let roots = state.config.lock().unwrap().roots.clone();
        let _ = app.emit("projects-changed", serde_json::json!({ "projects": store.views(&roots), "ignored": store.ignored }));
    }
    tracing::info!(path = %path.display(), kind, "banco do projeto alterado");
    Ok(())
}

/// Usado ao criar um projeto novo: liga ao banco sem diálogo.
pub async fn apply_db(app: &AppHandle, path: &Path, kind: &str) -> Result<(), String> {
    project_db_apply(app.clone(), path.to_path_buf(), kind.to_string(), None).await
}

#[tauri::command]
pub fn project_env_undo(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    envfile::undo(&path).map_err(|e| e.to_string())?;
    crate::tools::env_changed(&app, &path);
    Ok(())
}

#[tauri::command]
pub fn project_env_has_backup(path: PathBuf) -> bool {
    envfile::has_backup(&path)
}

// ---------- Backups ----------

#[derive(Serialize)]
pub struct Backup {
    file: PathBuf,
    name: String,
    size: u64,
    modified: u64,
}

fn backup_dir(app: &AppHandle, domain: &str) -> PathBuf {
    app.state::<AppState>().paths.backups_dir().join(domain)
}

#[tauri::command]
pub fn db_backups(app: AppHandle, path: PathBuf) -> CmdResult<Vec<Backup>> {
    let cfg = project_config(&app, &path)?;
    let dir = backup_dir(&app, &cfg.domain);
    let mut list: Vec<Backup> = std::fs::read_dir(&dir)
        .map(|r| {
            r.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    Some(Backup {
                        name: e.file_name().to_string_lossy().to_string(),
                        file: e.path(),
                        size: m.len(),
                        modified: m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|b| std::cmp::Reverse(b.modified));
    Ok(list)
}

#[tauri::command]
pub async fn db_backup(app: AppHandle, path: PathBuf) -> CmdResult<PathBuf> {
    let cfg = project_config(&app, &path)?;
    let db = database_name(&cfg.domain);
    let stamp = chrono_like_stamp();
    let file = backup_dir(&app, &cfg.domain).join(format!("{db}-{stamp}.sql"));
    if cfg.database.as_deref() == Some("postgres") {
        std::fs::create_dir_all(backup_dir(&app, &cfg.domain)).map_err(|e| e.to_string())?;
        crate::pg::client(&app).await?.dump(&db, &file).await?;
    } else {
        client_for_project(&app, &cfg).await?.dump(&db, &file).await?;
    }
    tracing::info!(file = %file.display(), "backup criado");
    Ok(file)
}

/// Restaura um backup no banco do projeto (substitui as tabelas que o arquivo tiver).
#[tauri::command]
pub async fn db_restore(app: AppHandle, path: PathBuf, file: PathBuf) -> CmdResult<()> {
    let cfg = project_config(&app, &path)?;
    // Só arquivos da pasta de backups deste projeto.
    let dir = backup_dir(&app, &cfg.domain);
    if file.parent().is_none_or(|p| !same_path(p, &dir)) {
        return Err(t!("arquivo de backup fora da pasta do projeto"));
    }
    if cfg.database.as_deref() == Some("postgres") {
        return crate::pg::client(&app).await?.restore(&database_name(&cfg.domain), &file).await;
    }
    client_for_project(&app, &cfg)
        .await?
        .restore(&database_name(&cfg.domain), &file)
        .await
}

#[tauri::command]
pub fn open_backups_folder(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let cfg = project_config(&app, &path)?;
    let dir = backup_dir(&app, &cfg.domain);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

/// Data/hora local para nome de arquivo (AAAAMMDD-HHMMSS).
fn chrono_like_stamp() -> String {
    // SAFETY: GetLocalTime só preenche a estrutura.
    let t = unsafe {
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Abre o Adminer (liga o MariaDB e o gateway; pode pedir UAC para o hosts).
#[tauri::command]
pub async fn open_adminer(
    app: AppHandle,
    database: Option<String>,
    connection: Option<String>,
    project: Option<PathBuf>,
) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let paths = app.state::<AppState>().paths.clone();
    if !extras::adminer_installed(&paths) {
        let client = http_client().map_err(|e| e.to_string())?;
        let port = app.state::<AppState>().config.lock().unwrap().db_port;
        let pg = crate::pg::pg_port(&app);
        extras::ensure_adminer(&client, &paths, &[port], &[pg]).await.map_err(|e| e.to_string())?;
        refresh_adminer(&app);
    }
    // Servidor externo: o Adminer abre com servidor e usuário preenchidos
    // (a senha é digitada lá). O do Larafim entra sozinho.
    // "postgres" = o PostgreSQL do Larafim.
    let postgres = connection.as_deref() == Some("postgres");
    let external = match connection.filter(|c| c != "postgres") {
        Some(id) => Some(crate::dbconn::find(&app, &id)?),
        None => None,
    };
    // Servidor próprio do projeto: liga ele e o Adminer entra pela porta dele.
    let own_port = match &project {
        Some(p) => {
            start_own(&app, p).await?;
            project_config(&app, p)?.db_port
        }
        None => None,
    };
    if postgres {
        crate::pg::start_pg(&app).await?;
    } else if external.is_none() && own_port.is_none() {
        start_maria(&app).await?;
    }
    crate::server::ensure_hosts(&app).await.map_err(|e| e.message)?;
    crate::server::refresh(&app).await.map_err(|e| e.message)?;
    let mut url = adminer_url(&app);
    let db = database.filter(|d| d.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    if postgres {
        url = format!("{url}/?pg={}&db={}", crate::pg::pg_port(&app), db.unwrap_or_default());
    } else if let Some(c) = external {
        let user: String = c.user.chars().filter(|ch| ch.is_ascii_alphanumeric() || "_-.".contains(*ch)).collect();
        url = format!("{url}/?server={}:{}&username={user}&db={}", c.host, c.port, db.unwrap_or_default());
    } else if let Some(port) = own_port {
        url = format!("{url}/?port={port}&db={}", db.unwrap_or_default());
    } else if let Some(db) = db {
        url = format!("{url}/?db={db}");
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}
