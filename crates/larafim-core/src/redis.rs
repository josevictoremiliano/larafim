//! Redis opcional do Larafim (filas e cache). Não há Redis oficial para
//! Windows: ele roda num contêiner do Docker ou numa distribuição do WSL2.
//! Aqui fica a parte pura (nomes, lista do WSL, `.env`).

use std::path::Path;

use crate::envfile::EnvChange;
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
}
