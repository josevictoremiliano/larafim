//! Painel da bandeja: janelinha sem moldura que abre junto ao ícone com o
//! clique esquerdo (projetos, e-mail, logs, serviços, versão do PHP).
//! Some ao perder o foco; depois de um tempo escondida, é descarregada
//! para não ocupar memória.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

pub const TRAY_WINDOW: &str = "tray";
/// Largura (lógica) do painel; a altura vem da interface (`tray_fit`).
const WIDTH: f64 = 340.0;
const START_HEIGHT: f64 = 480.0;
/// Distância da barra de tarefas e da borda da tela.
const MARGIN: f64 = 12.0;
/// Escondido por mais que isso: a janela é fechada.
const UNLOAD_AFTER: Duration = Duration::from_secs(45);

struct PanelState {
    /// Onde o ícone foi clicado (coordenadas físicas da tela).
    anchor: Option<(f64, f64)>,
    /// Quando o painel sumiu por perder o foco. Clicar no ícone com ele
    /// aberto tira o foco antes do clique chegar: sem isso, ele reabriria.
    hidden_at: Option<Instant>,
    /// Muda a cada vez que aparece (para o descarregamento não fechar um painel reaberto).
    generation: u64,
}

static STATE: Mutex<PanelState> = Mutex::new(PanelState { anchor: None, hidden_at: None, generation: 0 });

/// Clique esquerdo no ícone: abre ou fecha o painel.
pub fn toggle(app: &AppHandle, click: PhysicalPosition<f64>) {
    {
        let mut s = STATE.lock().unwrap();
        if s.hidden_at.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            s.hidden_at = None;
            return;
        }
        s.hidden_at = None;
        s.anchor = Some((click.x, click.y));
        s.generation += 1;
    }
    if let Some(w) = app.get_webview_window(TRAY_WINDOW) {
        if w.is_visible().unwrap_or(false) {
            hide(&w);
            return;
        }
        place(&w, None);
        let _ = w.emit("tray:shown", ());
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    create(app);
}

fn create(app: &AppHandle) {
    let built = crate::panel_builder(app, TRAY_WINDOW, (WIDTH, START_HEIGHT))
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .focused(true)
        .build();
    let w = match built {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("não foi possível abrir o painel da bandeja: {e}");
            return;
        }
    };
    let handle = w.clone();
    w.on_window_event(move |event| {
        // Ainda escondida (nascendo) também "perde o foco": só conta se estiver à vista.
        if let WindowEvent::Focused(false) = event
            && handle.is_visible().unwrap_or(false)
        {
            hide(&handle);
        }
    });
    place(&w, None);
    // A interface mostra a janela quando termina de desenhar (`tray_fit`).
}

fn hide(w: &WebviewWindow) {
    let generation = {
        let mut s = STATE.lock().unwrap();
        s.hidden_at = Some(Instant::now());
        s.generation
    };
    let _ = w.hide();
    let w = w.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UNLOAD_AFTER).await;
        let same = STATE.lock().unwrap().generation == generation;
        if same && !w.is_visible().unwrap_or(true) {
            let _ = w.destroy();
        }
    });
}

/// Põe o painel acima (ou abaixo) do ícone, dentro da área útil da tela.
fn place(w: &WebviewWindow, height: Option<f64>) {
    let Some((cx, cy)) = STATE.lock().unwrap().anchor else { return };
    let Ok(Some(monitor)) = w.monitor_from_point(cx, cy) else { return };
    let scale = monitor.scale_factor();
    let (ax, ay, aw, ah) = work_area(cx, cy).unwrap_or_else(|| {
        let area = monitor.work_area();
        (area.position.x as f64, area.position.y as f64, area.size.width as f64, area.size.height as f64)
    });

    let width = WIDTH * scale;
    let height = height.map(|h| h * scale).unwrap_or_else(|| w.outer_size().map(|s| s.height as f64).unwrap_or(START_HEIGHT * scale));
    let margin = MARGIN * scale;

    let x = (cx - width / 2.0).clamp(ax + margin, ax + aw - width - margin);
    // Barra de tarefas embaixo (o comum) ou em cima.
    let y = if cy >= ay + ah / 2.0 { ay + ah - height - margin } else { ay + margin };
    let _ = w.set_size(PhysicalSize::new(width.round() as u32, height.round() as u32));
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.max(ay).round() as i32));
}

/// Área útil (sem a barra de tarefas) do monitor do ponto, em pixels físicos.
#[cfg(windows)]
fn work_area(x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    // SAFETY: estrutura com cbSize preenchido; o monitor vem do próprio Windows.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: x as i32, y: y as i32 }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let mut r = info.rcWork;
        // Barra de tarefas que se esconde sozinha não conta na área útil, mas o
        // painel não deve ficar por cima dela quando ela aparece: desconta a barra.
        use windows_sys::Win32::UI::Shell::{ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA, SHAppBarMessage};
        let mut bar: APPBARDATA = std::mem::zeroed();
        bar.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) != 0 {
            let t = bar.rc;
            let m = info.rcMonitor;
            let on_monitor = t.left < m.right && t.right > m.left && t.top < m.bottom && t.bottom > m.top;
            if on_monitor {
                match bar.uEdge {
                    ABE_BOTTOM => r.bottom = r.bottom.min(t.top),
                    ABE_TOP => r.top = r.top.max(t.bottom),
                    ABE_LEFT => r.left = r.left.max(t.right),
                    ABE_RIGHT => r.right = r.right.min(t.left),
                    _ => {}
                }
            }
        }
        Some((r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64))
    }
}
#[cfg(not(windows))]
fn work_area(_x: f64, _y: f64) -> Option<(f64, f64, f64, f64)> {
    None
}

/// A interface mede o conteúdo e pede a altura; na primeira vez, também mostra.
#[tauri::command]
pub fn tray_fit(app: AppHandle, height: f64) {
    let Some(w) = app.get_webview_window(TRAY_WINDOW) else { return };
    place(&w, Some(height.clamp(120.0, 900.0)));
    if !w.is_visible().unwrap_or(true) && STATE.lock().unwrap().hidden_at.is_none() {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Ações do painel que não têm comando próprio. O painel some em seguida.
/// `async` de propósito: comando síncrono roda na thread principal, e criar
/// uma janela ali trava o WebView2 no Windows (janela branca, app congelado).
/// Acontecia ao abrir pela bandeja depois de iniciar com o Windows (`--tray`).
#[tauri::command]
pub async fn tray_action(app: AppHandle, action: String) {
    if let Some(w) = app.get_webview_window(TRAY_WINDOW) {
        hide(&w);
    }
    match action.as_str() {
        "main" => crate::show_main_window(&app),
        "new-laravel" => crate::show_main_window_new_laravel(&app),
        "quit" => {
            tracing::info!("saindo pelo painel da bandeja");
            app.exit(0);
        }
        "hide" => {}
        other => tracing::warn!(action = other, "ação desconhecida do painel da bandeja"),
    }
}
