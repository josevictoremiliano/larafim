//! Qual PHP usar numa pasta (terminal, Composer, testes). Ordem:
//!
//! 1. Variável `LARAFIM_PHP` (ex.: `8.3`) — escolha manual da sessão.
//! 2. Projeto do Larafim que contém a pasta: versão fixada, ou a mais nova
//!    instalada que atende ao que o projeto pede.
//! 3. Pasta fora do Larafim: `.php-version` ou `composer.json` mais próximo.
//! 4. Versão padrão das configurações, ou a mais nova instalada.

use larafim_core::t;
use std::path::{Path, PathBuf};

use larafim_core::store::same_path;
use larafim_core::{AppConfig, Paths, ProjectsFile};
use serde::Serialize;

use super::installed::{InstalledPhp, best_installed, list_installed};

#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    /// PHP escolhido; `None` = nenhum instalado serve (veja `reason`).
    pub php: Option<InstalledPhp>,
    /// Explicação curta, em português, de por que essa versão.
    pub reason: String,
    /// Projeto do Larafim que contém a pasta, se houver.
    pub project: Option<String>,
    /// O que o projeto/pasta pede, se pedir.
    pub request: Option<String>,
}

fn newest_of_minor<'a>(installed: &'a [InstalledPhp], minor: &str) -> Option<&'a InstalledPhp> {
    installed.iter().find(|p| p.minor == minor || p.version == minor)
}

/// O que uma pasta pede: `.php-version` primeiro, depois `composer.json`.
fn request_in(dir: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(dir.join(".php-version"))
        && let Some(line) = text.lines().next().map(str::trim).filter(|l| !l.is_empty())
    {
        return Some(line.trim_start_matches('v').to_string());
    }
    let text = std::fs::read_to_string(dir.join("composer.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.pointer("/require/php").and_then(|v| v.as_str()).map(String::from)
}

fn ancestors(dir: &Path) -> impl Iterator<Item = &Path> {
    dir.ancestors()
}

pub fn select(paths: &Paths, dir: &Path) -> Selection {
    let installed = list_installed(&paths.php_dir());
    let pick = |php: Option<&InstalledPhp>, reason: String, project: Option<String>, request: Option<String>| Selection {
        php: php.cloned(),
        reason,
        project,
        request,
    };

    if installed.is_empty() {
        return pick(None, t!("nenhum PHP instalado no Larafim"), None, None);
    }

    // 1. Escolha manual da sessão.
    if let Ok(forced) = std::env::var("LARAFIM_PHP")
        && !forced.trim().is_empty()
    {
        let forced = forced.trim().to_string();
        return match newest_of_minor(&installed, &forced) {
            Some(p) => pick(Some(p), format!("LARAFIM_PHP={forced}"), None, None),
            None => pick(None, t!("LARAFIM_PHP={forced}, mas essa versão não está instalada", forced = forced), None, None),
        };
    }

    // 2. Projeto do Larafim que contém a pasta (o mais específico).
    let store = ProjectsFile::load(&paths.projects_file()).unwrap_or_default();
    let project = ancestors(dir).find_map(|a| store.projects.iter().find(|p| same_path(&p.path, a)));
    if let Some(cfg) = project {
        let name = cfg.domain.clone();
        let request = request_in(&cfg.path);
        if let Some(minor) = &cfg.php {
            return match newest_of_minor(&installed, minor) {
                Some(p) => pick(Some(p), t!("versão fixada no projeto ({minor})", minor = minor), Some(name), request),
                None => pick(None, t!("o projeto fixa o PHP {minor}, que não está instalado", minor = minor), Some(name), request),
            };
        }
        return match &request {
            Some(req) => match best_installed(req, &installed) {
                Some(p) => pick(Some(p), t!("mais nova compatível com {req}", req = req), Some(name), request.clone()),
                None => pick(None, t!("o projeto pede {req} e nenhuma versão instalada atende", req = req), Some(name), request.clone()),
            },
            None => pick(installed.first(), t!("mais nova instalada (o projeto não informa a versão)"), Some(name), None),
        };
    }

    // 3. Pasta fora do Larafim com .php-version / composer.json.
    if let Some((found_in, req)) = ancestors(dir).find_map(|a| request_in(a).map(|r| (a.to_path_buf(), r))) {
        let found: PathBuf = found_in;
        return match best_installed(&req, &installed) {
            Some(p) => pick(
                Some(p),
                t!("mais nova compatível com {req} (de {a})", req = req, a = found.display()),
                None,
                Some(req),
            ),
            None => pick(None, t!("{a} pede {req} e nenhuma versão instalada atende", a = found.display(), req = req), None, Some(req)),
        };
    }

    // 4. Padrão.
    let config = AppConfig::load(&paths.config_file()).unwrap_or_default();
    if let Some(minor) = &config.default_php
        && let Some(p) = newest_of_minor(&installed, minor)
    {
        return pick(Some(p), t!("versão padrão ({minor})", minor = minor), None, None);
    }
    pick(installed.first(), t!("mais nova instalada"), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use larafim_core::ProjectConfig;
    use std::fs;

    fn fake_php(paths: &Paths, version: &str) {
        let d = paths.php_dir().join(version);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("php.exe"), "").unwrap();
    }

    #[test]
    fn selection_order() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::at(tmp.path().join("home"));
        for v in ["8.1.34", "8.4.28", "8.6.0"] {
            fake_php(&paths, v);
        }
        let www = tmp.path().join("www");
        let loja = www.join("loja");
        let blog = www.join("blog");
        let solto = tmp.path().join("solto");
        fs::create_dir_all(loja.join("app").join("Http")).unwrap();
        fs::create_dir_all(&blog).unwrap();
        fs::create_dir_all(solto.join("src")).unwrap();
        fs::write(loja.join("composer.json"), r#"{"require":{"php":"^8.3"}}"#).unwrap();
        fs::write(blog.join(".php-version"), "8.1\n").unwrap();
        fs::write(solto.join("composer.json"), r#"{"require":{"php":">=8.1 <8.5"}}"#).unwrap();

        let mut store = ProjectsFile::default();
        let cfg = |path: &Path, domain: &str, php: Option<&str>| ProjectConfig {
            path: path.to_path_buf(),
            domain: domain.into(),
            aliases: Vec::new(),
            php: php.map(String::from),
            docroot: String::new(),
            node: false,
            node_command: None,
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
            online: false,
        };
        store.upsert(cfg(&loja, "loja", None)).unwrap();
        store.upsert(cfg(&blog, "blog", Some("8.1"))).unwrap();
        store.save(&paths.projects_file()).unwrap();

        // Projeto do Larafim, subpasta, versão pedida (^8.3 → mais nova: 8.6).
        let s = select(&paths, &loja.join("app").join("Http"));
        assert_eq!(s.php.unwrap().version, "8.6.0");
        assert_eq!(s.project.as_deref(), Some("loja"));

        // Versão fixada.
        let s = select(&paths, &blog);
        assert_eq!(s.php.unwrap().version, "8.1.34");
        assert!(s.reason.contains("fixada"));

        // Pasta fora do Larafim com composer.json.
        let s = select(&paths, &solto.join("src"));
        assert_eq!(s.php.unwrap().version, "8.4.28");
        assert_eq!(s.project, None);

        // Pasta qualquer → mais nova.
        let s = select(&paths, tmp.path());
        assert_eq!(s.php.unwrap().version, "8.6.0");

        // Padrão configurado.
        let config = AppConfig { default_php: Some("8.4".into()), ..Default::default() };
        config.save(&paths.config_file()).unwrap();
        let s = select(&paths, tmp.path());
        assert_eq!(s.php.unwrap().version, "8.4.28");
    }

    #[test]
    fn nothing_installed_or_incompatible() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::at(tmp.path().join("home"));
        assert!(select(&paths, tmp.path()).php.is_none());

        fake_php(&paths, "8.4.28");
        let old = tmp.path().join("legado");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("composer.json"), r#"{"require":{"php":"^7.4"}}"#).unwrap();
        let s = select(&paths, &old);
        assert!(s.php.is_none());
        assert!(s.reason.contains("^7.4"));
    }
}
