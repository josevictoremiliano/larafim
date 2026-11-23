//! Comandos de projetos: listar, importar, editar, remover e ignorar.
//! Tudo fica em `projects.toml`; nada é escrito nas pastas dos projetos.

use larafim_core::t;
use std::path::PathBuf;

use larafim_core::projects::{detect_any, docroot_options};
use larafim_core::{ProjectConfig, ProjectView, ProjectsFile};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Serialize, Clone)]
pub struct ProjectsOverview {
    projects: Vec<ProjectView>,
    ignored: Vec<PathBuf>,
}

fn roots(state: &AppState) -> CmdResult<Vec<PathBuf>> {
    Ok(state.config.lock().map_err(|e| e.to_string())?.roots.clone())
}

fn overview(state: &AppState) -> CmdResult<ProjectsOverview> {
    let roots = roots(state)?;
    let store = state.projects.lock().map_err(|e| e.to_string())?;
    Ok(ProjectsOverview { projects: store.views(&roots), ignored: store.ignored.clone() })
}

/// Aplica uma mudança no cadastro, grava em disco e avisa as janelas.
/// Se gravar falhar, o cadastro em memória volta ao que era.
fn change(
    app: &AppHandle,
    state: &AppState,
    f: impl FnOnce(&mut ProjectsFile, &[PathBuf]) -> larafim_core::Result<()>,
) -> CmdResult<ProjectsOverview> {
    let roots = roots(state)?;
    {
        let mut store = state.projects.lock().map_err(|e| e.to_string())?;
        let backup = store.clone();
        let result = f(&mut store, &roots).and_then(|_| store.save(&state.paths.projects_file()));
        if let Err(e) = result {
            *store = backup;
            return Err(e.to_string());
        }
    }
    let data = overview(state)?;
    let _ = app.emit("projects-changed", &data);
    Ok(data)
}

/// Lista detectados + importados. Só lê arquivos; não liga nada.
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> CmdResult<ProjectsOverview> {
    // A varredura toca o disco: roda fora da thread da interface.
    let state = state.inner();
    let roots = roots(state)?;
    let store = state.projects.lock().map_err(|e| e.to_string())?.clone();
    let ignored = store.ignored.clone();
    let projects = tauri::async_runtime::spawn_blocking(move || store.views(&roots))
        .await
        .map_err(|e| e.to_string())?;
    Ok(ProjectsOverview { projects, ignored })
}

/// Define os domínios extras (aliases) do projeto e atualiza rotas e hosts.
#[tauri::command]
pub fn set_project_aliases(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    aliases: Vec<String>,
) -> CmdResult<ProjectsOverview> {
    let tld = state.config.lock().map_err(|e| e.to_string())?.tld.clone();
    let data = change(&app, &state, |store, _| store.set_aliases(&path, &aliases, &tld))?;
    crate::server::after_change(&app, true);
    Ok(data)
}

#[tauri::command]
pub fn import_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<ProjectsOverview> {
    change(&app, &state, |store, _| store.import(&path).map(|domain| tracing::info!(domain, "projeto importado")))
}

#[tauri::command]
pub fn import_all(app: AppHandle, state: State<'_, AppState>) -> CmdResult<ProjectsOverview> {
    change(&app, &state, |store, roots| {
        let new: Vec<PathBuf> = store
            .views(roots)
            .into_iter()
            .filter(|v| v.status == larafim_core::ProjectStatus::New)
            .map(|v| v.path)
            .collect();
        for path in new {
            store.import(&path)?;
        }
        Ok(())
    })
}

#[tauri::command]
pub fn save_project(app: AppHandle, state: State<'_, AppState>, config: ProjectConfig) -> CmdResult<ProjectsOverview> {
    // `online` é controlado só pelo interruptor (set_project_online).
    let mut config = config;
    // `online` e `database` são controlados pelos próprios comandos.
    {
        let store = state.projects.lock().map_err(|e| e.to_string())?;
        let existing = store.find(&config.path);
        config.online = existing.is_some_and(|p| p.online);
        config.database = existing.and_then(|p| p.database.clone());
        config.db_version = existing.and_then(|p| p.db_version.clone());
        config.db_port = existing.and_then(|p| p.db_port);
        config.mail = existing.and_then(|p| p.mail.clone());
        config.storage = existing.and_then(|p| p.storage.clone());
        config.workers = existing.map(|p| p.workers.clone()).unwrap_or_default();
        config.reverb_port = existing.and_then(|p| p.reverb_port);
        config.queues = existing.map(|p| p.queues.clone()).unwrap_or_default();
        config.queue_connection = existing.and_then(|p| p.queue_connection.clone());
        config.queue_redis_env = existing.and_then(|p| p.queue_redis_env.clone());
        // A interface ainda não edita aliases: preserve os do projects.toml.
        if config.aliases.is_empty() {
            config.aliases = existing.map(|p| p.aliases.clone()).unwrap_or_default();
        }
    }
    let data = change(&app, &state, |store, _| store.upsert(config))?;
    crate::server::after_change(&app, true);
    Ok(data)
}

/// Tira do Larafim. Os arquivos do projeto não são apagados.
#[tauri::command]
pub fn remove_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<ProjectsOverview> {
    // Projeto fora da lista não pode deixar processos (Vite, filas, Reverb) para trás.
    tauri::Manager::state::<crate::server::ServerState>(&app).dev.stop(&crate::server::dev_key(&path));
    crate::tools::stop_workers(&app, &path);
    let data = change(&app, &state, |store, roots| {
        store.remove(&path, roots);
        Ok(())
    })?;
    crate::server::after_change(&app, false);
    Ok(data)
}

#[tauri::command]
pub fn ignore_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<ProjectsOverview> {
    change(&app, &state, |store, _| {
        store.ignore(&path);
        Ok(())
    })
}

#[tauri::command]
pub fn unignore_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<ProjectsOverview> {
    change(&app, &state, |store, _| {
        store.unignore(&path);
        Ok(())
    })
}

/// Para o diálogo de projeto avulso: o que o Larafim entende da pasta.
#[derive(Serialize)]
pub struct FolderProposal {
    config: ProjectConfig,
    view_kind: larafim_core::ProjectKind,
    recognized: bool,
    php_request: Option<String>,
    has_node: bool,
    docroots: Vec<String>,
    already_imported: bool,
}

#[tauri::command]
pub fn inspect_folder(state: State<'_, AppState>, path: PathBuf) -> CmdResult<FolderProposal> {
    let path = larafim_core::projects::project_root(&path);
    let d = detect_any(&path).ok_or_else(|| t!("a pasta {a} não existe", a = path.display()))?;
    let parent = path.parent().map(PathBuf::from).unwrap_or_default();
    let recognized = larafim_core::projects::detect(&path, &parent).is_some();
    let store = state.projects.lock().map_err(|e| e.to_string())?;
    let used: Vec<String> = store.projects.iter().map(|p| p.domain.clone()).collect();
    Ok(FolderProposal {
        config: ProjectConfig {
            path: path.clone(),
            domain: larafim_core::store::unique_domain(&d.domain, &used),
            aliases: Vec::new(),
            php: None,
            docroot: d.docroot.clone(),
            node: d.kind == larafim_core::ProjectKind::Node && d.node_command.is_some(),
            node_command: d.node_command.clone(),
            node_version: None,
            database: None,
            db_version: None,
            db_port: None,
            mail: None,
            storage: None,
            workers: Vec::new(),
            reverb_port: None,
            queues: Vec::new(),
            queue_connection: None,
            queue_redis_env: None,
            online: false,
        },
        view_kind: d.kind,
        recognized,
        php_request: d.php,
        has_node: d.has_node,
        docroots: docroot_options(&path),
        already_imported: store.find(&path).is_some(),
    })
}

#[tauri::command]
pub fn project_docroots(path: PathBuf) -> Vec<String> {
    docroot_options(&path)
}
