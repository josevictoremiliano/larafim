//! Redis nativo para Windows. O Redis oficial não publica binários para
//! Windows; o Larafim usa as compilações do projeto redis-windows (MSYS2,
//! sem serviço do Windows), com versão e SHA-256 fixados no código.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use larafim_core::{Paths, t};

use crate::archive::{Limits, extract_zip};
use crate::download::download_verified;
use crate::error::{Error, Result};

const REDIS_VERSION: &str = "8.10.3";
const REDIS_URL: &str = "https://github.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip";
const REDIS_SHA256: &str = "af16df340525b431e9ec8dabfd625d5e7a233f82805b5418819904ea909c447f";

pub fn redis_dir(paths: &Paths) -> PathBuf {
    paths.runtimes_dir().join("redis")
}

pub fn redis_server_exe(paths: &Paths) -> PathBuf {
    redis_dir(paths).join("redis-server.exe")
}

pub fn redis_cli_exe(paths: &Paths) -> PathBuf {
    redis_dir(paths).join("redis-cli.exe")
}

pub fn redis_installed(paths: &Paths) -> bool {
    redis_server_exe(paths).is_file()
}

pub fn installed_version(paths: &Paths) -> Option<String> {
    std::fs::read_to_string(redis_dir(paths).join("VERSION")).ok().map(|v| v.trim().to_string())
}

/// Pasta (dentro da extração) que tem o `redis-server.exe`.
fn find_bin(root: &Path) -> Option<PathBuf> {
    if root.join("redis-server.exe").is_file() {
        return Some(root.to_path_buf());
    }
    std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.join("redis-server.exe").is_file())
}

pub async fn install_redis(
    client: &reqwest::Client,
    paths: &Paths,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<String> {
    let zip_path = paths.downloads_dir().join(format!("redis-{REDIS_VERSION}.zip"));
    let never = AtomicBool::new(false);
    download_verified(client, REDIS_URL, REDIS_SHA256, &zip_path, &never, on_progress).await?;
    let dir = redis_dir(paths);
    let staging = paths.runtimes_dir().join(format!("redis-{REDIS_VERSION}.tmp"));
    let (z, s, d) = (zip_path.clone(), staging.clone(), dir.clone());
    tokio::task::spawn_blocking(move || -> Result<()> {
        extract_zip(&z, &s, Limits { max_entries: 200, max_total_bytes: 512 * 1024 * 1024 })?;
        let bin = find_bin(&s).ok_or_else(|| Error::Zip(t!("o pacote não contém redis-server.exe")))?;
        // Só os programas: os dados ficam em data\redis e não são tocados.
        if d.exists() {
            std::fs::remove_dir_all(&d).map_err(|e| Error::io(&d, e))?;
        }
        std::fs::rename(&bin, &d).map_err(|e| Error::io(&d, e))?;
        let _ = std::fs::remove_dir_all(&s);
        Ok(())
    })
    .await
    .map_err(|e| Error::Zip(e.to_string()))?
    .inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&staging);
    })?;
    let _ = std::fs::remove_file(&zip_path);
    std::fs::write(dir.join("VERSION"), REDIS_VERSION).map_err(|e| Error::io(&dir, e))?;
    tracing::info!(version = REDIS_VERSION, "Redis instalado");
    Ok(REDIS_VERSION.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_package_has_a_trusted_versioned_url_and_sha256() {
        use larafim_core::safety::{is_https_url_on, is_safe_component, is_sha256};
        assert!(is_safe_component(REDIS_VERSION));
        assert!(is_https_url_on(REDIS_URL, &["github.com"]));
        assert_eq!(REDIS_URL, format!(
            "https://github.com/redis-windows/redis-windows/releases/download/{REDIS_VERSION}/Redis-{REDIS_VERSION}-Windows-x64-msys2.zip"
        ));
        assert!(is_sha256(REDIS_SHA256));
    }
}
