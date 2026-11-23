//! Edição segura do `.env` de um projeto: mostra o que muda, guarda uma
//! cópia antes da primeira alteração e mexe só nas chaves pedidas,
//! preservando comentários, ordem e o resto do arquivo.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

pub const BACKUP_NAME: &str = ".env.larafim.bak";

/// Uma mudança: `value: None` = comentar a linha (desativar a chave).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvChange {
    pub key: String,
    pub value: Option<String>,
    raw: bool,
}

impl EnvChange {
    pub fn set(key: &str, value: &str) -> Self {
        Self { key: key.into(), value: Some(value.into()), raw: false }
    }
    pub fn unset(key: &str) -> Self {
        Self { key: key.into(), value: None, raw: false }
    }

    /// Restaura o lado direito original, incluindo aspas, referências e comentários.
    pub(crate) fn restore(key: &str, value: Option<String>) -> Self {
        Self { key: key.into(), value, raw: true }
    }
}

/// Linha antes → depois, para a pessoa revisar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub key: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

fn key_of(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let t = t.strip_prefix("export ").unwrap_or(t);
    let (k, _) = t.split_once('=')?;
    let k = k.trim();
    (!k.is_empty() && !k.starts_with('#') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(k)
}

/// Valor atual de uma chave (ativa, não comentada). Se a chave aparecer
/// repetida, vale a última, como no phpdotenv e no Vite.
pub fn get(text: &str, key: &str) -> Option<String> {
    get_raw(text, key).map(|v| unquote(v.trim()))
}

pub(crate) fn get_raw<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().rev().find(|l| key_of(l) == Some(key)).and_then(|l| l.split_once('=')).map(|(_, v)| v)
}

/// Uma atribuição restaurada deve terminar na própria linha, inclusive com aspas.
fn safe_raw_value(value: &str) -> bool {
    if !crate::safety::is_env_value(&value.replace('\t', " ")) {
        return false;
    }
    let mut chars = value.trim_start().chars();
    let Some(delimiter @ ('\'' | '"')) = chars.next() else { return true };
    while let Some(c) = chars.next() {
        if c == delimiter {
            let rest = chars.as_str().trim_start();
            return rest.is_empty() || rest.starts_with('#');
        }
        if delimiter == '"' && c == '\\' {
            chars.next();
        }
    }
    false
}

/// Inverso de `quote`: tira as aspas e desfaz os escapes das aspas duplas.
fn unquote(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].to_string();
    }
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let mut out = String::new();
        let mut chars = v[1..v.len() - 1].chars();
        while let Some(c) = chars.next() {
            match (c, chars.clone().next()) {
                ('\\', Some(n @ ('\\' | '"' | '$'))) => {
                    out.push(n);
                    chars.next();
                }
                _ => out.push(c),
            }
        }
        return out;
    }
    v.trim_matches('"').to_string()
}

/// Valor pronto para o `.env`. Sem aspas só quando é "simples"; senão entre
/// aspas simples (literal: o phpdotenv não expande `${...}` nem `\`). Se o
/// valor tiver aspas simples, usa aspas duplas escapando `\`, `"` e `$`.
fn quote(v: &str) -> String {
    if v.is_empty() || v.chars().all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c)) {
        v.to_string()
    } else if !v.contains('\'') {
        format!("'{v}'")
    } else {
        let escaped = v.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$");
        format!("\"{escaped}\"")
    }
}

/// Recusa chaves fora do padrão e valores com quebra de linha (que criariam
/// outras variáveis no arquivo).
pub fn validate(changes: &[EnvChange]) -> Result<()> {
    for ch in changes {
        if !crate::safety::is_env_key(&ch.key) {
            return Err(Error::Invalid(crate::t!("chave inválida no .env: {a}", a = ch.key.escape_debug())));
        }
        if ch.value.as_deref().is_some_and(|v| !(if ch.raw { safe_raw_value(v) } else { crate::safety::is_env_value(v) })) {
            return Err(Error::Invalid(crate::t!("valor inválido para {a} no .env (quebra de linha ou caractere de controle)", a = ch.key)));
        }
    }
    Ok(())
}

/// Aplica as mudanças e devolve (texto novo, o que mudou).
pub fn apply(text: &str, changes: &[EnvChange]) -> (String, Vec<DiffLine>) {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut diff = Vec::new();

    for ch in changes {
        // `write`/`preview` já recusam; aqui só garante que nada inválido entra.
        if validate(std::slice::from_ref(ch)).is_err() {
            continue;
        }
        let current = lines.join("\n");
        let before = get(&current, &ch.key);
        let same = if ch.raw { get_raw(&current, &ch.key) == ch.value.as_deref() } else { before == ch.value };
        if same {
            continue;
        }
        let idx = lines.iter().position(|l| key_of(l) == Some(ch.key.as_str()));
        // Linhas repetidas da mesma chave: todas recebem a mudança (senão a
        // última, que é a que vale, continuaria com o valor antigo).
        let dups: Vec<usize> = match idx {
            Some(first) => (first + 1..lines.len()).filter(|&i| key_of(&lines[i]) == Some(ch.key.as_str())).collect(),
            None => Vec::new(),
        };
        match (&ch.value, idx) {
            (Some(v), Some(i)) => {
                let value = if ch.raw { v.clone() } else { quote(v) };
                for j in std::iter::once(i).chain(dups) {
                    lines[j] = format!("{}={value}", ch.key);
                }
            }
            (Some(v), None) => {
                let value = if ch.raw { v.clone() } else { quote(v) };
                // Reaproveita uma linha comentada da mesma chave, se houver.
                let commented = lines.iter().position(|l| {
                    l.trim_start().strip_prefix('#').map(str::trim_start).and_then(key_of) == Some(ch.key.as_str())
                });
                match commented {
                    Some(i) => lines[i] = format!("{}={value}", ch.key),
                    None => lines.push(format!("{}={value}", ch.key)),
                }
            }
            (None, Some(i)) => {
                for j in std::iter::once(i).chain(dups) {
                    lines[j] = format!("# {}", lines[j]);
                }
            }
            (None, None) => continue,
        }
        let after = ch.value.as_deref().map(|v| if ch.raw { unquote(v.trim()) } else { v.to_string() });
        diff.push(DiffLine { key: ch.key.clone(), before, after });
    }
    let mut out = lines.join(newline);
    if text.ends_with('\n') || text.is_empty() {
        out.push_str(newline);
    }
    (out, diff)
}

pub fn env_path(project: &Path) -> PathBuf {
    project.join(".env")
}

fn read(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Só calcula o que mudaria (para a tela de confirmação).
pub fn preview(project: &Path, changes: &[EnvChange]) -> Result<Vec<DiffLine>> {
    validate(changes)?;
    let text = read(&env_path(project))?;
    Ok(apply(&text, changes).1)
}

/// Grava as mudanças. Antes da primeira alteração feita pelo Larafim,
/// guarda o `.env` original em `.env.larafim.bak`.
pub fn write(project: &Path, changes: &[EnvChange]) -> Result<Vec<DiffLine>> {
    validate(changes)?;
    let path = env_path(project);
    let text = read(&path)?;
    let (new_text, diff) = apply(&text, changes);
    if diff.is_empty() {
        return Ok(diff);
    }
    let backup = project.join(BACKUP_NAME);
    if !backup.exists() && path.exists() {
        std::fs::copy(&path, &backup).map_err(|e| Error::io(&backup, e))?;
    }
    std::fs::write(&path, new_text).map_err(|e| Error::io(&path, e))?;
    Ok(diff)
}

/// Volta o `.env` para a cópia feita antes da primeira alteração.
pub fn undo(project: &Path) -> Result<()> {
    let backup = project.join(BACKUP_NAME);
    if !backup.is_file() {
        return Err(Error::Invalid(crate::t!("não há cópia do .env para restaurar")));
    }
    std::fs::copy(&backup, env_path(project)).map_err(|e| Error::io(&backup, e))?;
    std::fs::remove_file(&backup).map_err(|e| Error::io(&backup, e))
}

pub fn has_backup(project: &Path) -> bool {
    project.join(BACKUP_NAME).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV: &str = "APP_NAME=Loja\r\n# banco\r\nDB_CONNECTION=sqlite\r\n# DB_HOST=127.0.0.1\r\nDB_DATABASE=\"laravel\"\r\nMAIL_MAILER=log\r\n";

    #[test]
    fn apply_keeps_rest_and_reports_diff() {
        let (out, diff) = apply(
            ENV,
            &[
                EnvChange::set("DB_CONNECTION", "mariadb"),
                EnvChange::set("DB_HOST", "127.0.0.1"),
                EnvChange::set("DB_PORT", "3307"),
                EnvChange::set("DB_DATABASE", "loja_virtual"),
                EnvChange::set("DB_PASSWORD", ""),
                EnvChange::set("MAIL_MAILER", "log"), // igual: não muda
            ],
        );
        assert!(out.starts_with("APP_NAME=Loja\r\n# banco\r\nDB_CONNECTION=mariadb\r\nDB_HOST=127.0.0.1\r\n"));
        assert!(out.contains("DB_DATABASE=loja_virtual\r\n"));
        assert!(out.ends_with("DB_PORT=3307\r\nDB_PASSWORD=\r\n"));
        let keys: Vec<_> = diff.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["DB_CONNECTION", "DB_HOST", "DB_PORT", "DB_DATABASE", "DB_PASSWORD"]);
        assert_eq!(diff[0].before.as_deref(), Some("sqlite"));
        assert_eq!(diff[3].before.as_deref(), Some("laravel"));
    }

    #[test]
    fn unset_comments_line_and_quotes_when_needed() {
        let (out, diff) = apply(ENV, &[EnvChange::unset("DB_DATABASE"), EnvChange::set("APP_NAME", "Minha Loja")]);
        assert!(out.contains("# DB_DATABASE=\"laravel\""));
        assert!(out.contains("APP_NAME='Minha Loja'"));
        assert_eq!(diff.len(), 2);
        assert_eq!(get(&out, "DB_DATABASE"), None);
        assert_eq!(get(&out, "APP_NAME").as_deref(), Some("Minha Loja"));
    }

    #[test]
    fn write_backup_and_undo() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".env"), ENV).unwrap();
        write(tmp.path(), &[EnvChange::set("DB_CONNECTION", "mariadb")]).unwrap();
        assert!(has_backup(tmp.path()));
        // Segunda alteração não sobrescreve a cópia original.
        write(tmp.path(), &[EnvChange::set("DB_CONNECTION", "sqlite"), EnvChange::set("X", "1")]).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join(BACKUP_NAME)).unwrap(), ENV);
        undo(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join(".env")).unwrap(), ENV);
        assert!(!has_backup(tmp.path()));
    }

    #[test]
    fn repeated_keys_all_change_and_last_wins() {
        let env = "REVERB_PORT=8080\nA=1\nREVERB_PORT=6009\n";
        assert_eq!(get(env, "REVERB_PORT").as_deref(), Some("6009"));
        let (out, diff) = apply(env, &[EnvChange::set("REVERB_PORT", "8081")]);
        assert_eq!(out, "REVERB_PORT=8081\nA=1\nREVERB_PORT=8081\n");
        assert_eq!(diff[0].before.as_deref(), Some("6009"));
        let (out, _) = apply(env, &[EnvChange::unset("REVERB_PORT")]);
        assert_eq!(get(&out, "REVERB_PORT"), None, "{out}");
    }

    // ---------- Injeção ----------

    #[test]
    fn newline_in_value_cannot_create_variables() {
        let evil = EnvChange::set("APP_NAME", "loja\nAPP_DEBUG=true");
        let (out, diff) = apply("APP_NAME=x\n", std::slice::from_ref(&evil));
        assert!(diff.is_empty());
        assert!(!out.contains("APP_DEBUG"));
        assert!(validate(&[evil]).is_err());
        assert!(validate(&[EnvChange::set("A", "x\rB=1")]).is_err());
    }

    #[test]
    fn bad_keys_are_refused() {
        for key in ["", "A=B", "A B", "A\nB", "1A", "../x"] {
            assert!(validate(&[EnvChange::set(key, "1")]).is_err(), "{key:?}");
            assert!(apply("", &[EnvChange::set(key, "1")]).1.is_empty());
        }
    }

    #[test]
    fn values_are_literal() {
        // `${...}` seria expandido pelo phpdotenv em aspas duplas; `\` no fim
        // fecharia a aspa errado. Todos voltam iguais ao que foi gravado.
        for v in ["${APP_KEY}", "a\\", "it's $HOME \"q\"", "p@ss w0rd", "x'y\\z$"] {
            let (out, _) = apply("", &[EnvChange::set("SECRET", v)]);
            assert_eq!(get(&out, "SECRET").as_deref(), Some(v), "{out}");
            assert_eq!(out.lines().count(), 1, "{out}");
        }
        let (out, _) = apply("", &[EnvChange::set("A", "${APP_KEY}")]);
        assert_eq!(out.trim(), "A='${APP_KEY}'");
    }

    #[test]
    fn write_refuses_injection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(env_path(dir.path()), "APP_NAME=x\n").unwrap();
        assert!(write(dir.path(), &[EnvChange::set("APP_NAME", "x\nEVIL=1")]).is_err());
        assert_eq!(std::fs::read_to_string(env_path(dir.path())).unwrap(), "APP_NAME=x\n");
    }
}
