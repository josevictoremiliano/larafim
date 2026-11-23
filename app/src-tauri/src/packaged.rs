//! Larafim instalado pela Microsoft Store (pacote MSIX).
//!
//! No pacote, o executável fica numa pasta com a versão no nome
//! (`C:\Program Files\WindowsApps\jotav.larafim_0.1.2.0_...`), que muda a cada
//! atualização. Para iniciar com o Windows e para o ajudante elevado do
//! `hosts`, usamos o alias `larafim.exe` declarado no manifesto, que fica
//! sempre no mesmo lugar. Atualizações vêm da Store, não do GitHub.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Nome do alias em `packaging/msix/AppxManifest.xml` (AppExecutionAlias).
const ALIAS: &str = "larafim.exe";

/// O app está rodando como pacote MSIX.
pub fn is_packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
    let mut len = 0u32;
    // SAFETY: com tamanho 0 e buffer nulo a função só informa o tamanho
    // necessário (ou que não há pacote); nada é escrito no buffer.
    let r = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    r != APPMODEL_ERROR_NO_PACKAGE
}

#[cfg(not(windows))]
fn detect() -> bool {
    false
}

/// Executável para abrir o Larafim de fora (registro Run, UAC): o alias no
/// pacote MSIX; senão o próprio executável.
pub fn launch_exe() -> std::io::Result<PathBuf> {
    if is_packaged()
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        let alias = PathBuf::from(local).join(r"Microsoft\WindowsApps").join(ALIAS);
        if alias.exists() {
            return Ok(alias);
        }
        tracing::warn!("alias {ALIAS} não encontrado; usando o executável do pacote");
    }
    std::env::current_exe()
}
