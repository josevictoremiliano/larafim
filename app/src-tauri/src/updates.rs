//! Versão nova do Larafim: confere a última release no GitHub ao abrir e a
//! cada 6 horas (se a opção estiver ligada) e avisa as janelas. Só baixa e
//! instala quando a pessoa clica em "Atualizar": o instalador vem da release,
//! com a assinatura conferida pela chave pública embutida no app
//! (`plugins.updater.pubkey`), e roda em modo de atualização (sem a tela de
//! desinstalar a versão anterior). Instalado pela Microsoft Store, quem
//! atualiza é a Store: nada é consultado.

use std::sync::Mutex;
use std::time::Duration;

use larafim_core::t;
use larafim_runtimes::app_update::{self, REPO_URL};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

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
    /// Baixando/instalando a versão nova.
    pub installing: bool,
    /// Progresso do download (0–100), quando o servidor informa o tamanho.
    pub progress: Option<u8>,
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

/// O instalador só pode vir das releases deste repositório. A assinatura já
/// garante a origem; isto evita baixar qualquer coisa de outro lugar.
fn is_release_asset(url: &str) -> bool {
    let prefix = format!("{REPO_URL}/releases/download/");
    url.starts_with(&prefix) && larafim_core::safety::is_https_url_on(url, &["github.com"]) && !url.contains("..")
}

fn set_installing(app: &AppHandle, installing: bool, progress: Option<u8>, error: Option<String>) {
    {
        let state = app.state::<UpdateState>();
        let mut s = state.0.lock().unwrap();
        s.installing = installing;
        s.progress = progress;
        if error.is_some() {
            s.error = error;
        }
    }
    let _ = app.emit("app:update", snapshot(app));
}

/// Botão "Atualizar e reiniciar": baixa o instalador da release, confere a
/// assinatura e roda em modo de atualização. No Windows o app fecha sozinho
/// antes do instalador começar e é reaberto por ele no fim.
#[tauri::command]
pub async fn app_update_install(app: AppHandle) -> Result<(), String> {
    if crate::packaged::is_packaged() {
        return Err(t!("instalado pela Microsoft Store: a Store atualiza o Larafim"));
    }
    if snapshot(&app).installing {
        return Ok(());
    }
    set_installing(&app, true, None, None);
    let result = install(&app).await;
    if let Err(e) = &result {
        tracing::warn!("atualização do Larafim falhou: {e}");
        set_installing(&app, false, None, Some(e.clone()));
    }
    result
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let handle = app.clone();
    let updater = app
        .updater_builder()
        // Antes do instalador: para PHP, bancos e o gateway como ao sair
        // normalmente (o `exit` do plugin pula o RunEvent::Exit).
        .on_before_exit(move || {
            crate::server::shutdown(&handle);
            handle.cleanup_before_exit();
        })
        .build()
        .map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Err(t!("nenhuma versão nova encontrada"));
    };
    if !is_release_asset(update.download_url.as_str()) {
        return Err(t!("link do instalador fora do repositório oficial"));
    }
    tracing::info!(version = update.version, "baixando a versão nova do Larafim");
    let progress = app.clone();
    let mut done: u64 = 0;
    let mut last: Option<u8> = None;
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let pct = total.filter(|t| *t > 0).map(|t| (done.saturating_mul(100) / t).min(100) as u8);
                // Só avisa a interface quando o número muda.
                if pct != last {
                    last = pct;
                    set_installing(&progress, true, pct, None);
                }
            },
            || tracing::info!("download concluído; abrindo o instalador"),
        )
        .await
        .map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::is_release_asset;

    #[test]
    fn instalador_so_das_releases_do_repositorio() {
        assert!(is_release_asset("https://github.com/josevictoremiliano/larafim/releases/download/v0.1.3/Larafim_0.1.3_x64-setup.exe"));
        for bad in [
            "http://github.com/josevictoremiliano/larafim/releases/download/v0.1.3/a.exe",
            "https://github.com/outra-pessoa/larafim/releases/download/v0.1.3/a.exe",
            "https://github.com/josevictoremiliano/larafim-falso/releases/download/v1/a.exe",
            "https://github.com/josevictoremiliano/larafim/releases/download/../../../x/a.exe",
            "https://github.com.evil.example/josevictoremiliano/larafim/releases/download/v1/a.exe",
            "https://evil.example/https://github.com/josevictoremiliano/larafim/releases/download/v1/a.exe",
            "https://github.com/josevictoremiliano/larafim/archive/main.zip",
        ] {
            assert!(!is_release_asset(bad), "{bad}");
        }
    }
}
