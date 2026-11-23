//! Versão nova do Larafim: confere a última release no GitHub ao abrir e a
//! cada 6 horas (se a opção estiver ligada) e avisa as janelas. A pessoa
//! baixa pela página da release; o app não baixa nem executa nada sozinho.
//! Instalado pela Microsoft Store, quem atualiza é a Store: nada é consultado.

use std::sync::Mutex;
use std::time::Duration;

use larafim_core::t;
use larafim_runtimes::app_update::{self, REPO_URL};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateStatus {
    pub current: String,
    /// Última versão publicada (`None` = nenhuma ainda ou não verificado).
    pub latest: Option<String>,
    pub available: bool,
    pub published_at: Option<String>,
    /// Quando verificou pela última vez (segundos desde 1970).
    pub checked_at: Option<u64>,
    pub checking: bool,
    pub error: Option<String>,
    /// Instalado pela Microsoft Store: as atualizações vêm de lá.
    pub store: bool,
    /// Página da release (só deste repositório; validada em `parse_latest`).
    #[serde(skip)]
    pub url: Option<String>,
}

#[derive(Default)]
pub struct UpdateState(Mutex<UpdateStatus>);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn snapshot(app: &AppHandle) -> UpdateStatus {
    let mut s = app.state::<UpdateState>().0.lock().unwrap().clone();
    s.current = CURRENT.into();
    s.store = crate::packaged::is_packaged();
    s
}

async fn check(app: &AppHandle) -> UpdateStatus {
    if crate::packaged::is_packaged() {
        return snapshot(app);
    }
    app.state::<UpdateState>().0.lock().unwrap().checking = true;
    let _ = app.emit("app:update", snapshot(app));
    let result = match larafim_runtimes::http_client() {
        Ok(client) => app_update::latest(&client).await,
        Err(e) => Err(e),
    };
    {
        let state = app.state::<UpdateState>();
        let mut s = state.0.lock().unwrap();
        s.checking = false;
        s.checked_at = Some(now());
        match result {
            Ok(Some(r)) => {
                s.available = app_update::is_newer(&r.version, CURRENT);
                s.latest = Some(r.version);
                s.published_at = r.published_at;
                s.url = Some(r.url);
                s.error = None;
            }
            Ok(None) => {
                s.available = false;
                s.latest = None;
                s.url = None;
                s.error = None;
            }
            Err(e) => s.error = Some(e.to_string()),
        }
    }
    let status = snapshot(app);
    if status.available {
        tracing::info!(latest = status.latest, "versão nova do Larafim disponível");
    }
    let _ = app.emit("app:update", status.clone());
    crate::tray::update_tooltip_for_update(app, &status);
    status
}

/// Verificação automática: 20 s depois de abrir e a cada 6 horas.
pub fn start_checker(app: AppHandle) {
    if crate::packaged::is_packaged() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let enabled = app.state::<AppState>().config.lock().map(|c| c.check_updates).unwrap_or(true);
            if enabled {
                check(&app).await;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Situação atual (sem ir à internet).
#[tauri::command]
pub fn app_update_status(app: AppHandle) -> UpdateStatus {
    snapshot(&app)
}

/// Botão "Verificar atualizações".
#[tauri::command]
pub async fn app_update_check(app: AppHandle) -> UpdateStatus {
    check(&app).await
}

/// Abre um dos links fixos do projeto. A interface nunca manda uma URL:
/// só o nome do link, e o endereço sai daqui.
#[tauri::command]
pub fn open_project_link(app: AppHandle, kind: String) -> Result<(), String> {
    let url = match kind.as_str() {
        "repo" => REPO_URL.to_string(),
        "issues" => format!("{REPO_URL}/issues"),
        "releases" => format!("{REPO_URL}/releases"),
        "security" => format!("{REPO_URL}/security"),
        "license" => format!("{REPO_URL}/blob/main/LICENSE"),
        "docs" => format!("{REPO_URL}/tree/main/docs"),
        "author" => "https://github.com/josevictoremiliano".to_string(),
        "update" => snapshot(&app).url.unwrap_or_else(|| format!("{REPO_URL}/releases/latest")),
        _ => return Err(t!("link desconhecido")),
    };
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}
