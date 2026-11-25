//! App desktop do Larafim (Tauri 2).
//!
//! Leveza: janelas são **destruídas** ao fechar, liberando o WebView da
//! memória. Com "continuar na bandeja" ligado, o processo segue vivo só com o
//! ícone da bandeja e recria a janela quando a pessoa pede.

mod commands;
mod node;
mod php;
mod projects;
mod server;
mod dbconn;
mod secret;
mod autostart;
mod https;
mod language;
mod packaged;
mod pg;
mod redis;
mod s3;
mod service_info;
mod services;
mod terminal;
mod tools;
mod tray;
mod tray_panel;
mod updates;

use std::sync::Mutex;

use larafim_core::{AppConfig, Paths, ProjectsFile, Theme};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_WINDOW: &str = "settings";
pub const LOGS_WINDOW: &str = "logs";

/// Argumentos do WebView2. Os 3 primeiros são os padrões do Tauri
/// (substituídos ao definir args). Sem GPU e com um só processo de
/// renderização a interface gasta bem menos memória.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --disable-gpu --disable-gpu-compositing --renderer-process-limit=1 \
    --disable-background-networking --disable-component-update --no-pings";

/// Estado compartilhado do app.
pub struct AppState {
    pub paths: Paths,
    pub config: Mutex<AppConfig>,
    pub projects: Mutex<ProjectsFile>,
}

pub fn run() {
    let paths = Paths::default_root().expect("pasta de dados indisponível");
    paths.ensure().expect("não foi possível criar a pasta de dados do Larafim");
    let _log_guard = larafim_core::logging::init(&paths.logs_dir());

    let mut config = AppConfig::load(&paths.config_file()).unwrap_or_else(|e| {
        tracing::error!("configuração ilegível, usando padrão: {e}");
        AppConfig::default()
    });
    // Idioma: o da configuração, ou o escolhido no instalador, ou o do Windows.
    let (language, decided_now) = language::resolve(config.language.as_deref());
    larafim_core::i18n::set_language(&language);
    if decided_now {
        config.language = Some(language.clone());
        if let Err(e) = config.save(&paths.config_file()) {
            tracing::warn!("idioma não gravado: {e}");
        }
    }
    tracing::info!(language, "idioma");
    let projects = ProjectsFile::load(&paths.projects_file()).unwrap_or_else(|e| {
        tracing::error!("projects.toml ilegível, começando vazio: {e}");
        ProjectsFile::default()
    });
    tracing::info!(version = env!("CARGO_PKG_VERSION"), root = %paths.root().display(), "Larafim iniciando");

    let app = tauri::Builder::default()
        // Abrir o Larafim de novo só traz a janela existente para frente.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Atualizações do próprio Larafim; só usado pelo Rust (updates.rs),
        // sem permissão para a interface chamar o plugin direto.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState { paths, config: Mutex::new(config), projects: Mutex::new(projects) })
        .manage(php::PhpState::default())
        .manage(node::NodeState::default())
        .manage(server::ServerState::new())
        .manage(services::ServicesState::default())
        .manage(pg::PgState::default())
        .manage(tools::ToolsState::default())
        .manage(updates::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            service_info::service_info,
            commands::save_config,
            commands::app_info,
            commands::open_data_dir,
            commands::open_folder,
            commands::open_settings,
            commands::open_logs,
            tray_panel::tray_fit,
            updates::app_update_status,
            updates::app_update_check,
            updates::app_update_install,
            updates::open_project_link,
            tray_panel::tray_action,
            projects::list_projects,
            projects::import_project,
            projects::import_all,
            projects::save_project,
            projects::set_project_aliases,
            projects::remove_project,
            projects::ignore_project,
            projects::unignore_project,
            projects::inspect_folder,
            projects::project_docroots,
            php::php_overview,
            php::php_install,
            php::php_cancel,
            php::php_remove,
            php::php_extensions,
            php::php_set_extension,
            php::php_deprecations,
            php::php_set_deprecations,
            php::php_open_ini,
            php::php_open_folder,
            php::php_match,
            php::open_vc_redist,
            server::set_project_online,
            server::server_status,
            server::stop_all_projects,
            server::fix_hosts,
            server::open_url,
            server::dev_info,
            services::services_overview,
            services::maria_install,
            services::maria_remove,
            services::maria_start,
            services::maria_stop,
            pg::pg_overview,
            pg::pg_install,
            pg::pg_remove,
            pg::pg_start,
            pg::pg_stop,
            pg::pg_configure,
            https::https_status,
            https::https_configure,
            https::https_trust,
            https::https_untrust,
            https::https_open_ca_folder,
            s3::s3_status,
            s3::s3_install,
            s3::s3_start,
            s3::s3_stop,
            s3::open_s3,
            s3::project_storage_preview,
            s3::project_storage_apply,
            services::maria_configure,
            services::project_db_preview,
            services::project_db_apply,
            services::project_env_undo,
            services::project_env_has_backup,
            services::db_backups,
            services::db_backup,
            services::db_restore,
            services::open_backups_folder,
            services::open_adminer,
            tools::mailpit_status,
            tools::mailpit_install,
            tools::mailpit_start,
            tools::mailpit_stop,
            tools::open_mailpit,
            tools::project_mail_preview,
            tools::project_mail_apply,
            tools::log_files,
            tools::log_read,
            tools::log_clear,
            tools::log_open,
            tools::worker_set,
            tools::worker_restart,
            tools::process_info,
            tools::has_reverb,
            tools::project_install_reverb,
            tools::reverb_preview,
            tools::queues_discover,
            tools::queues_set,
            redis::redis_status,
            redis::redis_install,
            redis::redis_configure,
            redis::redis_remove,
            redis::redis_start,
            redis::redis_stop,
            redis::project_queue_redis_preview,
            redis::project_queue_connection,
            redis::project_install_predis,
            tools::xdebug_info,
            tools::xdebug_set,
            tools::project_run,
            tools::laravel_templates,
            tools::laravel_create,
            services::suggest_own_port,
            services::own_start,
            services::own_stop,
            dbconn::db_connections,
            dbconn::db_connection_test,
            dbconn::db_connection_save,
            dbconn::db_connection_remove,
            dbconn::db_detect,
            server::restart_dev,
            node::node_overview,
            node::node_install,
            node::node_cancel,
            node::node_remove,
            node::node_installed,
            terminal::terminal_status,
            terminal::install_shims,
            terminal::set_shims_in_path,
            terminal::php_for_folder,
            terminal::open_terminal,
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            // Antes do servidor: php.ini com caminhos da pasta antiga não carrega extensão nenhuma.
            let fixed = larafim_runtimes::php::installed::repair_moved(&app.state::<AppState>().paths.php_dir());
            if !fixed.is_empty() {
                tracing::info!(?fixed, "php.ini apontava para outra pasta; caminhos corrigidos");
            }
            php::start_update_checker(app.handle().clone());
            updates::start_checker(app.handle().clone());
            server::start(app.handle());
            terminal::refresh_on_start(app.handle());
            let autostart = app.state::<AppState>().config.lock().map(|c| c.start_with_windows).unwrap_or(false);
            autostart::sync(autostart);
            // Aberto pelo Windows ao entrar: fica só na bandeja.
            if !autostart::started_in_tray() {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o Larafim");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            server::shutdown(app);
            return;
        }
        if let RunEvent::ExitRequested { code, api, .. } = event {
            // `code == None`: a última janela foi fechada (não foi um "Sair").
            if code.is_none() && keep_in_tray(app) {
                api.prevent_exit();
                tracing::info!("janelas fechadas; Larafim continua na bandeja");
            }
        }
    });
}

/// Troca o idioma do backend: mensagens novas, menu da bandeja e título
/// das janelas abertas (a interface troca sozinha pelo evento config-changed).
pub(crate) fn language_changed(app: &AppHandle, code: &str) {
    larafim_core::i18n::set_language(code);
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = w.set_title(&larafim_core::t!("Configurações — Larafim"));
    }
    if let Some(w) = app.get_webview_window(LOGS_WINDOW) {
        let _ = w.set_title(&larafim_core::t!("Logs — Larafim"));
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let s = server::status(&app).await;
        tray::update_online(&app, &s.online);
    });
    tracing::info!(code, "idioma trocado");
}

fn keep_in_tray(app: &AppHandle) -> bool {
    app.state::<AppState>().config.lock().map(|c| c.keep_in_tray).unwrap_or(true)
}

fn native_theme(app: &AppHandle) -> Option<tauri::Theme> {
    match app.state::<AppState>().config.lock().map(|c| c.theme) {
        Ok(Theme::Dark) => Some(tauri::Theme::Dark),
        Ok(Theme::Light) => Some(tauri::Theme::Light),
        _ => None,
    }
}

/// Traz uma janela para frente se ela existir. Devolve `false` se não existir.
fn focus_existing(app: &AppHandle, label: &str) -> bool {
    match app.get_webview_window(label) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            true
        }
        None => false,
    }
}

/// Janela sem moldura do Windows: a barra de título é desenhada pela interface.
fn window_builder<'a>(
    app: &'a AppHandle,
    label: &'a str,
    title: &str,
    size: (f64, f64),
    min: (f64, f64),
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(min.0, min.1)
        .decorations(false)
        .shadow(true)
        .center()
        .theme(native_theme(app))
        .additional_browser_args(BROWSER_ARGS)
}

/// Mostra a janela principal, criando-a se tiver sido descarregada.
pub fn show_main_window(app: &AppHandle) {
    if focus_existing(app, MAIN_WINDOW) {
        return;
    }
    let result = window_builder(app, MAIN_WINDOW, "Larafim", (1120.0, 720.0), (860.0, 520.0)).build();
    if let Err(e) = result {
        tracing::error!("não foi possível abrir a janela principal: {e}");
    }
}

/// Abre a janela principal já com o diálogo de novo projeto Laravel.
pub fn show_main_window_new_laravel(app: &AppHandle) {
    if focus_existing(app, MAIN_WINDOW) {
        let _ = app.emit_to(MAIN_WINDOW, "main:new-laravel", ());
        return;
    }
    let result = window_builder(app, MAIN_WINDOW, "Larafim", (1120.0, 720.0), (860.0, 520.0))
        .initialization_script("window.__LARAFIM_NEW_LARAVEL__ = true;")
        .build();
    if let Err(e) = result {
        tracing::error!("não foi possível abrir a janela principal: {e}");
    }
}

/// Janela de painel (bandeja): sem moldura, fundo translúcido do Windows 11.
fn panel_builder<'a>(app: &'a AppHandle, label: &'a str, size: (f64, f64)) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    use tauri::window::{Effect, EffectsBuilder};
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title("Larafim")
        .inner_size(size.0, size.1)
        .decorations(false)
        .shadow(true)
        .transparent(true)
        .effects(EffectsBuilder::new().effect(Effect::Acrylic).build())
        .theme(native_theme(app))
        .additional_browser_args(BROWSER_ARGS)
}

/// Abre (ou traz para frente) o visualizador de logs, já num projeto se vier um.
pub fn show_logs_window(app: &AppHandle, project: Option<&str>) {
    if focus_existing(app, LOGS_WINDOW) {
        if let Some(p) = project {
            let _ = app.emit_to(LOGS_WINDOW, "logs:navigate", p);
        }
        return;
    }
    let init = format!(
        "window.__LARAFIM_LOGS_PROJECT__ = {};",
        serde_json::to_string(&project).unwrap_or_else(|_| "null".into())
    );
    let result = window_builder(app, LOGS_WINDOW, &larafim_core::t!("Logs — Larafim"), (1100.0, 700.0), (720.0, 460.0))
        .initialization_script(&init)
        .build();
    if let Err(e) = result {
        tracing::error!("não foi possível abrir os logs: {e}");
    }
}

/// Abre (ou traz para frente) a janela de Configurações numa seção.
pub fn show_settings_window(app: &AppHandle, section: &str) {
    if focus_existing(app, SETTINGS_WINDOW) {
        let _ = app.emit_to(SETTINGS_WINDOW, "settings:navigate", section);
        return;
    }
    // A seção inicial é entregue antes da página carregar.
    let init = format!(
        "window.__LARAFIM_SECTION__ = {};",
        serde_json::to_string(section).unwrap_or_else(|_| "\"general\"".into())
    );
    let result = window_builder(app, SETTINGS_WINDOW, &larafim_core::t!("Configurações — Larafim"), (1040.0, 680.0), (760.0, 480.0))
        .initialization_script(&init)
        .build();
    if let Err(e) = result {
        tracing::error!("não foi possível abrir as configurações: {e}");
    }
}
