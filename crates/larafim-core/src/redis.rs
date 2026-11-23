//! Redis opcional do Larafim (filas e cache). Não há Redis oficial para
//! Windows: ele roda como binário de terceiros, no Docker ou no WSL2.
//! Aqui fica a parte pura (nomes, lista do WSL, `.env`).

use std::{collections::BTreeMap, path::Path};

use crate::envfile::{self, EnvChange};
use crate::safety::is_wsl_distro;

/// Imagem oficial do Docker Hub (versão fixa, para não mudar sozinha).
pub const DOCKER_IMAGE: &str = "redis:7.4-alpine";
/// Nome do contêiner e do volume de dados.
pub const CONTAINER: &str = "larafim-redis";
pub const VOLUME: &str = "larafim-redis-data";
pub const DEFAULT_PORT: u16 = 6379;

/// Saída de `wsl.exe -l -q` (UTF-16LE no Windows) → distribuições usáveis.
/// As do Docker Desktop ficam de fora (não são para uso direto).
pub fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let text = if bom || (bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).take(8).all(|b| *b == 0)) {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0');
        if is_wsl_distro(name) && !name.to_ascii_lowercase().starts_with("docker-desktop") && !out.iter().any(|d| d == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// O projeto tem um cliente Redis para PHP? No Windows o Larafim não traz a
/// extensão `phpredis`, então vale o pacote `predis/predis`.
pub fn has_predis(project: &Path) -> bool {
    project.join("vendor").join("predis").join("predis").is_dir() && crate::composer::autoloads(project, "Predis")
}

/// Mudanças no `.env` para as filas usarem o Redis do Larafim.
pub fn env_changes(port: u16) -> Vec<EnvChange> {
    vec![
        EnvChange::set("QUEUE_CONNECTION", "redis"),
        EnvChange::set("REDIS_CLIENT", "predis"),
        EnvChange::set("REDIS_HOST", "127.0.0.1"),
        EnvChange::set("REDIS_PORT", &port.to_string()),
        EnvChange::set("REDIS_PASSWORD", "null"),
    ]
}

pub type EnvSnapshot = BTreeMap<String, Option<String>>;

/// Guarda cada chave na primeira alteração, inclusive quando ela estava ausente.
pub fn remember_env(text: &str, changes: &[EnvChange], saved: &mut EnvSnapshot) -> crate::error::Result<()> {
    for change in envfile::apply(text, changes).1 {
        saved.entry(change.key.clone()).or_insert_with(|| envfile::get_raw(text, &change.key).map(str::to_owned));
    }
    envfile::validate(&restore_env(saved))
}

pub fn restore_env(saved: &EnvSnapshot) -> Vec<EnvChange> {
    // Uma cópia guardada só pode repor as chaves que o Redis gerencia.
    env_changes(DEFAULT_PORT).into_iter().filter_map(|ch| {
        saved.get(&ch.key).map(|value| EnvChange::restore(&ch.key, value.clone()))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parses_wsl_list() {
        let out = utf16("\u{feff}Ubuntu\r\ndocker-desktop\r\nUbuntu-24.04\r\n\r\nUbuntu\r\n");
        assert_eq!(parse_wsl_list(&out), ["Ubuntu", "Ubuntu-24.04"]);
        assert_eq!(parse_wsl_list(b"Debian\nkali-linux\n"), ["Debian", "kali-linux"]);
        assert!(parse_wsl_list(&[]).is_empty());
    }

    #[test]
    fn hostile_wsl_names_are_dropped() {
        // A lista vem de um programa de fora e vira `wsl -d <nome>`.
        let out = utf16("-u root\r\n--exec calc\r\nUbuntu & calc\r\na\"b\r\n..\\x\r\nok\r\n");
        assert_eq!(parse_wsl_list(&out), ["ok"]);
    }

    #[test]
    fn env_is_valid() {
        let changes = env_changes(6380);
        assert!(crate::envfile::validate(&changes).is_ok());
        let (out, _) = crate::envfile::apply("QUEUE_CONNECTION=database\n", &changes);
        assert_eq!(crate::envfile::get(&out, "QUEUE_CONNECTION").as_deref(), Some("redis"));
        assert_eq!(crate::envfile::get(&out, "REDIS_PORT").as_deref(), Some("6380"));
    }

    #[test]
    fn restores_previous_values_without_touching_other_services() {
        let original = "QUEUE_CONNECTION=sync\r\nREDIS_CLIENT=\r\nREDIS_PASSWORD=\"${LOCAL_PASSWORD}\" # exemplo\r\nDB_CONNECTION=sqlite\r\n";
        let mut saved = EnvSnapshot::new();
        remember_env(original, &env_changes(6379), &mut saved).unwrap();
        let (managed, _) = envfile::apply(original, &env_changes(6379));
        remember_env(&managed, &env_changes(6380), &mut saved).unwrap();
        let (managed, _) = envfile::apply(&managed, &[EnvChange::set("REDIS_PORT", "6380"), EnvChange::set("MAIL_MAILER", "smtp")]);
        let (restored, _) = envfile::apply(&managed, &restore_env(&saved));
        assert!(restored.starts_with(original), "{restored}");
        assert_eq!(envfile::get(&restored, "REDIS_HOST"), None);
        assert_eq!(envfile::get(&restored, "REDIS_PORT"), None);
        assert_eq!(envfile::get(&restored, "MAIL_MAILER").as_deref(), Some("smtp"));
    }

    #[test]
    fn restore_rejects_injection_and_ignores_unmanaged_keys() {
        for value in ["sync\r\nEVIL=1", "\"sem fechar", "\"x\" NEW_KEY=1"] {
            let saved = EnvSnapshot::from([("QUEUE_CONNECTION".into(), Some(value.into()))]);
            assert!(envfile::validate(&restore_env(&saved)).is_err());
        }
        let saved = EnvSnapshot::from([("APP_DEBUG".into(), Some("true".into()))]);
        assert!(restore_env(&saved).is_empty());
    }
}
