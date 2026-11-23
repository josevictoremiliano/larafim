//! Núcleo do Larafim: onde ficam os dados, configuração do app e logs.

pub mod config;
pub mod composer;
pub mod envfile;
pub mod error;
pub mod i18n;
pub mod logging;
pub mod paths;
pub mod projects;
pub mod queues;
pub mod redis;
pub mod reverb;
pub mod safety;
pub mod store;

pub use config::{AppConfig, DbConnection, RedisConfig, Theme};
pub use projects::{DetectedProject, ProjectKind};
pub use store::{ProjectConfig, ProjectStatus, ProjectView, ProjectsFile};
pub use error::{Error, Result};
pub use paths::Paths;
