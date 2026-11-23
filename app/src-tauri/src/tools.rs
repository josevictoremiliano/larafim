//! Ferramentas (fase 7): Mailpit, logs do projeto, workers do Laravel,
//! Xdebug e criação de projeto Laravel novo.

use larafim_core::t;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use larafim_core::envfile::{self, DiffLine, EnvChange};
use larafim_core::store::same_path;
use larafim_core::{ProjectKind, ProjectView, reverb, safety};
use larafim_runtimes::php::{self as rphp};
use larafim_runtimes::{http_client, tools};
use larafim_server::DevSpec;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::server::ServerState;

type CmdResult<T> = Result<T, String>;

pub const MAILPIT_KEY: &str = "service:mailpit";
const MAIL_UI_PORT: u16 = 8025;
pub(crate) const MAIL_SMTP_PORT: u16 = 1025;

#[derive(Default)]
pub struct ToolsState {
    /// Criações de projeto em andamento: chave do processo → dados.
    creating: Mutex<HashMap<String, CreateJob>>,
}

#[derive(Clone)]
struct CreateJob {
    path: PathBuf,
    php: String,
    database: String,
}

fn view_of(app: &AppHandle, path: &Path) -> Result<ProjectView, String> {
    crate::server::views(&app.state::<AppState>())
        .into_iter()
        .find(|v| same_path(&v.path, path))
        .ok_or_else(|| t!("projeto não encontrado"))
}

fn notify_projects(app: &AppHandle) {
    let state = app.state::<AppState>();
    let roots = state.config.lock().unwrap().roots.clone();
    let store = state.projects.lock().unwrap();
    let _ = app.emit("projects-changed", serde_json::json!({ "projects": store.views(&roots), "ignored": store.ignored }));
}

pub(crate) fn update_project(app: &AppHandle, path: &Path, f: impl FnOnce(&mut larafim_core::ProjectConfig)) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut store = state.projects.lock().unwrap();
        let p = store
            .projects
            .iter_mut()
            .find(|p| same_path(&p.path, path))
            .ok_or_else(|| t!("importe o projeto antes"))?;
        f(p);
        store.save(&state.paths.projects_file()).map_err(|e| e.to_string())?;
    }
    notify_projects(app);
    Ok(())
}

// ================= Mailpit =================

#[derive(Serialize)]
pub struct MailpitStatus {
    installed: bool,
    version: Option<String>,
    running: bool,
    ui_url: String,
    smtp: String,
}

pub fn mail_url(app: &AppHandle) -> String {
    let c = app.state::<AppState>().config.lock().unwrap().clone();
    if c.http_port == 80 { format!("http://mail.{}", c.tld) } else { format!("http://mail.{}:{}", c.tld, c.http_port) }
}

pub fn mailpit_running(app: &AppHandle) -> bool {
    app.state::<ServerState>().dev.is_running(MAILPIT_KEY)
}

#[tauri::command]
pub fn mailpit_status(app: AppHandle) -> MailpitStatus {
    let paths = app.state::<AppState>().paths.clone();
    MailpitStatus {
        installed: tools::mailpit_installed(&paths),
        version: std::fs::read_to_string(tools::mailpit_dir(&paths).join("VERSION")).ok().map(|v| v.trim().to_string()),
        running: mailpit_running(&app),
        ui_url: mail_url(&app),
        smtp: format!("127.0.0.1:{MAIL_SMTP_PORT}"),
    }
}

#[tauri::command]
pub async fn mailpit_install(app: AppHandle) -> CmdResult<()> {
    let paths = app.state::<AppState>().paths.clone();
    let client = http_client().map_err(|e| e.to_string())?;
    tools::install_mailpit(&client, &paths).await.map_err(|e| e.to_string())?;
    let _ = app.emit("services-changed", ());
    Ok(())
}

/// Liga o Mailpit (SMTP 1025, caixa de entrada em mail.<domínio>).
pub async fn start_mailpit(app: &AppHandle) -> Result<(), String> {
    if mailpit_running(app) {
        return Ok(());
    }
    let paths = app.state::<AppState>().paths.clone();
    if !tools::mailpit_installed(&paths) {
        let client = http_client().map_err(|e| e.to_string())?;
        tools::install_mailpit(&client, &paths).await.map_err(|e| e.to_string())?;
    }
    for port in [MAIL_UI_PORT, MAIL_SMTP_PORT] {
        if let Err(owner) = larafim_server::port::check_free(port) {
            let who = owner.map(|o| o.name).unwrap_or_else(|| t!("outro programa"));
            return Err(t!("a porta {port} (Mailpit) está ocupada por {who}", port = port, who = who));
        }
    }
    let data = paths.data_dir().join("mailpit");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let exe = tools::mailpit_exe(&paths);
    let command = format!(
        "\"{}\" --listen 127.0.0.1:{MAIL_UI_PORT} --smtp 127.0.0.1:{MAIL_SMTP_PORT} --database \"{}\"",
        exe.display(),
        data.join("mailpit.db").display()
    );
    app.state::<ServerState>()
        .dev
        .start(DevSpec {
            key: MAILPIT_KEY.into(),
            cwd: tools::mailpit_dir(&paths),
            command,
            node_dir: tools::mailpit_dir(&paths),
            env: vec![("PORT".into(), MAIL_UI_PORT.to_string())],
        })
        .map_err(|e| e.to_string())?;
    crate::server::after_change(app, false);
    let _ = app.emit("services-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn mailpit_start(app: AppHandle) -> CmdResult<()> {
    start_mailpit(&app).await
}

#[tauri::command]
pub async fn mailpit_stop(app: AppHandle) {
    app.state::<ServerState>().dev.stop(MAILPIT_KEY);
    crate::server::after_change(&app, false);
    let _ = app.emit("services-changed", ());
}

#[tauri::command]
pub async fn open_mailpit(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    start_mailpit(&app).await?;
    crate::server::ensure_hosts(&app).await.map_err(|e| e.message)?;
    crate::server::refresh(&app).await.map_err(|e| e.message)?;
    app.opener().open_url(mail_url(&app), None::<&str>).map_err(|e| e.to_string())
}

fn mail_changes(kind: &str) -> Vec<EnvChange> {
    match kind {
        "mailpit" => vec![
            EnvChange::set("MAIL_MAILER", "smtp"),
            EnvChange::set("MAIL_HOST", "127.0.0.1"),
            EnvChange::set("MAIL_PORT", &MAIL_SMTP_PORT.to_string()),
            EnvChange::set("MAIL_USERNAME", "null"),
            EnvChange::set("MAIL_PASSWORD", "null"),
        ],
        _ => Vec::new(),
    }
}

#[tauri::command]
pub fn project_mail_preview(path: PathBuf, kind: String) -> CmdResult<Vec<DiffLine>> {
    let base = std::fs::read_to_string(envfile::env_path(&path)).unwrap_or_default();
    Ok(envfile::apply(&base, &mail_changes(&kind)).1)
}

/// Liga o projeto ao Mailpit (ajusta o .env com cópia) ou desvincula.
#[tauri::command]
pub async fn project_mail_apply(app: AppHandle, path: PathBuf, kind: String) -> CmdResult<()> {
    if kind == "mailpit" {
        start_mailpit(&app).await?;
        if envfile::env_path(&path).exists() {
            envfile::write(&path, &mail_changes(&kind)).map_err(|e| e.to_string())?;
            env_changed(&app, &path);
        }
    } else if kind != "none" {
        return Err(t!("serviço de e-mail desconhecido: {kind}", kind = kind));
    }
    update_project(&app, &path, |p| p.mail = (kind == "mailpit").then_some(kind.clone()))
}

// ================= Logs =================

#[derive(Serialize)]
pub struct LogFile {
    name: String,
    size: u64,
    modified: u64,
}

/// Pasta de logs do projeto: `storage\logs` (Laravel) ou `logs`.
fn log_dir(project: &Path) -> PathBuf {
    let laravel = project.join("storage").join("logs");
    if laravel.is_dir() { laravel } else { project.join("logs") }
}

fn safe_log(project: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") || !name.ends_with(".log") {
        return Err(t!("arquivo de log inválido"));
    }
    Ok(log_dir(project).join(name))
}

#[tauri::command]
pub fn log_files(path: PathBuf) -> Vec<LogFile> {
    let mut list: Vec<LogFile> = std::fs::read_dir(log_dir(&path))
        .map(|r| {
            r.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    Some(LogFile {
                        name: e.file_name().to_string_lossy().to_string(),
                        size: m.len(),
                        modified: m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|f| std::cmp::Reverse(f.modified));
    list
}

#[derive(Serialize)]
pub struct LogChunk {
    text: String,
    offset: u64,
    /// O arquivo encolheu (foi limpo ou trocado): o texto recomeça.
    reset: bool,
}

/// Lê o que entrou no log desde `offset`. Na primeira leitura (sem
/// offset) traz só o final (até 256 KB), para não pesar.
#[tauri::command]
pub fn log_read(path: PathBuf, name: String, offset: Option<u64>) -> CmdResult<LogChunk> {
    const FIRST: u64 = 256 * 1024;
    let file = safe_log(&path, &name)?;
    let mut f = std::fs::File::open(&file).map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    let (start, reset) = match offset {
        Some(o) if o <= size => (o, false),
        Some(_) => (size.saturating_sub(FIRST), true),
        None => (size.saturating_sub(FIRST), true),
    };
    let len = (size - start).min(4 * 1024 * 1024);
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(LogChunk { text: String::from_utf8_lossy(&buf).to_string(), offset: start + len, reset })
}

/// Esvazia o arquivo de log (a pessoa confirma antes).
#[tauri::command]
pub fn log_clear(path: PathBuf, name: String) -> CmdResult<()> {
    let file = safe_log(&path, &name)?;
    std::fs::write(&file, b"").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn log_open(app: AppHandle, path: PathBuf, name: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let file = safe_log(&path, &name)?;
    app.opener().open_path(file.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

// ================= Após mudar o .env =================

/// `php artisan config:clear` se o projeto tiver a configuração em cache
/// (`bootstrap/cache/config.php`): com o cache, o Laravel ignora o `.env`.
fn clear_config_cache(app: &AppHandle, path: &Path) {
    if !path.join("artisan").is_file() || !path.join("bootstrap").join("cache").join("config.php").is_file() {
        return;
    }
    let Ok(view) = view_of(app, path) else { return };
    let installed = rphp::list_installed(&app.state::<AppState>().paths.php_dir());
    let Some(php) = crate::server::php_for(&view, &installed).0 else { return };
    let (exe, artisan) = (php.dir.join("php.exe").display().to_string(), path.join("artisan").display().to_string());
    let ok = crate::redis::run_hidden(&exe, &[&artisan, "config:clear"], std::time::Duration::from_secs(60)).is_some_and(|(ok, _)| ok);
    tracing::info!(path = %path.display(), ok, "config:clear depois de mudar o .env");
}

/// Depois de mudar o `.env`: limpa o cache de configuração e reinicia os
/// workers que estão rodando (processos longos leem o `.env` só ao subir).
pub fn env_changed(app: &AppHandle, path: &Path) {
    let (app, path) = (app.clone(), path.to_path_buf());
    tauri::async_runtime::spawn(async move {
        let (a, p) = (app.clone(), path.clone());
        let _ = tauri::async_runtime::spawn_blocking(move || clear_config_cache(&a, &p)).await;
        let Ok(view) = view_of(&app, &path) else { return };
        for w in ["queue", "schedule", "reverb"] {
            if app.state::<ServerState>().dev.is_running(&worker_key(&path, w))
                && let Err(e) = start_worker(&app, &view, w)
            {
                tracing::warn!(project = view.name, worker = w, "worker não reiniciou: {e}");
            }
        }
    });
}

// ================= Instalações no projeto (Composer/Artisan) =================

#[derive(Clone, Serialize)]
struct InstallProgress {
    path: PathBuf,
    line: String,
    percent: u8,
}

/// Roda um comando do Composer/Artisan no projeto e manda cada linha (com a
/// porcentagem estimada) no evento `project-install`. Devolve (ok, saída).
pub(crate) fn run_install(
    app: &AppHandle,
    path: &Path,
    exe: &Path,
    args: &[String],
    env: &[(&str, String)],
) -> (bool, String) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, e.to_string()),
    };
    let out = std::sync::Arc::new(Mutex::new(String::new()));
    let percent = std::sync::Arc::new(Mutex::new(5u8));
    let emit = |line: &str, p: u8| {
        let _ = app.emit("project-install", InstallProgress { path: path.to_path_buf(), line: line.to_string(), percent: p });
    };
    emit("", 5);
    let readers: Vec<Box<dyn std::io::Read + Send>> = vec![
        Box::new(child.stdout.take().expect("stdout")),
        Box::new(child.stderr.take().expect("stderr")),
    ];
    let handles: Vec<_> = readers
        .into_iter()
        .map(|r| {
            let (app, path, out, percent) = (app.clone(), path.to_path_buf(), out.clone(), percent.clone());
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(r).lines().map_while(Result::ok) {
                    let line = larafim_server::devserver::strip_ansi(&line);
                    let p = {
                        let mut cur = percent.lock().unwrap();
                        if let Some(n) = larafim_core::composer::progress(&line) {
                            *cur = (*cur).max(n); // nunca volta
                        }
                        *cur
                    };
                    {
                        let mut o = out.lock().unwrap();
                        if o.len() < 64 * 1024 {
                            o.push_str(&line);
                            o.push('\n');
                        }
                    }
                    if !line.trim().is_empty() {
                        let _ = app.emit("project-install", InstallProgress { path: path.clone(), line: line.trim().to_string(), percent: p });
                    }
                }
            })
        })
        .collect();
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    for h in handles {
        let _ = h.join();
    }
    emit("", if ok { 100 } else { *percent.lock().unwrap() });
    let text = out.lock().unwrap().clone();
    (ok, text)
}

// ================= Workers do Laravel =================

pub fn worker_key(path: &Path, worker: &str) -> String {
    format!("{}|{worker}", path.display().to_string().to_lowercase())
}

fn worker_command(worker: &str) -> Option<&'static str> {
    match worker {
        "queue" => Some("artisan queue:work --tries=3"),
        "schedule" => Some("artisan schedule:work"),
        "reverb" => Some("artisan reverb:start"),
        _ => None,
    }
}

/// Sobe um worker com o PHP do projeto.
pub fn start_worker(app: &AppHandle, view: &ProjectView, worker: &str) -> Result<(), String> {
    let args = worker_command(worker).ok_or_else(|| t!("worker desconhecido"))?;
    if view.kind != ProjectKind::Laravel {
        return Err(t!("workers só existem em projetos Laravel"));
    }
    let key = worker_key(&view.path, worker);
    // O Reverb escuta na porta do projeto (a linha de comando vale mais que o
    // .env), para dois projetos não disputarem a 8080.
    let args = if worker == "reverb" {
        let port = view.reverb_port.ok_or_else(|| t!("ligue o Reverb pelo painel do projeto para escolher a porta"))?;
        if !app.state::<ServerState>().dev.is_running(&key)
            && let Err(owner) = larafim_server::port::check_free(port)
        {
            let who = owner.map(|o| o.name).unwrap_or_else(|| t!("outro programa"));
            return Err(t!(
                "a porta {port} (Reverb) está ocupada por {who}; desligue e ligue o Reverb para trocar de porta",
                port = port,
                who = who
            ));
        }
        format!("{args} --host={} --port={port}", reverb::HOST)
    } else if worker == "queue" {
        // Nomes já validados (só letras, números e `._:-`): seguros no cmd.exe.
        let list: Vec<&str> = view.queues.iter().map(String::as_str).filter(|q| safety::is_queue_name(q)).collect();
        let connection = if crate::redis::wanted_by(view) { " redis" } else { "" };
        let args = args.replacen("queue:work", &format!("queue:work{connection}"), 1);
        if list.is_empty() { args } else { format!("{args} --queue={}", list.join(",")) }
    } else {
        args.to_string()
    };
    let installed = rphp::list_installed(&app.state::<AppState>().paths.php_dir());
    let (php, _) = crate::server::php_for(view, &installed);
    let php = php.ok_or_else(|| t!("nenhum PHP instalado atende a este projeto"))?;
    let exe = php.dir.join("php.exe");
    app.state::<ServerState>()
        .dev
        .start(DevSpec {
            key,
            cwd: view.path.clone(),
            command: format!("\"{}\" {args}", exe.display()),
            node_dir: php.dir.clone(),
            env: Vec::new(),
        })
        .map_err(|e| e.to_string())
}

pub fn stop_workers(app: &AppHandle, path: &Path) {
    for w in ["queue", "schedule", "reverb"] {
        app.state::<ServerState>().dev.stop(&worker_key(path, w));
    }
}

/// Liga os workers marcados de um projeto (ao ficar online).
pub fn start_workers(app: &AppHandle, view: &ProjectView) {
    for w in &view.workers {
        // Fila ligada antes de existir a descoberta: descobre e liga depois.
        if w == "queue" && view.queues.is_empty() {
            let (app, view) = (app.clone(), view.clone());
            tauri::async_runtime::spawn(async move {
                let found = discover_queues(&app, &view).await;
                let started = update_project(&app, &view.path, |p| p.queues = found)
                    .and_then(|_| view_of(&app, &view.path))
                    .and_then(|v| start_worker(&app, &v, "queue"));
                if let Err(e) = started {
                    tracing::warn!(project = view.name, "worker de fila não iniciou: {e}");
                }
            });
            continue;
        }
        if let Err(e) = start_worker(app, view, w) {
            tracing::warn!(project = view.name, worker = w, "worker não iniciou: {e}");
        }
    }
}

// Comandos que iniciam processos precisam ser `async`: o gerenciador de
// processos usa o runtime assíncrono (num comando síncrono o app cairia).

/// Marca/desmarca um worker. Se o projeto estiver online, liga/desliga na hora.
/// O Reverb liga na hora mesmo com o projeto offline e ajusta o `.env` (a
/// interface mostra a prévia antes).
#[tauri::command]
pub async fn worker_set(app: AppHandle, path: PathBuf, worker: String, enabled: bool) -> CmdResult<()> {
    worker_command(&worker).ok_or_else(|| t!("worker desconhecido"))?;
    if worker == "reverb" && enabled {
        return reverb_enable(&app, &path).await;
    }
    // Primeira vez: descobre as filas do projeto antes de ligar.
    if worker == "queue" && enabled && view_of(&app, &path)?.queues.is_empty() {
        let found = discover_queues(&app, &view_of(&app, &path)?).await;
        update_project(&app, &path, |p| p.queues = found)?;
    }
    update_project(&app, &path, |p| {
        p.workers.retain(|w| w != &worker);
        if enabled {
            p.workers.push(worker.clone());
        }
    })?;
    let view = view_of(&app, &path)?;
    if enabled && view.online {
        start_worker(&app, &view, &worker)?;
    } else if !enabled {
        app.state::<ServerState>().dev.stop(&worker_key(&path, &worker));
    }
    Ok(())
}

#[tauri::command]
pub async fn worker_restart(app: AppHandle, path: PathBuf, worker: String) -> CmdResult<()> {
    let view = view_of(&app, &path)?;
    start_worker(&app, &view, &worker)
}

/// Status e saída de qualquer processo gerenciado, pela chave.
#[tauri::command]
pub fn process_info(app: AppHandle, key: String) -> crate::server::DevInfo {
    crate::server::dev_info_for(&app, &key)
}

/// Projeto tem Reverb instalado?
#[tauri::command]
pub fn has_reverb(path: PathBuf) -> bool {
    reverb::is_installed(&path)
}

/// Instala o Reverb no projeto (a pessoa confirma antes): o mesmo que
/// `php artisan install:broadcasting --reverb`, sem perguntas e sem npm,
/// usando o PHP e o Composer do Larafim. Altera composer.json, rotas e .env.
#[tauri::command]
pub async fn project_install_reverb(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    if reverb::is_installed(&path) {
        return Ok(());
    }
    let view = view_of(&app, &path)?;
    if view.kind != ProjectKind::Laravel {
        return Err(t!("workers só existem em projetos Laravel"));
    }
    let paths = app.state::<AppState>().paths.clone();
    let composer = larafim_runtimes::extras::composer_path(&paths);
    if !composer.is_file() {
        return Err(t!("o Composer ainda não foi baixado (instale um PHP pelo Larafim)"));
    }
    let installed = rphp::list_installed(&paths.php_dir());
    let php = crate::server::php_for(&view, &installed).0.ok_or_else(|| t!("nenhum PHP instalado atende a este projeto"))?;
    // O comando roda o reverb:install, que lê o banco/cache em alguns projetos.
    crate::server::start_project_db(&app, &view).await?;
    let (app2, dir) = (app.clone(), path.clone());
    let args: Vec<String> = ["artisan", "install:broadcasting", "--reverb", "--without-node", "--no-interaction", "--no-ansi"]
        .iter()
        .map(|s| s.to_string())
        .chain([format!("--composer={}", composer.display())])
        .collect();
    let exe = php.dir.join("php.exe");
    // O Composer chamado pelo Laravel usa este PHP (o do projeto).
    let path_env = format!("{};{}", php.dir.display(), std::env::var("PATH").unwrap_or_default());
    let (ok, out) = tauri::async_runtime::spawn_blocking(move || {
        run_install(&app2, &dir, &exe, &args, &[("PATH", path_env), ("COMPOSER_NO_INTERACTION", "1".into())])
    })
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(path = %path.display(), ok, "install:broadcasting --reverb");
    // Instalação interrompida deixa o vendor sem autoload novo ("Class Pusher\Pusher
    // not found"): refaz o autoload e publica a config, se faltar.
    let (ok, out) = if !reverb::is_installed(&path) && path.join("vendor").join("laravel").join("reverb").is_dir() {
        let (app2, dir, exe) = (app.clone(), path.clone(), php.dir.join("php.exe"));
        let args = vec![composer.display().to_string(), "dump-autoload".into(), "--no-interaction".into(), "--no-ansi".into()];
        tauri::async_runtime::spawn_blocking(move || run_install(&app2, &dir, &exe, &args, &[("COMPOSER_NO_INTERACTION", "1".into())]))
            .await
            .map_err(|e| e.to_string())?
    } else {
        (ok, out)
    };
    if reverb::is_installed(&path) && !path.join("config").join("reverb.php").is_file() {
        let (app2, dir, exe) = (app.clone(), path.clone(), php.dir.join("php.exe"));
        let args: Vec<String> = ["artisan", "vendor:publish", "--tag=reverb-config", "--no-interaction", "--no-ansi"].iter().map(|s| s.to_string()).collect();
        let _ = tauri::async_runtime::spawn_blocking(move || run_install(&app2, &dir, &exe, &args, &[])).await;
    }
    if !ok || !reverb::is_installed(&path) {
        let tail: String = out.trim().chars().rev().take(500).collect::<String>().chars().rev().collect();
        return Err(t!("não foi possível instalar o Reverb: {a}", a = tail));
    }
    env_changed(&app, &path);
    Ok(())
}

// ================= Filas =================

/// Pergunta ao Laravel quais filas ele declara (conexão padrão, Horizon,
/// `queue.names`) e junta com os `onQueue('...')` do código. Se o PHP
/// falhar (banco desligado num provider, por exemplo), fica só a varredura.
async fn discover_queues(app: &AppHandle, view: &ProjectView) -> Vec<String> {
    let installed = rphp::list_installed(&app.state::<AppState>().paths.php_dir());
    let php = crate::server::php_for(view, &installed).0.map(|p| p.dir.join("php.exe"));
    let dir = view.path.clone();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        let declared = php.map(|exe| run_probe(&exe, &dir)).unwrap_or_default();
        larafim_core::queues::merge(&[declared, larafim_core::queues::scan_project(&dir)])
    });
    probe.await.unwrap_or_else(|_| vec!["default".into()])
}

/// Roda `PHP_PROBE` com tempo limite (um provider travado não segura o app).
fn run_probe(php: &Path, project: &Path) -> Vec<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(php);
    cmd.arg("-r").arg(larafim_core::queues::PHP_PROBE).current_dir(project).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(mut child) = cmd.spawn() else { return Vec::new() };
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.take(1024 * 1024).read_to_string(&mut s);
        }
        s
    });
    let started = std::time::Instant::now();
    while child.try_wait().ok().flatten().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(30) {
            let _ = child.kill();
            tracing::warn!(path = %project.display(), "descoberta de filas passou do tempo");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.wait();
    larafim_core::queues::parse_probe(&reader.join().unwrap_or_default())
}

/// Se o worker de fila estiver rodando, reinicia com a lista nova.
pub(crate) fn restart_queue_if_running(app: &AppHandle, path: &Path) -> Result<(), String> {
    if app.state::<ServerState>().dev.is_running(&worker_key(path, "queue")) {
        start_worker(app, &view_of(app, path)?, "queue")?;
    }
    Ok(())
}

/// Descobre de novo as filas do projeto e grava.
#[tauri::command]
pub async fn queues_discover(app: AppHandle, path: PathBuf) -> CmdResult<Vec<String>> {
    let view = view_of(&app, &path)?;
    if view.kind != ProjectKind::Laravel {
        return Err(t!("workers só existem em projetos Laravel"));
    }
    let found = discover_queues(&app, &view).await;
    update_project(&app, &path, |p| p.queues = found.clone())?;
    restart_queue_if_running(&app, &path)?;
    Ok(found)
}

/// Grava a lista editada pela pessoa (`a, b, c`).
#[tauri::command]
pub async fn queues_set(app: AppHandle, path: PathBuf, text: String) -> CmdResult<Vec<String>> {
    let list = larafim_core::queues::parse_list(&text).map_err(|bad| t!("nome de fila inválido: {a}", a = bad.escape_debug()))?;
    let list = if list.is_empty() { vec!["default".to_string()] } else { list };
    if list.len() > larafim_core::queues::MAX_QUEUES {
        return Err(t!("nome de fila inválido"));
    }
    update_project(&app, &path, |p| p.queues = list.clone())?;
    restart_queue_if_running(&app, &path)?;
    Ok(list)
}

// ================= Reverb =================

/// Portas que o Reverb deste projeto não pode usar: as dos outros projetos
/// (mesmo offline, para cada um manter a sua) e as dos serviços do Larafim.
fn reverb_reserved(app: &AppHandle, except: &Path) -> Vec<u16> {
    let state = app.state::<AppState>();
    let c = state.config.lock().unwrap().clone();
    let mut ports = vec![
        c.http_port,
        c.https_port,
        c.db_port,
        c.pg_port,
        MAIL_UI_PORT,
        MAIL_SMTP_PORT,
        crate::s3::S3_API_PORT,
        crate::s3::S3_CONSOLE_PORT,
    ];
    ports.extend(c.redis.as_ref().map(|r| r.port));
    for p in state.projects.lock().unwrap().projects.iter().filter(|p| !same_path(&p.path, except)) {
        ports.extend(p.reverb_port);
        ports.extend(p.db_port);
    }
    ports
}

/// Porta do Reverb do projeto: mantém a atual enquanto servir; senão, a
/// primeira livre da faixa.
fn reverb_port_for(app: &AppHandle, path: &Path) -> Result<u16, String> {
    let current = app.state::<AppState>().projects.lock().unwrap().find(path).and_then(|p| p.reverb_port);
    let reserved = reverb_reserved(app, path);
    let ours = app.state::<ServerState>().dev.is_running(&worker_key(path, "reverb"));
    if let Some(p) = current
        && reverb::PORTS.contains(&p)
        && !reserved.contains(&p)
        && (ours || larafim_server::port::check_free(p).is_ok())
    {
        return Ok(p);
    }
    reverb::pick_port(&reserved, |p| larafim_server::port::check_free(p).is_ok()).ok_or_else(|| {
        t!("nenhuma porta livre entre {a} e {b} para o Reverb", a = reverb::PORTS.start(), b = reverb::PORTS.end())
    })
}

/// Endereço do Reverb para o navegador: o domínio do projeto no gateway
/// (HTTPS, se estiver ligado).
fn reverb_public(app: &AppHandle, path: &Path) -> Result<reverb::Public, String> {
    let state = app.state::<AppState>();
    let c = state.config.lock().unwrap().clone();
    let domain = state.projects.lock().unwrap().find(path).map(|p| p.domain.clone()).ok_or_else(|| t!("importe o projeto antes"))?;
    Ok(reverb::Public {
        host: format!("{domain}.{}", c.tld),
        port: if c.https { c.https_port } else { c.http_port },
        https: c.https,
    })
}

fn reverb_credentials() -> reverb::Credentials {
    const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    reverb::Credentials {
        app_id: crate::s3::random_token(b"123456789", 6),
        app_key: crate::s3::random_token(ALNUM, 20),
        app_secret: crate::s3::random_token(ALNUM, 20),
    }
}

#[derive(Serialize)]
pub struct ReverbPreview {
    installed: bool,
    port: u16,
    diff: Vec<DiffLine>,
    /// Não há .env: será criado a partir do .env.example.
    from_example: bool,
    /// Não há .env nem .env.example: só a porta muda.
    no_env: bool,
}

/// O que ligar o Reverb faria: porta escolhida e mudanças no `.env`.
#[tauri::command]
pub fn reverb_preview(app: AppHandle, path: PathBuf) -> CmdResult<ReverbPreview> {
    let env = envfile::env_path(&path);
    let example = path.join(".env.example");
    if !reverb::is_installed(&path) {
        return Ok(ReverbPreview { installed: false, port: 0, diff: Vec::new(), from_example: false, no_env: false });
    }
    let port = reverb_port_for(&app, &path)?;
    let base = std::fs::read_to_string(&env).or_else(|_| std::fs::read_to_string(&example)).unwrap_or_default();
    // Credenciais novas só aparecem como "gerada": o valor real é criado ao aplicar.
    let generated = t!("(gerada)");
    let fake = reverb::Credentials { app_id: generated.clone(), app_key: generated.clone(), app_secret: generated };
    let changes = reverb::env_changes(&base, port, &reverb_public(&app, &path)?, &fake);
    envfile::validate(&changes).map_err(|e| e.to_string())?;
    Ok(ReverbPreview {
        installed: true,
        port,
        diff: envfile::apply(&base, &changes).1,
        from_example: !env.exists() && example.exists(),
        no_env: !env.exists() && !example.exists(),
    })
}

/// Liga o Reverb do projeto: escolhe a porta, ajusta o `.env` (com cópia),
/// marca para ligar junto com o projeto e já inicia.
async fn reverb_enable(app: &AppHandle, path: &Path) -> Result<(), String> {
    let view = view_of(app, path)?;
    if view.kind != ProjectKind::Laravel {
        return Err(t!("workers só existem em projetos Laravel"));
    }
    if !reverb::is_installed(path) {
        return Err(t!("o Reverb não está instalado neste projeto (php artisan install:broadcasting)"));
    }
    let port = reverb_port_for(app, path)?;
    let public = reverb_public(app, path)?;
    let env = envfile::env_path(path);
    let example = path.join(".env.example");
    if !env.exists() && example.exists() {
        std::fs::copy(&example, &env).map_err(|e| e.to_string())?;
    }
    if env.exists() {
        let text = std::fs::read_to_string(&env).map_err(|e| e.to_string())?;
        envfile::write(path, &reverb::env_changes(&text, port, &public, &reverb_credentials())).map_err(|e| e.to_string())?;
        clear_config_cache(app, path);
        // A fila também publica eventos: precisa ler o BROADCAST_* novo.
        restart_queue_if_running(app, path)?;
    }
    update_project(app, path, |p| {
        p.reverb_port = Some(port);
        if !p.workers.iter().any(|w| w == "reverb") {
            p.workers.push("reverb".into());
        }
    })?;
    // O `reverb:start` lê o cache (que costuma estar no banco): com o projeto
    // offline, o banco dele pode estar desligado.
    crate::server::start_project_db(app, &view).await?;
    tracing::info!(path = %path.display(), port, "Reverb do projeto ligado");
    start_worker(app, &view_of(app, path)?, "reverb")?;
    // Rotas novas: o gateway passa a repassar o WebSocket para a porta.
    crate::server::after_change(app, false);
    Ok(())
}

// ================= Xdebug =================

#[derive(Serialize)]
pub struct XdebugInfo {
    downloaded: bool,
    enabled: bool,
    launch_json: String,
}

fn installed_php(app: &AppHandle, version: &str) -> Result<rphp::InstalledPhp, String> {
    rphp::list_installed(&app.state::<AppState>().paths.php_dir())
        .into_iter()
        .find(|p| p.version == version)
        .ok_or_else(|| t!("o PHP {version} não está instalado", version = version))
}

#[tauri::command]
pub fn xdebug_info(app: AppHandle, version: String) -> CmdResult<XdebugInfo> {
    let php = installed_php(&app, &version)?;
    let s = tools::xdebug_state(&php);
    Ok(XdebugInfo { downloaded: s.downloaded, enabled: s.enabled, launch_json: tools::vscode_launch(None) })
}

/// Liga/desliga o Xdebug numa versão do PHP. Os processos do PHP dessa
/// versão são reiniciados para valer na próxima requisição.
#[tauri::command]
pub async fn xdebug_set(app: AppHandle, version: String, enabled: bool) -> CmdResult<XdebugInfo> {
    let php = installed_php(&app, &version)?;
    if enabled && !tools::xdebug_state(&php).downloaded {
        let client = http_client().map_err(|e| e.to_string())?;
        tools::ensure_xdebug(&client, &php).await.map_err(|e| e.to_string())?;
    }
    tools::set_xdebug(&php, enabled).map_err(|e| e.to_string())?;
    app.state::<ServerState>().restart_php().await;
    tracing::info!(version, enabled, "Xdebug alterado");
    xdebug_info(app, version)
}

// ================= Projeto Laravel novo =================

#[derive(Deserialize)]
pub struct NewLaravel {
    name: String,
    root: PathBuf,
    php: String,
    /// "none", "react", "vue", "livewire"
    kit: String,
    /// "sqlite" ou "mariadb"
    database: String,
}

fn kit_package(kit: &str) -> Option<&'static str> {
    match kit {
        "none" => Some("laravel/laravel"),
        "react" => Some("laravel/react-starter-kit"),
        "vue" => Some("laravel/vue-starter-kit"),
        "livewire" => Some("laravel/livewire-starter-kit"),
        _ => None,
    }
}

/// Cria o projeto com `composer create-project` (e `npm install` + build
/// nos starter kits). A saída aparece ao vivo; no fim o projeto é importado.
#[tauri::command]
pub async fn laravel_create(app: AppHandle, input: NewLaravel) -> CmdResult<String> {
    let name = input.name.trim().to_lowercase();
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(t!("use só letras, números, - e _ no nome"));
    }
    let package = kit_package(&input.kit).ok_or_else(|| t!("starter kit desconhecido"))?;
    if !input.root.is_dir() {
        return Err(t!("a pasta escolhida não existe"));
    }
    let target = input.root.join(&name);
    if target.exists() {
        return Err(t!("já existe uma pasta {name} ali", name = name));
    }
    let paths = app.state::<AppState>().paths.clone();
    if !rphp::list_installed(&paths.php_dir()).iter().any(|p| p.minor == input.php) {
        return Err(t!("o PHP {a} não está instalado", a = input.php));
    }
    if !larafim_runtimes::extras::composer_path(&paths).is_file() {
        return Err(t!("o Composer ainda não foi baixado (instale um PHP pelo Larafim)"));
    }
    if input.kit != "none" && larafim_runtimes::node::list_installed(&paths.node_dir()).is_empty() {
        return Err(t!("os starter kits precisam do Node.js: instale em Configurações › Node.js"));
    }
    // Os atalhos (php, composer, npm) escolhem as versões; LARAFIM_PHP fixa o PHP.
    crate::terminal::ensure_shims(&app)?;
    let mut command = format!("composer create-project {package} \"{name}\" --no-interaction --prefer-dist");
    if input.kit != "none" {
        command.push_str(&format!(" && cd \"{name}\" && npm install && npm run build"));
    }
    let key = format!("create:{}", target.display().to_string().to_lowercase());
    app.state::<ToolsState>().creating.lock().unwrap().insert(
        key.clone(),
        CreateJob { path: target.clone(), php: input.php.clone(), database: input.database.clone() },
    );
    app.state::<ServerState>()
        .dev
        .start(DevSpec {
            key: key.clone(),
            cwd: input.root.clone(),
            command,
            node_dir: paths.bin_dir(),
            env: vec![("LARAFIM_PHP".into(), input.php.clone()), ("COMPOSER_NO_INTERACTION".into(), "1".into())],
        })
        .map_err(|e| e.to_string())?;
    tracing::info!(name, kit = input.kit, "criando projeto Laravel");
    Ok(key)
}

#[derive(Serialize, Clone)]
struct Created {
    key: String,
    path: PathBuf,
    ok: bool,
    error: Option<String>,
}

/// Chamado quando um processo termina: se era uma criação, importa o projeto.
pub fn on_process_exit(app: &AppHandle, key: &str, code: Option<i32>) {
    let Some(job) = app.state::<ToolsState>().creating.lock().unwrap().remove(key) else { return };
    let app = app.clone();
    let key = key.to_string();
    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            if code != Some(0) || !job.path.join("artisan").is_file() {
                return Err(match code {
                    Some(c) => t!("a criação falhou (código {c}). Veja a saída.", c = c),
                    None => t!("a criação foi interrompida. Veja a saída."),
                });
            }
            {
                let state = app.state::<AppState>();
                let mut store = state.projects.lock().unwrap();
                if store.find(&job.path).is_none() {
                    store.import(&job.path).map_err(|e| e.to_string())?;
                }
                if let Some(p) = store.projects.iter_mut().find(|p| same_path(&p.path, &job.path)) {
                    p.php = Some(job.php.clone());
                }
                store.save(&state.paths.projects_file()).map_err(|e| e.to_string())?;
            }
            notify_projects(&app);
            if job.database == "mariadb" {
                crate::services::apply_db(&app, &job.path, "mariadb").await?;
            }
            Ok(())
        }
        .await;
        let _ = app.emit(
            "laravel:created",
            Created { key, path: job.path.clone(), ok: result.is_ok(), error: result.err() },
        );
    });
}
