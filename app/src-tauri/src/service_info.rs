//! Detalhes de um serviço para o painel de Serviços: pasta de dados,
//! últimas linhas do log e as variáveis prontas para colar no `.env`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::server::ServerState;

type CmdResult<T> = Result<T, String>;

/// Quantas linhas do log mostrar.
const TAIL_LINES: usize = 60;

#[derive(Serialize)]
pub struct ServiceInfo {
    data_dir: Option<PathBuf>,
    log_file: Option<PathBuf>,
    log: Vec<String>,
    env: Vec<(String, String)>,
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn db_env(connection: &str, port: u16, user: &str) -> Vec<(String, String)> {
    let port = port.to_string();
    pairs(&[
        ("DB_CONNECTION", connection),
        ("DB_HOST", "127.0.0.1"),
        ("DB_PORT", &port),
        ("DB_DATABASE", "laravel"),
        ("DB_USERNAME", user),
        ("DB_PASSWORD", ""),
    ])
}

/// Últimas linhas de um arquivo (lê só o final).
fn tail_file(path: &Path) -> Vec<String> {
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(32 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // primeira linha provavelmente cortada
    }
    let skip = lines.len().saturating_sub(TAIL_LINES);
    lines[skip..].iter().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect()
}

fn dev_tail(app: &AppHandle, key: &str) -> Vec<String> {
    let all = app.state::<ServerState>().dev.logs(key);
    let skip = all.len().saturating_sub(TAIL_LINES);
    all[skip..].to_vec()
}

/// `service`: "mariadb", "postgres", "own" (com `project`), "mailpit", "s3" ou "conn:<id>".
#[tauri::command]
pub fn service_info(app: AppHandle, service: String, project: Option<PathBuf>) -> CmdResult<ServiceInfo> {
    let paths = app.state::<AppState>().paths.clone();
    let info = match service.as_str() {
        "mariadb" => {
            let port = app.state::<AppState>().config.lock().unwrap().db_port;
            let setup = crate::services::maria_setup(&app).ok();
            let log_file = setup.as_ref().map(|s| s.data_dir.join("larafim-mariadb.log"));
            ServiceInfo {
                data_dir: setup.map(|s| s.data_dir),
                log: log_file.as_deref().map(tail_file).unwrap_or_default(),
                log_file,
                env: db_env("mariadb", port, "root"),
            }
        }
        "postgres" => {
            let port = app.state::<AppState>().config.lock().unwrap().pg_port;
            let setup = crate::pg::pg_setup(&app).ok();
            let log_file = setup.as_ref().map(|s| s.data_dir.join("larafim-postgres.log"));
            ServiceInfo {
                data_dir: setup.map(|s| s.data_dir),
                log: log_file.as_deref().map(tail_file).unwrap_or_default(),
                log_file,
                env: db_env("pgsql", port, "postgres"),
            }
        }
        "own" => {
            let path = project.ok_or_else(|| "projeto ausente".to_string())?;
            let cfg = crate::services::project_config(&app, &path)?;
            let own = crate::services::own_of(&cfg).ok_or_else(|| larafim_core::t!("o projeto não tem MariaDB próprio configurado"))?;
            let setup = crate::services::own_setup(&app, &path, &own).ok();
            let log_file = setup.as_ref().map(|s| s.data_dir.join("larafim-mariadb.log"));
            ServiceInfo {
                data_dir: setup.map(|s| s.data_dir),
                log: log_file.as_deref().map(tail_file).unwrap_or_default(),
                log_file,
                env: db_env("mariadb", own.port, "root"),
            }
        }
        "mailpit" => ServiceInfo {
            data_dir: Some(paths.data_dir().join("mailpit")),
            log_file: None,
            log: dev_tail(&app, crate::tools::MAILPIT_KEY),
            env: pairs(&[
                ("MAIL_MAILER", "smtp"),
                ("MAIL_HOST", "127.0.0.1"),
                ("MAIL_PORT", &crate::tools::MAIL_SMTP_PORT.to_string()),
                ("MAIL_USERNAME", "null"),
                ("MAIL_PASSWORD", "null"),
                ("MAIL_ENCRYPTION", "null"),
            ]),
        },
        "s3" => {
            let secret = crate::s3::secret(&app)?;
            ServiceInfo {
                data_dir: Some(paths.data_dir().join("s3")),
                log_file: None,
                log: dev_tail(&app, crate::s3::S3_KEY),
                env: crate::s3::changes("laravel", &secret)
                    .into_iter()
                    .map(|c| (c.key, c.value.unwrap_or_default()))
                    .collect(),
            }
        }
        "redis" => {
            let port = crate::redis::redis_config(&app).map(|r| r.port).unwrap_or(larafim_core::redis::DEFAULT_PORT);
            ServiceInfo {
                data_dir: Some(crate::redis::data_dir(&app)),
                log_file: None,
                log: dev_tail(&app, crate::redis::REDIS_KEY),
                env: larafim_core::redis::env_changes(port).into_iter().map(|c| (c.key, c.value.unwrap_or_default())).collect(),
            }
        }
        other => {
            let id = other.strip_prefix("conn:").ok_or_else(|| format!("serviço desconhecido: {other}"))?;
            let c = crate::dbconn::find(&app, id)?;
            let port = c.port.to_string();
            ServiceInfo {
                data_dir: None,
                log_file: None,
                log: Vec::new(),
                env: pairs(&[
                    ("DB_CONNECTION", if c.kind == "mysql" { "mysql" } else { "mariadb" }),
                    ("DB_HOST", &c.host),
                    ("DB_PORT", &port),
                    ("DB_DATABASE", "laravel"),
                    ("DB_USERNAME", &c.user),
                    ("DB_PASSWORD", ""),
                ]),
            }
        }
    };
    Ok(info)
}
