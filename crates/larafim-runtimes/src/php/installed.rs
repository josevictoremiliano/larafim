//! Versões instaladas e escolha da versão certa para um projeto.

use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use serde::Serialize;

use super::parse_version;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledPhp {
    /// "8.4.28"
    pub version: String,
    /// "8.4"
    pub minor: String,
    pub dir: PathBuf,
}

impl InstalledPhp {
    pub fn php_exe(&self) -> PathBuf {
        self.dir.join("php.exe")
    }
    pub fn ini(&self) -> PathBuf {
        self.dir.join("php.ini")
    }
    fn semver(&self) -> Version {
        parse_version(&self.version).unwrap_or(Version::new(0, 0, 0))
    }
}

/// Pastas `runtimes\php\<x.y.z>` que têm `php.exe`, da mais nova para a mais antiga.
/// Pastas temporárias de instalação (`.tmp-…`) são ignoradas.
pub fn list_installed(php_dir: &Path) -> Vec<InstalledPhp> {
    let Ok(entries) = std::fs::read_dir(php_dir) else { return Vec::new() };
    let mut list: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let v = parse_version(&name)?;
            let dir = e.path();
            dir.join("php.exe").is_file().then(|| InstalledPhp {
                minor: format!("{}.{}", v.major, v.minor),
                version: name,
                dir,
            })
        })
        .collect();
    list.sort_by_key(|p| std::cmp::Reverse(p.semver()));
    list
}

/// Conserta o `php.ini` das versões instaladas cuja pasta mudou de lugar
/// (veja `ini::relocate`). Devolve as versões consertadas.
pub fn repair_moved(php_dir: &Path) -> Vec<String> {
    let mut fixed = Vec::new();
    for php in list_installed(php_dir) {
        let Ok(text) = super::ini::read(&php.ini()) else { continue };
        let Some(updated) = super::ini::relocate(&text, &php.dir) else { continue };
        match super::ini::write(&php.ini(), &updated) {
            Ok(()) => fixed.push(php.version),
            Err(e) => tracing::warn!("não foi possível consertar o php.ini do PHP {}: {e}", php.version),
        }
    }
    fixed
}

/// A versão mais nova de cada minor (a que os projetos usam).
pub fn current_per_minor(installed: &[InstalledPhp]) -> Vec<&InstalledPhp> {
    let mut seen = std::collections::HashSet::new();
    installed.iter().filter(|p| seen.insert(p.minor.clone())).collect()
}

/// Converte o que o projeto pede (`composer.json` ou `.php-version`) em
/// requisitos do semver. Aceita `^8.2`, `>=8.1 <8.4`, `^8.1|^8.2`, `8.*`,
/// e versões soltas (`8.1` = qualquer 8.1.x).
fn parse_constraint(constraint: &str) -> Vec<VersionReq> {
    constraint
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|alt| {
            let alt = alt.split('@').next().unwrap_or(alt).trim();
            let bare = alt.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            let req = if bare {
                let parts: Vec<_> = alt.split('.').collect();
                match parts.len() {
                    1 => format!("~{}", parts[0]),
                    2 => format!("~{}.{}", parts[0], parts[1]),
                    _ => format!("={alt}"),
                }
            } else {
                // Composer separa "E" com espaço ou vírgula; semver só com vírgula.
                alt.replace(", ", ",")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(",")
            };
            VersionReq::parse(&req).ok()
        })
        .collect()
}

/// `true` se a versão atende ao pedido do projeto. Pedido ilegível = `false`.
pub fn matches_constraint(constraint: &str, version: &str) -> bool {
    let Some(v) = parse_version(version) else { return false };
    parse_constraint(constraint).iter().any(|r| r.matches(&v))
}

/// A versão instalada mais nova que atende ao pedido do projeto.
pub fn best_installed<'a>(constraint: &str, installed: &'a [InstalledPhp]) -> Option<&'a InstalledPhp> {
    current_per_minor(installed)
        .into_iter()
        .find(|p| matches_constraint(constraint, &p.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(dir: &Path, version: &str) {
        let d = dir.join(version);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("php.exe"), "").unwrap();
    }

    #[test]
    fn lists_sorted_and_ignores_junk() {
        let tmp = tempfile::tempdir().unwrap();
        fake(tmp.path(), "8.3.36");
        fake(tmp.path(), "8.4.28");
        fake(tmp.path(), "8.3.37");
        std::fs::create_dir_all(tmp.path().join(".tmp-8.5.13")).unwrap();
        std::fs::create_dir_all(tmp.path().join("8.2.0")).unwrap(); // sem php.exe

        let list = list_installed(tmp.path());
        let v: Vec<_> = list.iter().map(|p| p.version.as_str()).collect();
        assert_eq!(v, ["8.4.28", "8.3.37", "8.3.36"]);

        let current: Vec<_> = current_per_minor(&list).iter().map(|p| p.version.as_str()).collect();
        assert_eq!(current, ["8.4.28", "8.3.37"]);
    }

    #[test]
    fn constraints() {
        assert!(matches_constraint("^8.2", "8.4.1"));
        assert!(!matches_constraint("^8.2", "8.1.9"));
        assert!(matches_constraint(">=8.1 <8.4", "8.3.0"));
        assert!(!matches_constraint(">=8.1 <8.4", "8.4.0"));
        assert!(matches_constraint("^8.1|^7.4", "7.4.33"));
        assert!(matches_constraint("^8.1 || ^8.2", "8.2.1"));
        assert!(matches_constraint("8.1", "8.1.34"));
        assert!(!matches_constraint("8.1", "8.2.0"));
        assert!(matches_constraint("8.*", "8.5.0"));
        assert!(matches_constraint("~8.3.0", "8.3.5"));
        assert!(matches_constraint("^8.2@dev", "8.3.0"));
        assert!(!matches_constraint("lixo", "8.3.0"));
    }

    #[test]
    fn best_installed_prefers_newest_compatible() {
        let tmp = tempfile::tempdir().unwrap();
        fake(tmp.path(), "8.5.13");
        fake(tmp.path(), "8.3.37");
        fake(tmp.path(), "8.1.34");
        let list = list_installed(tmp.path());
        assert_eq!(best_installed("^8.2", &list).unwrap().version, "8.5.13");
        assert_eq!(best_installed(">=8.1 <8.4", &list).unwrap().version, "8.3.37");
        assert_eq!(best_installed("8.1", &list).unwrap().version, "8.1.34");
        assert!(best_installed("^7.4", &list).is_none());
    }
}
