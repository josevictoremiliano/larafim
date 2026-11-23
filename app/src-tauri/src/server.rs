//! Liga o servidor do Larafim ao app: rotas a partir dos projetos, gateway
//! na porta configurada, arquivo hosts e projetos online/offline.

use larafim_core::t;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use larafim_core::{ProjectKind, ProjectStatus, ProjectView};
use larafim_runtimes::php as rphp;
use larafim_server::port::{PortOwner, check_free};
use larafim_runtimes::node as rnode;
use larafim_server::{
    DevEvent, DevServers, DevSpec, DevStatus, Gateway, PhpRuntime, PoolSettings, Pools, Routes, SharedRoutes, Site, hosts,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

pub struct ServerState {
    routes: SharedRoutes,
    pools: Arc<Pools>,
    gateway: tokio::sync::Mutex<Option<Gateway>>,
    /// Gateway HTTPS (porta própria, com o certificado da CA local).
    gateway_tls: tokio::sync::Mutex<Option<(Gateway, String)>>,
    /// Hosts do Windows desatualizados para os projetos online.
    hosts_outdated: std::sync::atomic::AtomicBool,
    /// Servidores de desenvolvimento do Node.
    pub dev: Arc<DevServers>,
    dev_events: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<DevEvent>>>,
}

impl ServerState {
    /// Reinicia os processos do PHP (ex.: depois de ligar o Xdebug).
    pub async fn restart_php(&self) {
        self.pools.stop_all().await;
    }

    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            routes: Arc::new(RwLock::new(Routes::default())),
            pools: Pools::new(PoolSettings::default()),
            gateway: tokio::sync::Mutex::new(None),
            gateway_tls: tokio::sync::Mutex::new(None),
            hosts_outdated: false.into(),
            dev: DevServers::new(move |e| {
                let _ = tx.send(e);
            }),
            dev_events: std::sync::Mutex::new(Some(rx)),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct OnlineSite {
    pub name: String,
    pub url: String,
    pub path: PathBuf,
}

#[derive(Serialize, Clone)]
pub struct ServerStatus {
    running: bool,
    port: u16,
    pub(crate) online: Vec<OnlineSite>,
    /// Versões do PHP com processos rodando agora.
    php_running: Vec<String>,
    /// Servidores do Node rodando agora.
    node_running: usize,
    hosts_outdated: bool,
}

/// Erro com código para a interface oferecer a ação certa.
#[derive(Serialize, Debug)]
pub struct ServerError {
    /// "port_busy", "db_failed", "uac_denied", "hosts_failed", "no_php", "no_node", "no_command", "error"
    pub code: &'static str,
    pub message: String,
    owner: Option<PortOwner>,
    /// Minor do PHP sugerido para instalar (code = "no_php").
    suggest: Option<String>,
}

impl ServerError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), owner: None, suggest: None }
    }
}

pub(crate) fn views(state: &AppState) -> Vec<ProjectView> {
    let roots = state.config.lock().unwrap().roots.clone();
    state.projects.lock().unwrap().views(&roots)
}

/// Qual PHP instalado um projeto usa agora: (runtime, faltando?).
pub(crate) fn php_for(view: &ProjectView, installed: &[rphp::InstalledPhp]) -> (Option<PhpRuntime>, bool) {
    if view.kind == ProjectKind::Node {
        return (None, false);
    }
    let found = match (&view.php_pin, &view.php_request) {
        (Some(minor), _) => installed.iter().find(|p| &p.minor == minor),
        (None, Some(req)) => rphp::best_installed(req, installed),
        (None, None) => installed.first(),
    };
    match found {
        Some(p) => (Some(PhpRuntime { version: p.version.clone(), dir: p.dir.clone() }), false),
        None => (None, true),
    }
}

/// Reconstrói a tabela de domínios a partir do cadastro.
fn build_routes(app: &AppHandle) -> Routes {
    let state = app.state::<AppState>();
    let tld = state.config.lock().unwrap().tld.clone();
    let installed = rphp::list_installed(&state.paths.php_dir());
    let dev = app.state::<ServerState>().dev.clone();
    let mut routes = Routes::default();
    for v in views(&state).into_iter().filter(|v| v.status == ProjectStatus::Imported) {
        let (php, php_missing) = php_for(&v, &installed);
        // Projeto só de Node: o gateway repassa para o servidor de dev.
        let proxy = (v.kind == ProjectKind::Node)
            .then(|| dev.port_cell(&dev_key(&v.path)).unwrap_or_else(|| Arc::new(std::sync::Mutex::new(None))));
        // Reverb ligado: o WebSocket do navegador passa pelo gateway.
        let ws = v.reverb_port.filter(|_| v.kind == ProjectKind::Laravel && v.workers.iter().any(|w| w == "reverb"));
        let site = Site { name: v.name.clone(), docroot: v.path.join(&v.docroot), php, php_missing, online: v.online, proxy, ws };
        // Aliases apontam para o mesmo site (mesmo PHP, proxy e Reverb).
        let aliases = state.projects.lock().unwrap().find(&v.path).map(|c| c.alias_hosts(&tld)).unwrap_or_default();
        for host in aliases {
            routes.sites.entry(host).or_insert_with(|| site.clone());
        }
        routes.sites.insert(format!("{}.{tld}", v.domain), site);
    }
    // Caixa de e-mails do Mailpit em mail.<domínio> (proxy para a interface dele).
    if larafim_runtimes::tools::mailpit_installed(&state.paths) {
        let cell = dev
            .port_cell(crate::tools::MAILPIT_KEY)
            .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(None)));
        routes.sites.insert(
            format!("mail.{tld}"),
            Site {
                name: "Mailpit".into(),
                docroot: state.paths.root().to_path_buf(),
                php: None,
                php_missing: false,
                online: true,
                proxy: Some(cell),
                ws: None,
            },
        );
    }
    // Painel do storage S3 em s3.<domínio> (porta fixa: a saída do RustFS
    // mostra também a da API, que não é a do painel).
    if larafim_runtimes::s3::rustfs_installed(&state.paths) {
        routes.sites.insert(
            format!("s3.{tld}"),
            Site {
                name: "Storage S3".into(),
                docroot: state.paths.root().to_path_buf(),
                php: None,
                php_missing: false,
                online: true,
                proxy: Some(Arc::new(std::sync::Mutex::new(Some(crate::s3::S3_CONSOLE_PORT)))),
                ws: None,
            },
        );
    }
    // Adminer em db.<domínio>, com o PHP mais novo instalado.
    if larafim_runtimes::extras::adminer_installed(&state.paths)
        && let Some(p) = installed.first()
    {
        routes.sites.insert(
            format!("db.{tld}"),
            Site {
                name: "Adminer".into(),
                docroot: larafim_runtimes::extras::adminer_dir(&state.paths),
                php: Some(PhpRuntime { version: p.version.clone(), dir: p.dir.clone() }),
                php_missing: false,
                online: true,
                proxy: None,
                ws: None,
            },
        );
    }
    routes
}

/// Domínios que devem estar no arquivo hosts: todos os projetos importados
/// (assim ligar/desligar não pede permissão de novo).
fn wanted_hosts(app: &AppHandle) -> BTreeSet<String> {
    let state = app.state::<AppState>();
    let tld = state.config.lock().unwrap().tld.clone();
    state
        .projects
        .lock()
        .unwrap()
        .projects
        .iter()
        .flat_map(|p| std::iter::once(format!("{}.{tld}", p.domain)).chain(p.alias_hosts(&tld)))
        .chain(larafim_runtimes::extras::adminer_installed(&state.paths).then(|| format!("db.{tld}")))
        .chain(larafim_runtimes::tools::mailpit_installed(&state.paths).then(|| format!("mail.{tld}")))
        .chain(larafim_runtimes::s3::rustfs_installed(&state.paths).then(|| format!("s3.{tld}")))
        .filter(|h| hosts::valid_host(h))
        .collect()
}

/// Atualiza o hosts se precisar (pede UAC). Sem mudança, não pede nada.
pub async fn ensure_hosts(app: &AppHandle) -> Result<(), ServerError> {
    // Só para desenvolvimento/testes automáticos: não mexe no hosts.
    if std::env::var_os("LARAFIM_SKIP_HOSTS").is_some() {
        tracing::warn!("LARAFIM_SKIP_HOSTS definido: arquivo hosts não foi verificado");
        return Ok(());
    }
    let wanted = wanted_hosts(app);
    if hosts::is_up_to_date(&wanted) {
        app.state::<ServerState>().hosts_outdated.store(false, std::sync::atomic::Ordering::Relaxed);
        return Ok(());
    }
    let dir = app.state::<AppState>().paths.root().to_path_buf();
    let result = tauri::async_runtime::spawn_blocking(move || hosts::request_update(&wanted, &dir))
        .await
        .map_err(|e| ServerError::new("error", e.to_string()))?;
    match result {
        Ok(()) => {
            app.state::<ServerState>().hosts_outdated.store(false, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
        Err(hosts::HostsError::Denied) => Err(ServerError::new("uac_denied", hosts::HostsError::Denied.to_string())),
        Err(e) => Err(ServerError::new("hosts_failed", e.to_string())),
    }
}

/// Aplica o cadastro: rotas novas, gateway ligado só se houver projeto
/// online (e na porta configurada), status para a interface e bandeja.
pub async fn refresh(app: &AppHandle) -> Result<(), ServerError> {
    let routes = build_routes(app);
    // O gateway fica ligado se houver projeto online ou o MariaDB ligado (Adminer).
    let maria_on = app.state::<crate::services::ServicesState>().maria.status().await.running
        || crate::services::any_own_running(app).await
        || crate::pg::pg_running(app).await
        || crate::tools::mailpit_running(app)
        || crate::s3::s3_running(app);
    let any_online =
        routes.sites.iter().any(|(h, s)| s.online && !h.starts_with("db.") && !h.starts_with("mail.") && !h.starts_with("s3.")) || maria_on;
    let server = app.state::<ServerState>();
    *server.routes.write().unwrap() = routes;

    let port = app.state::<AppState>().config.lock().unwrap().http_port;
    let mut gateway = server.gateway.lock().await;
    let result = if any_online {
        if gateway.as_ref().is_some_and(|g| g.port() != port) {
            gateway.take().unwrap().stop();
        }
        if gateway.is_none() {
            match start_gateway(port, server.routes.clone(), server.pools.clone()).await {
                Ok(g) => {
                    *gateway = Some(g);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(())
        }
    } else {
        if let Some(g) = gateway.take() {
            g.stop();
            server.pools.stop_all().await;
        }
        Ok(())
    };
    drop(gateway);
    let result = result.and(refresh_tls(app, any_online).await);
    emit_status(app).await;
    result
}

/// Gateway HTTPS: ligado junto com o HTTP quando o HTTPS está ativo.
async fn refresh_tls(app: &AppHandle, any_online: bool) -> Result<(), ServerError> {
    let (wanted, port, tld) = {
        let c = app.state::<AppState>().config.lock().unwrap().clone();
        (c.https && any_online, c.https_port, c.tld.clone())
    };
    let server = app.state::<ServerState>();
    let mut tls = server.gateway_tls.lock().await;
    // Porta ou domínio mudaram: reinicia (o certificado depende do domínio).
    if let Some((g, t)) = tls.take() {
        if wanted && g.port() == port && t == tld {
            *tls = Some((g, t));
            return Ok(());
        }
        g.stop();
    }
    if !wanted {
        return Ok(());
    }
    if let Err(owner) = check_free(port) {
        let who = owner.map(|o| o.name).unwrap_or_else(|| t!("outro programa"));
        return Err(ServerError::new("https_port", t!("A porta {port} (HTTPS) já está em uso por {who}.", port = port, who = who)));
    }
    let config = crate::https::server_config(app).map_err(|m| ServerError::new("error", m))?;
    match Gateway::start_tls(port, server.routes.clone(), server.pools.clone(), config).await {
        Ok(g) => {
            *tls = Some((g, tld));
            Ok(())
        }
        Err(e) => Err(ServerError::new("https_port", t!("HTTPS na porta {port}: {e}", port = port, e = e))),
    }
}

async fn start_gateway(port: u16, routes: SharedRoutes, pools: Arc<Pools>) -> Result<Gateway, ServerError> {
    if let Err(owner) = check_free(port) {
        let who = owner
            .as_ref()
            .map(|o| match &o.hint {
                Some(h) => format!("{} ({h})", o.name),
                None => o.name.clone(),
            })
            .unwrap_or_else(|| t!("outro programa"));
        return Err(ServerError {
            code: "port_busy",
            message: t!("A porta {port} está ocupada por {who}.", port = port, who = who),
            owner,
            suggest: None,
        });
    }
    Gateway::start(port, routes, pools).await.map_err(|e| ServerError {
        code: "port_busy",
        message: t!("Não foi possível usar a porta {port}: {e}", port = port, e = e),
        owner: None,
        suggest: None,
    })
}

pub async fn status(app: &AppHandle) -> ServerStatus {
    let state = app.state::<AppState>();
    let server = app.state::<ServerState>();
    let (port, cfg) = {
        let c = state.config.lock().unwrap();
        (c.http_port, c.clone())
    };
    let running = server.gateway.lock().await.is_some();
    let online = state
        .projects
        .lock()
        .unwrap()
        .projects
        .iter()
        .filter(|p| p.online)
        .map(|p| OnlineSite {
            name: p.domain.clone(),
            url: public_url(&p.domain, &cfg),
            path: p.path.clone(),
        })
        .collect();
    ServerStatus {
        running,
        port,
        online,
        php_running: server.pools.running().await,
        node_running: server
            .dev
            .all()
            .iter()
            .filter(|d| d.state != larafim_server::DevState::Exited && is_node_key(&d.key))
            .count(),
        hosts_outdated: server.hosts_outdated.load(std::sync::atomic::Ordering::Relaxed),
    }
}

fn project_url(domain: &str, tld: &str, port: u16) -> String {
    if port == 80 { format!("http://{domain}.{tld}") } else { format!("http://{domain}.{tld}:{port}") }
}

/// Endereço para abrir no navegador: HTTPS quando ligado.
fn public_url(domain: &str, c: &larafim_core::AppConfig) -> String {
    if !c.https {
        return project_url(domain, &c.tld, c.http_port);
    }
    if c.https_port == 443 { format!("https://{domain}.{}", c.tld) } else { format!("https://{domain}.{}:{}", c.tld, c.https_port) }
}

async fn emit_status(app: &AppHandle) {
    let s = status(app).await;
    crate::tray::update_online(app, &s.online);
    let _ = app.emit("server-status", &s);
}

fn set_online_flag(app: &AppHandle, path: &std::path::Path, online: bool) -> Result<(), ServerError> {
    let state = app.state::<AppState>();
    let mut store = state.projects.lock().unwrap();
    let Some(cfg) = store.projects.iter_mut().find(|p| larafim_core::store::same_path(&p.path, path)) else {
        return Err(ServerError::new("error", t!("importe o projeto antes de deixá-lo online")));
    };
    cfg.online = online;
    store.save(&state.paths.projects_file()).map_err(|e| ServerError::new("error", e.to_string()))
}

#[derive(Serialize)]
pub struct OnlineResult {
    status: ServerStatus,
}

/// Liga o que o projeto precisa antes do PHP: o banco gerenciado (MariaDB,
/// MariaDB próprio ou PostgreSQL) e o Redis, se a fila usa ele.
pub async fn start_project_db(app: &AppHandle, view: &larafim_core::ProjectView) -> Result<(), String> {
    if crate::redis::wanted_by(view) {
        crate::redis::start_redis(app).await?;
    }
    match view.database.as_deref() {
        Some("mariadb") => crate::services::start_maria(app).await,
        Some("mariadb-own") => crate::services::start_own(app, &view.path).await,
        Some("postgres") => crate::pg::start_pg(app).await,
        _ => Ok(()),
    }
}

/// Liga ou desliga um projeto.
#[tauri::command]
pub async fn set_project_online(app: AppHandle, path: PathBuf, online: bool) -> Result<OnlineResult, ServerError> {
    if online {
        let view = views(&app.state::<AppState>())
            .into_iter()
            .find(|v| larafim_core::store::same_path(&v.path, &path))
            .ok_or_else(|| ServerError::new("error", t!("projeto não encontrado")))?;
        if view.status != ProjectStatus::Imported {
            return Err(ServerError::new("error", t!("importe o projeto antes de deixá-lo online")));
        }
        let installed = rphp::list_installed(&app.state::<AppState>().paths.php_dir());
        if let (None, true) = php_for(&view, &installed) {
            let mut e = ServerError::new("no_php", t!("Nenhum PHP instalado atende a este projeto."));
            e.suggest = view.php_pin.clone();
            return Err(e);
        }
        ensure_hosts(&app).await?;
        start_project_db(&app, &view).await.map_err(|m| ServerError::new("db_failed", m))?;
        if view.storage.as_deref() == Some("s3") {
            crate::s3::start_s3(&app).await.map_err(|m| ServerError::new("error", m))?;
        }
        if view.mail.as_deref() == Some("mailpit") {
            crate::tools::start_mailpit(&app).await.map_err(|m| ServerError::new("error", m))?;
        }
        if needs_dev(&view) {
            start_dev(&app, &view)?;
        }
        crate::tools::start_workers(&app, &view);
    } else {
        app.state::<ServerState>().dev.stop(&dev_key(&path));
        crate::tools::stop_workers(&app, &path);
    }

    set_online_flag(&app, &path, online)?;
    if let Err(e) = refresh(&app).await {
        // Não deu para ligar o servidor: o projeto volta a ficar offline.
        app.state::<ServerState>().dev.stop(&dev_key(&path));
        let _ = set_online_flag(&app, &path, false);
        let _ = refresh(&app).await;
        notify_projects(&app);
        return Err(e);
    }
    notify_projects(&app);
    tracing::info!(path = %path.display(), online, "projeto alterado");
    Ok(OnlineResult { status: status(&app).await })
}

/// Avisa as janelas que o cadastro mudou (interruptor Online).
fn notify_projects(app: &AppHandle) {
    let state = app.state::<AppState>();
    let roots = state.config.lock().unwrap().roots.clone();
    let store = state.projects.lock().unwrap();
    let _ = app.emit(
        "projects-changed",
        serde_json::json!({ "projects": store.views(&roots), "ignored": store.ignored }),
    );
}

#[tauri::command]
pub async fn server_status(app: AppHandle) -> ServerStatus {
    status(&app).await
}

/// Desliga todos os projetos (bandeja e barra de status).
#[tauri::command]
pub async fn stop_all_projects(app: AppHandle) -> Result<ServerStatus, ServerError> {
    stop_all(&app).await;
    Ok(status(&app).await)
}

pub async fn stop_all(app: &AppHandle) {
    app.state::<ServerState>().dev.stop_all();
    {
        let state = app.state::<AppState>();
        let mut store = state.projects.lock().unwrap();
        store.projects.iter_mut().for_each(|p| p.online = false);
        let _ = store.save(&state.paths.projects_file());
    }
    let _ = refresh(app).await;
    notify_projects(app);
}

/// Corrige o arquivo hosts (pede UAC).
#[tauri::command]
pub async fn fix_hosts(app: AppHandle) -> Result<ServerStatus, ServerError> {
    ensure_hosts(&app).await?;
    emit_status(&app).await;
    Ok(status(&app).await)
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    // Só endereços do Larafim: domínio local (http/https) ou localhost.
    let tld = app.state::<AppState>().config.lock().unwrap().tld.clone();
    if !larafim_core::safety::is_local_url(&url, &tld) {
        return Err(t!("endereço inválido"));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Depois de editar projetos ou trocar o PHP: rotas novas e, se um projeto
/// online mudou de domínio, o hosts (pode pedir UAC, porque foi a pessoa que mudou).
pub fn after_change(app: &AppHandle, hosts_too: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let any_online = app.state::<AppState>().projects.lock().unwrap().projects.iter().any(|p| p.online);
        if hosts_too && any_online && let Err(e) = ensure_hosts(&app).await {
            app.state::<ServerState>().hosts_outdated.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = app.emit("server-error", &e);
        }
        if let Err(e) = refresh(&app).await {
            let _ = app.emit("server-error", &e);
        }
    });
}

/// Ao abrir o Larafim: religa os projetos que estavam online. Não pede UAC
/// sozinho; se o hosts estiver desatualizado, a interface oferece corrigir.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Desliga pools parados de tempos em tempos.
        let reaper = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                reaper.state::<ServerState>().pools.reap_idle().await;
            }
        });

        forward_dev_events(&app);
        // Liga o MariaDB se algum projeto online usa ele.
        let wants_db = views(&app.state::<AppState>()).iter().any(|v| v.online && v.database.as_deref() == Some("mariadb"));
        if wants_db && let Err(e) = crate::services::start_maria(&app).await {
            tracing::warn!("MariaDB não iniciou: {e}");
        }
        for v in views(&app.state::<AppState>()).into_iter().filter(|v| v.online && v.database.as_deref() == Some("mariadb-own")) {
            if let Err(e) = crate::services::start_own(&app, &v.path).await {
                tracing::warn!(project = v.name, "MariaDB próprio não iniciou: {e}");
            }
        }
        if views(&app.state::<AppState>()).iter().any(|v| v.online && v.storage.as_deref() == Some("s3"))
            && let Err(e) = crate::s3::start_s3(&app).await
        {
            tracing::warn!("storage S3 não iniciou: {e}");
        }
        if views(&app.state::<AppState>()).iter().any(|v| v.online && v.database.as_deref() == Some("postgres"))
            && let Err(e) = crate::pg::start_pg(&app).await
        {
            tracing::warn!("PostgreSQL não iniciou: {e}");
        }
        if views(&app.state::<AppState>()).iter().any(|v| v.online && crate::redis::wanted_by(v))
            && let Err(e) = crate::redis::start_redis(&app).await
        {
            tracing::warn!("Redis não iniciou: {e}");
        }
        // Religa os servidores do Node dos projetos que estavam online.
        for v in views(&app.state::<AppState>()).into_iter().filter(|v| v.online && needs_dev(v)) {
            if let Err(e) = start_dev(&app, &v) {
                tracing::warn!(project = v.name, "servidor do Node não iniciou: {}", e.message);
            }
        }
        for v in views(&app.state::<AppState>()).into_iter().filter(|v| v.online) {
            crate::tools::start_workers(&app, &v);
        }
        if views(&app.state::<AppState>()).iter().any(|v| v.online && v.mail.as_deref() == Some("mailpit"))
            && let Err(e) = crate::tools::start_mailpit(&app).await
        {
            tracing::warn!("Mailpit não iniciou: {e}");
        }

        let any_online = app.state::<AppState>().projects.lock().unwrap().projects.iter().any(|p| p.online);
        if any_online && !hosts::is_up_to_date(&wanted_hosts(&app)) {
            app.state::<ServerState>().hosts_outdated.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Err(e) = refresh(&app).await {
            tracing::warn!("servidor não iniciou: {}", e.message);
            let _ = app.emit("server-error", &e);
        }
    });
}

/// Ao sair: para tudo (o Job Object garante o mesmo se o app travar).
pub fn shutdown(app: &AppHandle) {
    let server = app.state::<ServerState>();
    tauri::async_runtime::block_on(async {
        if let Some(g) = server.gateway.lock().await.take() {
            g.stop();
        }
        if let Some((g, _)) = server.gateway_tls.lock().await.take() {
            g.stop();
        }
        server.pools.stop_all().await;
        app.state::<crate::services::ServicesState>().maria.stop().await;
        crate::services::stop_all_own(app).await;
        app.state::<crate::pg::PgState>().svc.stop().await;
        crate::redis::stop_redis(app).await;
    });
    server.dev.stop_all();
}

// ---------- Servidores de desenvolvimento do Node ----------

/// Chave do servidor de dev de um projeto.
pub fn dev_key(path: &std::path::Path) -> String {
    path.display().to_string().to_lowercase()
}

/// Só os servidores Node dos projetos (Mailpit, workers e criação de projeto usam chaves próprias).
fn is_node_key(key: &str) -> bool {
    !key.contains('|') && !key.starts_with("service:") && !key.starts_with("create:")
}

/// Projeto Node, ou PHP com "Servidor do Node" ligado (ex.: Laravel + Vite).
fn needs_dev(v: &ProjectView) -> bool {
    v.kind == ProjectKind::Node || v.node_enabled
}

fn first_number(text: &str) -> Option<String> {
    let digits: String = text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    (!digits.is_empty()).then_some(digits)
}

fn start_dev(app: &AppHandle, v: &ProjectView) -> Result<(), ServerError> {
    let state = app.state::<AppState>();
    let command = v
        .node_command
        .clone()
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| ServerError::new("no_command", t!("Informe o comando do Node do projeto (ex.: npm run dev).")))?;
    let sel = rnode::select(&state.paths, &v.path);
    let Some(node) = sel.node else {
        let mut e = ServerError::new("no_node", format!("Node.js: {}.", sel.reason));
        e.suggest = v.node_pin.clone().or_else(|| v.node_request.as_deref().and_then(first_number));
        return Err(e);
    };
    let (tld, http_port) = {
        let c = state.config.lock().unwrap();
        (c.tld.clone(), c.http_port)
    };
    let url = project_url(&v.domain, &tld, http_port);
    // APP_URL: o plugin do Laravel para o Vite libera o domínio do projeto (CORS).
    let mut env = vec![("APP_URL".to_string(), url.clone()), ("LARAFIM_URL".to_string(), url)];
    if v.kind == ProjectKind::Node {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .unwrap_or(3000);
        env.push(("PORT".into(), port.to_string()));
    }
    let spec = DevSpec { key: dev_key(&v.path), cwd: v.path.clone(), command: command.clone(), node_dir: node.dir.clone(), env };
    app.state::<ServerState>()
        .dev
        .start(spec)
        .map_err(|e| ServerError::new("error", t!("Não foi possível iniciar o comando ({command}): {e}", command = command, e = e)))
}

#[derive(Serialize)]
pub struct DevInfo {
    status: Option<DevStatus>,
    logs: Vec<String>,
}

pub fn dev_info_for(app: &AppHandle, key: &str) -> DevInfo {
    let dev = &app.state::<ServerState>().dev;
    DevInfo { status: dev.status(key), logs: dev.logs(key) }
}

#[tauri::command]
pub fn dev_info(app: AppHandle, path: PathBuf) -> DevInfo {
    dev_info_for(&app, &dev_key(&path))
}

/// Reinicia o servidor do Node de um projeto online.
#[tauri::command]
pub async fn restart_dev(app: AppHandle, path: PathBuf) -> Result<(), ServerError> {
    let view = views(&app.state::<AppState>())
        .into_iter()
        .find(|v| larafim_core::store::same_path(&v.path, &path))
        .ok_or_else(|| ServerError::new("error", t!("projeto não encontrado")))?;
    if !view.online || !needs_dev(&view) {
        return Err(ServerError::new("error", t!("o projeto não está online com servidor do Node")));
    }
    start_dev(&app, &view)?;
    refresh(&app).await
}

#[derive(Serialize, Clone)]
struct DevLog {
    key: String,
    line: String,
}

/// Repassa os eventos dos servidores de dev para as janelas.
fn forward_dev_events(app: &AppHandle) {
    let Some(mut rx) = app.state::<ServerState>().dev_events.lock().unwrap().take() else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                DevEvent::Log { key, line } => {
                    let _ = app.emit("node:log", DevLog { key, line });
                }
                DevEvent::Status(s) => {
                    if s.state == larafim_server::DevState::Exited {
                        crate::tools::on_process_exit(&app, &s.key, s.exit_code);
                    }
                    let _ = app.emit("node:status", &s);
                    emit_status(&app).await;
                }
            }
        }
    });
}
