//! Redis nativo para Windows. O Redis oficial não publica binários para
//! Windows; o Larafim usa as compilações do projeto redis-windows (MSYS2,
//! sem serviço do Windows), baixadas do GitHub com o SHA-256 do release.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use larafim_core::{Paths, t};

use crate::archive::{Limits, extract_zip};
use crate::download::{download_verified, fetch_text};
use crate::error::{Error, Result};

const API: &str = "https://api.github.com/repos/redis-windows/redis-windows/releases/latest";
const URL_PREFIX: &str = "https://github.com/redis-windows/redis-windows/releases/download/";

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

/// (versão, url, sha256) do pacote MSYS2 sem serviço, pela API do GitHub.
pub fn parse_release(json: &str) -> Result<(String, String, String)> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| Error::Catalog(e.to_string()))?;
    let tag = v["tag_name"].as_str().unwrap_or_default().to_string();
    // A versão vira nome de arquivo e aparece na tela: só caracteres simples.
    if !larafim_core::safety::is_safe_component(&tag) {
        return Err(Error::Catalog(t!("versão do Redis inválida no catálogo")));
    }
    let wanted = format!("Redis-{tag}-Windows-x64-msys2.zip");
    let asset = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == wanted.as_str()))
        .ok_or_else(|| Error::Catalog(t!("Redis sem pacote para Windows")))?;
    let url = asset["browser_download_url"].as_str().unwrap_or_default().to_string();
    let sha = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).unwrap_or_default().to_lowercase();
    if url != format!("{URL_PREFIX}{tag}/{wanted}") || !larafim_core::safety::is_sha256(&sha) {
        return Err(Error::Catalog(t!("dados do Redis incompletos (sem SHA-256)")));
    }
    Ok((tag, url, sha))
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
    let (tag, url, sha) = parse_release(&fetch_text(client, API).await?)?;
    let zip_path = paths.downloads_dir().join(format!("redis-{tag}.zip"));
    let never = AtomicBool::new(false);
    download_verified(client, &url, &sha, &zip_path, &never, on_progress).await?;
    let dir = redis_dir(paths);
    let staging = paths.runtimes_dir().join(format!("redis-{tag}.tmp"));
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
    std::fs::write(dir.join("VERSION"), &tag).map_err(|e| Error::io(&dir, e))?;
    tracing::info!(version = tag, "Redis instalado");
    Ok(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, name: &str, url: &str, digest: &str) -> String {
        serde_json::json!({ "tag_name": tag, "assets": [
            { "name": format!("Redis-{tag}-Windows-x64-cygwin.zip"), "browser_download_url": "https://github.com/x", "digest": "sha256:00" },
            { "name": name, "browser_download_url": url, "digest": digest },
        ]})
        .to_string()
    }

    #[test]
    fn picks_msys2_package() {
        let sha = "af16df340525b431e9ec8dabfd625d5e7a233f82805b5418819904ea909c447f";
        let url = "https://github.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip";
        let json = release("8.10.3", "Redis-8.10.3-Windows-x64-msys2.zip", url, &format!("sha256:{}", sha.to_uppercase()));
        assert_eq!(parse_release(&json).unwrap(), ("8.10.3".into(), url.into(), sha.into()));
    }

    // ---------- Catálogo adulterado ----------

    #[test]
    fn tampered_release_is_refused() {
        let sha = format!("sha256:{}", "a".repeat(64));
        let name = "Redis-8.10.3-Windows-x64-msys2.zip";
        for url in [
            "https://evil.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip",
            "https://github.com/outro/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip",
            "http://github.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip",
        ] {
            assert!(parse_release(&release("8.10.3", name, url, &sha)).is_err(), "{url}");
        }
        let good = "https://github.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip";
        assert!(parse_release(&release("8.10.3", name, good, "sha256:curto")).is_err(), "sem SHA-256");
        for tag in ["../x", "8.10.3\\..\\x", "CON", "", "a b"] {
            assert!(parse_release(&release(tag, name, good, &sha)).is_err(), "{tag:?}");
        }
        assert!(parse_release("<html>").is_err());
    }
}
