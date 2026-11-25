//! Versões novas do próprio Larafim, pela última release publicada no GitHub.
//! Aqui só se descobre a versão; baixar e instalar fica com o updater do
//! Tauri (app/src-tauri/src/updates.rs), e só quando a pessoa pede.

use larafim_core::t;
use serde::Serialize;

use crate::download::fetch_text;
use crate::error::{Error, Result};

/// Repositório oficial.
pub const REPO: &str = "josevictoremiliano/larafim";
pub const REPO_URL: &str = "https://github.com/josevictoremiliano/larafim";
const API: &str = "https://api.github.com/repos/josevictoremiliano/larafim/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// "0.2.0" (sem o "v")
    pub version: String,
    /// Página da release no GitHub.
    pub url: String,
    /// Data de publicação (ISO 8601).
    pub published_at: Option<String>,
}

/// Lê a resposta de `/releases/latest`. Recusa tag que não seja `vX.Y.Z` e
/// página que não seja deste repositório (nada de link para outro site).
pub fn parse_latest(json: &str) -> Result<Release> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| Error::Catalog(e.to_string()))?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Err(Error::Catalog(t!("a última release não é uma versão estável")));
    }
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let parsed = semver::Version::parse(version).map_err(|_| Error::Catalog(t!("versão inválida na release: {tag}", tag = tag.escape_debug())))?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() {
        return Err(Error::Catalog(t!("a última release não é uma versão estável")));
    }
    let url = v["html_url"].as_str().unwrap_or_default();
    let expected = format!("{REPO_URL}/releases/tag/");
    if !url.starts_with(&expected) || !larafim_core::safety::is_https_url_on(url, &["github.com"]) {
        return Err(Error::Catalog(t!("link da release fora do repositório oficial")));
    }
    Ok(Release {
        version: parsed.to_string(),
        url: url.to_string(),
        published_at: v["published_at"].as_str().filter(|d| d.len() <= 40).map(String::from),
    })
}

/// `latest` é mais nova que `current`?
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (semver::Version::parse(latest), semver::Version::parse(current)) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    }
}

/// Última release estável, ou `None` se ainda não há nenhuma publicada.
pub async fn latest(client: &reqwest::Client) -> Result<Option<Release>> {
    match fetch_text(client, API).await {
        Ok(text) => parse_latest(&text).map(Some),
        Err(Error::HttpStatus { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, url: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","html_url":"{url}","draft":false,"prerelease":false,"published_at":"2026-11-21T12:00:00Z"}}"#)
    }

    #[test]
    fn reads_release() {
        let r = parse_latest(&release("v0.2.0", "https://github.com/josevictoremiliano/larafim/releases/tag/v0.2.0")).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.published_at.as_deref(), Some("2026-11-21T12:00:00Z"));
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("lixo", "0.1.0"));
    }

    /// Resposta adulterada não leva o usuário para outro site nem mostra
    /// uma versão estranha.
    #[test]
    fn refuses_tampered_release() {
        let ok_url = "https://github.com/josevictoremiliano/larafim/releases/tag/v9.0.0";
        for (tag, url) in [
            ("v9.0.0", "https://evil.example/larafim.exe"),
            ("v9.0.0", "https://github.com/outro/larafim/releases/tag/v9.0.0"),
            ("v9.0.0", "https://github.com.evil.example/josevictoremiliano/larafim/releases/tag/v9.0.0"),
            ("v9.0.0", "http://github.com/josevictoremiliano/larafim/releases/tag/v9.0.0"),
            ("v9.0.0-rc.1", ok_url),
            ("<script>", ok_url),
            ("v9", ok_url),
        ] {
            assert!(parse_latest(&release(tag, url)).is_err(), "{tag} {url}");
        }
        let draft = r#"{"tag_name":"v9.0.0","html_url":"https://github.com/josevictoremiliano/larafim/releases/tag/v9.0.0","draft":true}"#;
        assert!(parse_latest(draft).is_err());
        assert!(parse_latest("não é json").is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn real_latest() {
        let c = crate::http_client().unwrap();
        let r = latest(&c).await.unwrap();
        println!("{r:?}");
    }
}
