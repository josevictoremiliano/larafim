//! Iniciar com o Windows: uma entrada em HKCU\...\Run (só do usuário, sem
//! admin) que abre o Larafim direto na bandeja (`--tray`).
use larafim_core::t;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "Larafim";
pub const TRAY_ARG: &str = "--tray";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Linha de comando gravada no registro.
fn command_line() -> Result<String, String> {
    let exe = crate::packaged::launch_exe().map_err(|e| e.to_string())?;
    Ok(format!("\"{}\" {TRAY_ARG}", exe.display()))
}

#[cfg(windows)]
fn read() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let (key, value) = (wide(RUN_KEY), wide(VALUE));
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: buffers válidos e tamanho em bytes correto.
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
    };
    if r != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(windows)]
fn write(cmd: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    let (key, value, data) = (wide(RUN_KEY), wide(VALUE), wide(cmd));
    // SAFETY: strings terminadas em zero; tamanho em bytes inclui o zero final.
    let r = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
    };
    if r == 0 { Ok(()) } else { Err(t!("não foi possível gravar no registro (erro {r})", r = r)) }
}

#[cfg(windows)]
fn delete() -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyValueW};
    let (key, value) = (wide(RUN_KEY), wide(VALUE));
    // SAFETY: strings terminadas em zero.
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
    // 2 = ERROR_FILE_NOT_FOUND: já não existia.
    if r == 0 || r == 2 { Ok(()) } else { Err(t!("não foi possível remover do registro (erro {r})", r = r)) }
}

#[cfg(not(windows))]
fn read() -> Option<String> {
    None
}
#[cfg(not(windows))]
fn write(_: &str) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
fn delete() -> Result<(), String> {
    Ok(())
}

/// Liga ou desliga. Ligado, aponta sempre para o executável atual.
pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        let cmd = command_line()?;
        if read().as_deref() != Some(cmd.as_str()) {
            write(&cmd)?;
        }
        tracing::info!("iniciar com o Windows: ligado");
    } else {
        delete()?;
        tracing::info!("iniciar com o Windows: desligado");
    }
    Ok(())
}

/// Ao abrir: se estava ligado, corrige o caminho (o app pode ter mudado de pasta).
pub fn sync(enabled: bool) {
    if enabled && let Err(e) = set(true) {
        tracing::warn!("iniciar com o Windows: {e}");
    }
}

pub fn started_in_tray() -> bool {
    std::env::args().any(|a| a == TRAY_ARG)
}
