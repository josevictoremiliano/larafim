//! Comandos de PHP: catálogo, instalar/atualizar/remover, extensões e
//! verificação de atualizações em segundo plano.

use larafim_core::t;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use larafim_runtimes::php::{self, CatalogEntry, InstalledPhp, Stage, ini};
use larafim_runtimes::{extras, http_client};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

const CATALOG_TTL: Duration = Duration::from_secs(30 * 60);
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

/// Estado do PHP: catálogo em cache e instalações em andamento.
#[derive(Default)]
pub struct PhpState {
    catalog: Mutex<Option<(Instant, Vec<CatalogEntry>)>>,
    /// minor → sinal de cancelamento
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// Uma linha da tabela de versões (uma por minor).
#[derive(Serialize)]
pub struct PhpRow {
    minor: String,
    /// Build instalada em uso (a mais nova do minor).
    installed: Option<String>,
    /// Build anterior guardada para voltar.
    previous: Option<String>,
    /// Última build do catálogo.
    latest: Option<String>,
    size: Option<String>,
    update_available: bool,
    installing: bool,
}

#[derive(Serialize)]
pub struct PhpOverview {
    rows: Vec<PhpRow>,
    composer: bool,
    /// Se o catálogo não pôde ser lido (sem internet), a lista mostra só o instalado.
    catalog_error: Option<String>,
    updates: usize,
}

async fn catalog(state: &PhpState, force: bool) -> Result<Vec<CatalogEntry>, String> {
    if !force
        && let Some((at, entries)) = state.catalog.lock().unwrap().as_ref()
        && at.elapsed() < CATALOG_TTL
    {
        return Ok(entries.clone());
    }
    let client = http_client().map_err(|e| e.to_string())?;
    let entries = php::fetch_catalog(&client).await.map_err(|e| e.to_string())?;
    *state.catalog.lock().unwrap() = Some((Instant::now(), entries.clone()));
    Ok(entries)
}

fn installed(app: &AppState) -> Vec<InstalledPhp> {
    php::list_installed(&app.paths.php_dir())
}

fn build_overview(app: &AppState, php_state: &PhpState, catalog: Result<Vec<CatalogEntry>, String>) -> PhpOverview {
    let installed = installed(app);
    let running = php_state.running.lock().unwrap();
    let (entries, catalog_error) = match catalog {
        Ok(e) => (e, None),
        Err(e) => (Vec::new(), Some(e)),
    };

    let mut minors: Vec<String> = entries.iter().map(|e| e.minor.clone()).collect();
    for p in &installed {
        if !minors.contains(&p.minor) {
            minors.push(p.minor.clone());
        }
    }
    minors.sort_by(|a, b| php::parse_version(&format!("{b}.0")).cmp(&php::parse_version(&format!("{a}.0"))));

    let rows: Vec<PhpRow> = minors
        .into_iter()
        .map(|minor| {
            let mut builds = installed.iter().filter(|p| p.minor == minor);
            let current = builds.next().map(|p| p.version.clone());
            let previous = builds.next().map(|p| p.version.clone());
            let entry = entries.iter().find(|e| e.minor == minor);
            let update_available = match (&current, entry) {
                (Some(cur), Some(e)) => php::parse_version(&e.version) > php::parse_version(cur),
                _ => false,
            };
            PhpRow {
                installing: running.contains_key(&minor),
                latest: entry.map(|e| e.version.clone()),
                size: entry.map(|e| e.size.clone()),
                minor,
                installed: current,
                previous,
                update_available,
            }
        })
        .collect();

    PhpOverview {
        updates: rows.iter().filter(|r| r.update_available).count(),
        composer: extras::composer_path(&app.paths).is_file(),
        catalog_error,
        rows,
    }
}

#[tauri::command]
pub async fn php_overview(
    app: State<'_, AppState>,
    php_state: State<'_, PhpState>,
    refresh: Option<bool>,
) -> CmdResult<PhpOverview> {
    let cat = catalog(&php_state, refresh.unwrap_or(false)).await;
    Ok(build_overview(&app, &php_state, cat))
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    minor: String,
    #[serde(flatten)]
    stage: Stage,
}

#[derive(Clone, Serialize)]
struct DoneEvent {
    minor: String,
    ok: bool,
    version: Option<String>,
    error: Option<String>,
    /// "vc_redist", "cancelled", "network" ou "error".
    code: Option<String>,
}

/// Começa a instalar (ou atualizar) um minor. Volta na hora; o progresso
/// chega pelos eventos `php:progress` e `php:done`.
#[tauri::command]
pub async fn php_install(app: AppHandle, php_state: State<'_, PhpState>, minor: String) -> CmdResult<()> {
    let entry = catalog(&php_state, false)
        .await?
        .into_iter()
        .find(|e| e.minor == minor)
        .ok_or_else(|| t!("a versão {minor} não está no catálogo", minor = minor))?;

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = php_state.running.lock().unwrap();
        if running.contains_key(&minor) {
            return Err(t!("o PHP {minor} já está sendo instalado", minor = minor));
        }
        running.insert(minor.clone(), cancel.clone());
    }

    tauri::async_runtime::spawn(async move {
        let paths = app.state::<AppState>().paths.clone();
        let emitter = app.clone();
        let m = minor.clone();
        // Evita inundar a interface: no máximo ~10 eventos de download por segundo.
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let on_stage = move |stage: Stage| {
            if matches!(stage, Stage::Downloading { .. }) {
                let mut last = last.lock().unwrap();
                if last.elapsed() < Duration::from_millis(100) {
                    return;
                }
                *last = Instant::now();
            }
            let _ = emitter.emit("php:progress", ProgressEvent { minor: m.clone(), stage });
        };

        let result = match http_client() {
            Ok(client) => php::install(&client, &paths, &entry, &cancel, &on_stage).await,
            Err(e) => Err(e),
        };

        app.state::<PhpState>().running.lock().unwrap().remove(&minor);
        let event = match result {
            Ok(p) => DoneEvent { minor, ok: true, version: Some(p.version), error: None, code: None },
            Err(e) => {
                tracing::warn!("instalação do PHP falhou: {e}");
                DoneEvent { minor, ok: false, version: None, error: Some(e.to_string()), code: Some(e.code().into()) }
            }
        };
        let _ = app.emit("php:done", event);
        emit_updates(&app).await;
        crate::server::after_change(&app, false);
    });
    Ok(())
}

#[tauri::command]
pub fn php_cancel(php_state: State<'_, PhpState>, minor: String) {
    if let Some(flag) = php_state.running.lock().unwrap().get(&minor) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub async fn php_remove(app: AppHandle, state: State<'_, AppState>, php_state: State<'_, PhpState>, minor: String) -> CmdResult<()> {
    if php_state.running.lock().unwrap().contains_key(&minor) {
        return Err(t!("espere a instalação terminar"));
    }
    php::remove_minor(&state.paths, &minor).map_err(|e| e.to_string())?;
    emit_updates(&app).await;
    crate::server::after_change(&app, false);
    Ok(())
}

fn find_installed(state: &AppState, version: &str) -> CmdResult<InstalledPhp> {
    installed(state)
        .into_iter()
        .find(|p| p.version == version)
        .ok_or_else(|| t!("o PHP {version} não está instalado", version = version))
}

#[tauri::command]
pub fn php_extensions(state: State<'_, AppState>, version: String) -> CmdResult<Vec<ini::Extension>> {
    let p = find_installed(&state, &version)?;
    let text = ini::read(&p.ini()).map_err(|e| e.to_string())?;
    Ok(ini::list_extensions(&text, &p.dir.join("ext")))
}

#[tauri::command]
pub fn php_set_extension(
    state: State<'_, AppState>,
    version: String,
    name: String,
    enabled: bool,
) -> CmdResult<Vec<ini::Extension>> {
    let p = find_installed(&state, &version)?;
    let text = ini::read(&p.ini()).map_err(|e| e.to_string())?;
    let ext_dir = p.dir.join("ext");
    let current = ini::list_extensions(&text, &ext_dir);
    let ext = current
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| t!("extensão {name} não encontrada", name = name))?;
    let updated = ini::set_extension(&text, &ext.name, enabled, ext.zend);
    ini::write(&p.ini(), &updated).map_err(|e| e.to_string())?;
    tracing::info!(version, name, enabled, "extensão alterada");
    Ok(ini::list_extensions(&updated, &ext_dir))
}

/// Os avisos de obsolescência (`E_DEPRECATED`) aparecem nessa versão?
#[tauri::command]
pub fn php_deprecations(state: State<'_, AppState>, version: String) -> CmdResult<bool> {
    let p = find_installed(&state, &version)?;
    let text = ini::read(&p.ini()).map_err(|e| e.to_string())?;
    Ok(ini::deprecations_shown(&text))
}

/// Mostra ou esconde os avisos de obsolescência (só quando a pessoa pede:
/// o Larafim não mexe sozinho no `php.ini` de uma versão já instalada).
#[tauri::command]
pub fn php_set_deprecations(state: State<'_, AppState>, version: String, show: bool) -> CmdResult<bool> {
    let p = find_installed(&state, &version)?;
    let text = ini::read(&p.ini()).map_err(|e| e.to_string())?;
    let updated = ini::set_deprecations(&text, show);
    if updated != text {
        ini::write(&p.ini(), &updated).map_err(|e| e.to_string())?;
        tracing::info!(version, show, "avisos de obsolescência alterados");
    }
    Ok(ini::deprecations_shown(&updated))
}

/// Abre o `php.ini` no editor padrão do Windows.
#[tauri::command]
pub fn php_open_ini(app: AppHandle, state: State<'_, AppState>, version: String) -> CmdResult<()> {
    let p = find_installed(&state, &version)?;
    app.opener()
        .open_path(p.ini().display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn php_open_folder(app: AppHandle, state: State<'_, AppState>, version: String) -> CmdResult<()> {
    let p = find_installed(&state, &version)?;
    app.opener()
        .open_path(p.dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Baixa o instalador oficial do Visual C++ no navegador.
#[tauri::command]
pub fn open_vc_redist(app: AppHandle) -> CmdResult<()> {
    app.opener().open_url(VC_REDIST_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Qual PHP instalado atende ao que um projeto pede.
#[derive(Serialize)]
pub struct PhpMatch {
    /// Versão instalada escolhida (a mais nova compatível).
    installed: Option<String>,
    /// Minors instalados (para o seletor).
    available: Vec<String>,
    /// Minor do catálogo sugerido para instalar, se nada instalado servir.
    suggest: Option<String>,
}

#[tauri::command]
pub async fn php_match(
    state: State<'_, AppState>,
    php_state: State<'_, PhpState>,
    constraint: Option<String>,
    pinned: Option<String>,
) -> CmdResult<PhpMatch> {
    let list = installed(&state);
    let available: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        list.iter().filter(|p| seen.insert(p.minor.clone())).map(|p| p.minor.clone()).collect()
    };
    // Versão fixada no projeto: usa a build mais nova daquele minor.
    if let Some(minor) = pinned.filter(|m| !m.is_empty()) {
        let found = list.iter().find(|p| p.minor == minor).map(|p| p.version.clone());
        let suggest = found.is_none().then_some(minor);
        return Ok(PhpMatch { installed: found, available, suggest });
    }
    let constraint = constraint.unwrap_or_default();
    let installed_match = if constraint.is_empty() {
        list.first()
    } else {
        php::best_installed(&constraint, &list)
    };
    let suggest = if installed_match.is_none() {
        catalog(&php_state, false).await.ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|e| constraint.is_empty() || php::matches_constraint(&constraint, &e.version))
                .map(|e| e.minor)
        })
    } else {
        None
    };
    Ok(PhpMatch { installed: installed_match.map(|p| p.version.clone()), available, suggest })
}

/// Avisa as janelas quantas atualizações existem (badge na barra de ferramentas).
async fn emit_updates(app: &AppHandle) {
    let state = app.state::<AppState>();
    let php_state = app.state::<PhpState>();
    let cat = catalog(&php_state, false).await;
    let overview = build_overview(&state, &php_state, cat);
    let _ = app.emit("php:updates", overview.updates);
}

/// Confere atualizações logo após abrir e depois a cada 6 horas.
pub fn start_update_checker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            {
                let php_state = app.state::<PhpState>();
                match catalog(&php_state, true).await {
                    Ok(_) => emit_updates(&app).await,
                    Err(e) => tracing::info!("sem acesso ao catálogo do PHP agora: {e}"),
                }
            }
            tokio::time::sleep(UPDATE_CHECK_EVERY).await;
        }
    });
}
