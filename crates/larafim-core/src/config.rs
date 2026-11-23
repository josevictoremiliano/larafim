use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Segue o tema do Windows.
    #[default]
    System,
    Light,
    Dark,
}

/// Preferências globais do Larafim (`config.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: Theme,
    /// Idioma da interface ("pt-BR", "en"…). Vazio = ainda não escolhido.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Sufixo dos domínios locais, sem ponto. Ex.: `offline` → `loja.offline`.
    pub tld: String,
    pub http_port: u16,
    /// HTTPS local ligado (gateway também em `https_port`, certificado da CA do Larafim).
    pub https: bool,
    pub https_port: u16,
    /// Pastas raiz onde cada subpasta é candidata a projeto.
    pub roots: Vec<PathBuf>,
    pub start_with_windows: bool,
    /// Ao fechar a janela, o Larafim continua na bandeja.
    pub keep_in_tray: bool,
    /// Procura versões novas do Larafim no GitHub (ao abrir e a cada 6 horas).
    pub check_updates: bool,
    pub onboarding_done: bool,
    /// Minor do PHP usado no terminal fora de projetos ("8.4"). Vazio = mais novo instalado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_php: Option<String>,
    /// Linha do MariaDB usada ("11.8"). Vazio = mais nova instalada.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_version: Option<String>,
    /// Porta do MariaDB (3306; 3307 se a 3306 já tiver outro banco).
    pub db_port: u16,
    /// Major do PostgreSQL usada ("18"). Vazio = mais nova instalada.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pg_version: Option<String>,
    /// Porta do PostgreSQL (5432; outra se a 5432 já tiver um Postgres).
    pub pg_port: u16,
    /// Chave secreta do storage S3 local, protegida com DPAPI (gerada na primeira vez).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s3_secret: Option<String>,
    /// Bancos que já existem no PC (MySQL/MariaDB do Laragon, XAMPP…).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub db_connections: Vec<DbConnection>,
    /// Redis opcional (filas e cache), rodando no Docker ou no WSL2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redis: Option<RedisConfig>,
}

/// Onde o Redis do Larafim roda: nativo no Windows (padrão), Docker ou WSL2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedisConfig {
    /// "native", "docker" ou "wsl".
    pub backend: String,
    /// Distribuição do WSL (só com `backend = "wsl"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
    pub port: u16,
}

impl RedisConfig {
    pub fn validate(&self) -> Result<()> {
        match (self.backend.as_str(), &self.distro) {
            ("native" | "docker", _) => {}
            ("wsl", Some(d)) if crate::safety::is_wsl_distro(d) => {}
            ("wsl", _) => return Err(Error::Invalid(crate::t!("escolha uma distribuição do WSL válida"))),
            _ => return Err(Error::Invalid(crate::t!("o Redis roda nativo, no Docker ou no WSL2"))),
        }
        if self.port < 1024 {
            return Err(Error::Invalid(crate::t!("porta inválida")));
        }
        Ok(())
    }
}

/// Conexão com um servidor de banco que não é do Larafim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbConnection {
    /// Identificador curto (letras, números e hífen).
    pub id: String,
    /// Nome para mostrar ("MySQL do Laragon").
    pub name: String,
    /// "mysql" ou "mariadb".
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Senha protegida pelo Windows (DPAPI), em base64. Vazio = sem senha.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password_protected: String,
    /// Pasta com `mysql.exe`/`mysqldump.exe` (ou `mariadb.exe`) desse servidor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_dir: Option<PathBuf>,
}

impl DbConnection {
    pub fn validate(&self) -> Result<()> {
        let id_ok = !self.id.is_empty() && self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !id_ok {
            return Err(Error::Invalid(crate::t!("identificador da conexão inválido")));
        }
        if self.kind != "mysql" && self.kind != "mariadb" {
            return Err(Error::Invalid(crate::t!("tipo de banco deve ser MySQL ou MariaDB")));
        }
        if self.name.trim().is_empty() || self.host.trim().is_empty() || self.user.trim().is_empty() {
            return Err(Error::Invalid(crate::t!("preencha nome, servidor e usuário")));
        }
        if self.port == 0 {
            return Err(Error::Invalid(crate::t!("porta inválida")));
        }
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: None,
            tld: "offline".into(),
            http_port: 80,
            https: false,
            https_port: 443,
            roots: Vec::new(),
            start_with_windows: false,
            keep_in_tray: true,
            check_updates: true,
            onboarding_done: false,
            default_php: None,
            db_version: None,
            db_port: 3306,
            pg_version: None,
            pg_port: 5432,
            s3_secret: None,
            db_connections: Vec::new(),
            redis: None,
        }
    }
}

impl AppConfig {
    /// Lê o arquivo; se não existir, devolve o padrão.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::ConfigParse {
                path: path.to_path_buf(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Grava de forma atômica (arquivo temporário + renomear), para nunca
    /// deixar uma configuração pela metade se o app fechar no meio.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self)?;
        write_atomic(path, text.as_bytes())
    }

    pub fn validate(&self) -> Result<()> {
        let tld = self.tld.trim();
        let valid_tld = !tld.is_empty()
            && tld.len() <= 63
            && tld.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !tld.starts_with('-')
            && !tld.ends_with('-');
        if !valid_tld {
            return Err(Error::Invalid(format!(
                "domínio \".{}\" inválido: use só letras minúsculas, números e hífen",
                self.tld
            )));
        }
        if self.http_port == 0 {
            return Err(Error::Invalid(crate::t!("a porta HTTP não pode ser 0")));
        }
        if let Some(r) = &self.redis {
            r.validate()?;
        }
        Ok(())
    }

    /// URL completa de um projeto, com a porta quando não for a 80.
    pub fn project_url(&self, name: &str) -> String {
        if self.http_port == 80 {
            format!("http://{name}.{}", self.tld)
        } else {
            format!("http://{name}.{}:{}", self.tld, self.http_port)
        }
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(cfg, AppConfig::default());
        assert_eq!(cfg.tld, "offline");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub").join("config.toml");
        let cfg = AppConfig {
            theme: Theme::Dark,
            roots: vec![PathBuf::from(r"D:\www")],
            http_port: 8080,
            ..Default::default()
        };
        cfg.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), cfg);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "theme = \"light\"\n").unwrap();
        let cfg = AppConfig::load(&path).unwrap();
        assert_eq!(cfg.theme, Theme::Light);
        assert_eq!(cfg.tld, "offline");
        assert!(cfg.keep_in_tray);
    }

    #[test]
    fn rejects_bad_tld() {
        let cfg = AppConfig { tld: ".Off line".into(), ..Default::default() };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn project_url_includes_port_only_when_needed() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.project_url("loja"), "http://loja.offline");
        cfg.http_port = 8080;
        assert_eq!(cfg.project_url("loja"), "http://loja.offline:8080");
    }

    #[test]
    fn redis_config_is_validated() {
        let ok = |backend: &str, distro: Option<&str>, port| RedisConfig { backend: backend.into(), distro: distro.map(String::from), port };
        assert!(ok("native", None, 6379).validate().is_ok());
        assert!(ok("docker", None, 6379).validate().is_ok());
        assert!(ok("wsl", Some("Ubuntu-24.04"), 6380).validate().is_ok());
        assert!(ok("wsl", None, 6379).validate().is_err());
        assert!(ok("docker", None, 80).validate().is_err());
        assert!(ok("podman", None, 6379).validate().is_err());
        // Vai para `wsl -d <nome>`: nada de opções, espaços ou comandos.
        for evil in ["-u root", "--exec calc", "Ubuntu & calc", "a\"b", "..\\x", ""] {
            assert!(ok("wsl", Some(evil), 6379).validate().is_err(), "{evil:?}");
        }
    }
}
