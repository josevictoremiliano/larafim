//! Runtimes gerenciados pelo Larafim. Fase 1: PHP (+ Composer e certificados).

pub mod app_update;
pub mod archive;
pub mod download;
pub mod error;
pub mod extras;
pub mod mariadb;
pub mod node;
pub mod php;
pub mod postgres;
pub mod redis;
pub mod s3;
pub mod tools;

pub use error::{Error, Result};

/// Cliente HTTP com identificação genérica (sem dados do usuário).
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("Larafim/", env!("CARGO_PKG_VERSION"), " (gerenciador local de PHP)"))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(Error::Http)
}
