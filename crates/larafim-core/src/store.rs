//! Cadastro de projetos (`projects.toml`): o que a pessoa importou e
//! configurou. Nada disso é gravado dentro dos projetos.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::write_atomic;
use crate::error::{Error, Result};
use crate::projects::{DetectedProject, ProjectKind, detect_any, domain_label, scan_roots};

/// Configuração de um projeto importado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub path: PathBuf,
    /// Rótulo do domínio, sem o TLD (`loja` → `loja.offline`).
    pub domain: String,
    /// Domínios extras do mesmo projeto, sem o TLD (`finder.noveck` →
    /// `finder.noveck.local`). Útil para white-label/multi-tenant por host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Minor do PHP fixado ("8.3"). Vazio = automático (mais novo compatível).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php: Option<String>,
    /// Pasta pública relativa ao projeto; vazio = raiz.
    #[serde(default)]
    pub docroot: String,
    /// Rodar o servidor de desenvolvimento do Node junto.
    #[serde(default)]
    pub node: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_command: Option<String>,
    /// Major do Node fixado ("22"). Vazio = automático.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_version: Option<String>,
    /// Banco: "mariadb" (compartilhado), "mariadb-own" (servidor próprio do
    /// projeto), "sqlite" ou "external:<id>". Vazio = não gerenciado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// MariaDB próprio: linha ("10.11") e porta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_port: Option<u16>,
    /// E-mail: "mailpit" (caixa local). Vazio = não gerenciado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    /// Storage: "s3" (S3 local do Larafim, RustFS). Vazio = não gerenciado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
    /// Workers do Laravel ligados junto com o projeto: "queue", "schedule", "reverb".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workers: Vec<String>,
    /// Porta do Reverb do projeto (escolhida pelo Larafim, sem repetir entre projetos).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverb_port: Option<u16>,
    /// Filas que o worker processa (`--queue=a,b`). Vazio = descobrir ao ligar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queues: Vec<String>,
    /// Conexão do worker de fila: "redis" (Redis do Larafim). Vazio = a do `.env`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_connection: Option<String>,
    /// Valores anteriores do `.env`, cifrados com a DPAPI antes de gravar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_redis_env: Option<String>,
    /// Intenção de deixar online (usado a partir da fase 3).
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectsFile {
    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectConfig>,
    /// Pastas das raízes que a pessoa escolheu não ver na lista.
    #[serde(default)]
    pub ignored: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    /// Encontrado numa pasta raiz, ainda não importado.
    New,
    Imported,
    /// Importado, mas a pasta não existe mais.
    Missing,
}

/// O que a interface mostra: detecção + configuração.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectView {
    pub path: PathBuf,
    pub name: String,
    /// Pasta raiz onde está; `None` = projeto avulso.
    pub root: Option<PathBuf>,
    pub kind: ProjectKind,
    pub status: ProjectStatus,
    pub domain: String,
    /// Domínios extras (sem o TLD).
    pub aliases: Vec<String>,
    pub docroot: String,
    /// Minor fixado; `None` = automático.
    pub php_pin: Option<String>,
    /// O que o projeto pede (`composer.json`/`.php-version`).
    pub php_request: Option<String>,
    pub has_node: bool,
    pub node_enabled: bool,
    pub node_command: Option<String>,
    pub node_request: Option<String>,
    /// Major do Node fixado; `None` = automático.
    pub node_pin: Option<String>,
    pub database: Option<String>,
    pub db_version: Option<String>,
    pub db_port: Option<u16>,
    pub mail: Option<String>,
    pub storage: Option<String>,
    pub workers: Vec<String>,
    pub reverb_port: Option<u16>,
    pub queues: Vec<String>,
    pub queue_connection: Option<String>,
    pub online: bool,
}

/// Compara caminhos do Windows sem diferenciar maiúsculas e barras.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

fn under_root(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let parent = path.parent()?;
    roots.iter().find(|r| same_path(r, parent)).cloned()
}

impl ProjectsFile {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::ConfigParse { path: path.into(), source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = format!(
            "# Projetos do Larafim. Pode editar com o app fechado.\n\n{}",
            toml::to_string_pretty(self)?
        );
        write_atomic(path, text.as_bytes())
    }

    pub fn find(&self, path: &Path) -> Option<&ProjectConfig> {
        self.projects.iter().find(|p| same_path(&p.path, path))
    }

    fn is_ignored(&self, path: &Path) -> bool {
        self.ignored.iter().any(|i| same_path(i, path))
    }

    /// Junta o que foi detectado nas raízes com o que está cadastrado.
    pub fn views(&self, roots: &[PathBuf]) -> Vec<ProjectView> {
        let detected = scan_roots(roots);
        let mut used: Vec<String> = self.projects.iter().map(|p| p.domain.clone()).collect();
        let mut views = Vec::new();

        for cfg in &self.projects {
            let root = under_root(&cfg.path, roots);
            let found = detected
                .iter()
                .find(|d| same_path(&d.path, &cfg.path))
                .cloned()
                .or_else(|| detect_any(&cfg.path));
            views.push(match found {
                Some(d) => ProjectView {
                    status: ProjectStatus::Imported,
                    ..view_from(&d, root, cfg)
                },
                None => missing_view(cfg, root),
            });
        }

        for d in detected {
            if self.find(&d.path).is_some() || self.is_ignored(&d.path) {
                continue;
            }
            let domain = unique_domain(&d.domain, &used);
            used.push(domain.clone());
            let cfg = default_config(&d, domain);
            views.push(ProjectView { status: ProjectStatus::New, ..view_from(&d, Some(d.root.clone()), &cfg) });
        }

        views.sort_by_key(|v| v.name.to_lowercase());
        views
    }

    /// Importa uma pasta com a configuração sugerida. Devolve o domínio dado.
    pub fn import(&mut self, path: &Path) -> Result<String> {
        if self.find(path).is_some() {
            return Err(Error::Invalid(crate::t!("este projeto já foi importado")));
        }
        let d = detect_any(path).ok_or_else(|| Error::Invalid(crate::t!("a pasta {a} não existe", a = path.display())))?;
        let used: Vec<String> = self.projects.iter().map(|p| p.domain.clone()).collect();
        let cfg = default_config(&d, unique_domain(&d.domain, &used));
        let domain = cfg.domain.clone();
        self.ignored.retain(|i| !same_path(i, path));
        self.projects.push(cfg);
        Ok(domain)
    }

    /// Grava a configuração editada de um projeto já importado (ou novo, no
    /// caso de projeto avulso com revisão), validando tudo antes.
    pub fn upsert(&mut self, mut cfg: ProjectConfig) -> Result<()> {
        cfg.domain = cfg.domain.trim().to_lowercase();
        validate_domain(&cfg.domain)?;
        if self
            .projects
            .iter()
            .any(|p| !same_path(&p.path, &cfg.path) && p.domain == cfg.domain)
        {
            return Err(Error::Invalid(crate::t!("o domínio \"{a}\" já é usado por outro projeto", a = cfg.domain)));
        }
        if !cfg.path.is_dir() {
            return Err(Error::Invalid(crate::t!("a pasta {a} não existe", a = cfg.path.display())));
        }
        cfg.docroot = cfg.docroot.trim().trim_matches(['\\', '/']).to_string();
        if cfg.docroot.split(['\\', '/']).any(|part| part == ".." || part.contains(':'))
            || !cfg.path.join(&cfg.docroot).is_dir()
        {
            return Err(Error::Invalid(crate::t!("a pasta pública \"{a}\" não existe dentro do projeto", a = cfg.docroot)));
        }
        if let Some(php) = &cfg.php {
            let ok = php.split('.').count() == 2 && php.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            if !ok {
                return Err(Error::Invalid(crate::t!("versão do PHP inválida: {php}", php = php)));
            }
        }
        cfg.node_command = cfg.node_command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        if let Some(major) = &cfg.node_version
            && (major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()))
        {
            return Err(Error::Invalid(crate::t!("versão do Node inválida: {major}", major = major)));
        }
        if let Some(db) = &cfg.database
            && db != "mariadb"
            && db != "mariadb-own"
            && db != "postgres"
            && db != "sqlite"
            && !db.strip_prefix("external:").is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        {
            return Err(Error::Invalid(crate::t!("banco desconhecido: {db}", db = db)));
        }
        if cfg.database.as_deref() == Some("mariadb-own") && (cfg.db_version.is_none() || cfg.db_port.is_none()) {
            return Err(Error::Invalid(crate::t!("o MariaDB próprio precisa de versão e porta")));
        }
        if cfg.mail.as_deref().is_some_and(|m| m != "mailpit") {
            return Err(Error::Invalid(crate::t!("serviço de e-mail desconhecido")));
        }
        if cfg.storage.as_deref().is_some_and(|m| m != "s3") {
            return Err(Error::Invalid(crate::t!("storage desconhecido")));
        }
        if cfg.workers.iter().any(|w| !["queue", "schedule", "reverb"].contains(&w.as_str())) {
            return Err(Error::Invalid(crate::t!("worker desconhecido")));
        }
        if cfg.reverb_port.is_some_and(|p| !crate::reverb::PORTS.contains(&p)) {
            return Err(Error::Invalid(crate::t!("porta do Reverb inválida")));
        }
        if cfg.queues.len() > crate::queues::MAX_QUEUES || cfg.queues.iter().any(|q| !crate::safety::is_queue_name(q)) {
            return Err(Error::Invalid(crate::t!("nome de fila inválido")));
        }
        if cfg.queue_connection.as_deref().is_some_and(|c| c != "redis") {
            return Err(Error::Invalid(crate::t!("conexão de fila desconhecida")));
        }
        if cfg.node && cfg.node_command.is_none() {
            return Err(Error::Invalid(crate::t!("informe o comando do Node (ex.: npm run dev)")));
        }

        self.ignored.retain(|i| !same_path(i, &cfg.path));
        match self.projects.iter_mut().find(|p| same_path(&p.path, &cfg.path)) {
            Some(existing) => *existing = cfg,
            None => self.projects.push(cfg),
        }
        Ok(())
    }

    /// Tira o projeto do Larafim (os arquivos ficam intactos). Se ele estiver
    /// numa pasta raiz, passa a ser ignorado para não voltar como "novo".
    /// Define os domínios extras de um projeto (substitui a lista inteira).
    /// Cada alias é um ou mais rótulos separados por ponto, sem o TLD
    /// (`finder.noveck`). Recusa repetidos e nomes já usados por outro projeto.
    pub fn set_aliases(&mut self, path: &Path, aliases: &[String], tld: &str) -> Result<()> {
        let suffix = format!(".{tld}");
        let mut clean: Vec<String> = Vec::new();
        for raw in aliases {
            let mut alias = raw.trim().trim_end_matches('.').to_lowercase();
            if let Some(rest) = alias.strip_suffix(&suffix) {
                alias = rest.to_string();
            }
            if alias.is_empty() {
                continue;
            }
            validate_alias(&alias)?;
            if clean.contains(&alias) {
                continue;
            }
            clean.push(alias);
        }
        if clean.len() > MAX_ALIASES {
            return Err(Error::Invalid(crate::t!("no máximo {a} domínios extras por projeto", a = MAX_ALIASES)));
        }
        let Some(index) = self.projects.iter().position(|p| same_path(&p.path, path)) else {
            return Err(Error::Invalid(crate::t!("projeto {a} não está importado", a = path.display())));
        };
        let own = &self.projects[index].domain;
        if let Some(a) = clean.iter().find(|a| *a == own) {
            return Err(Error::Invalid(crate::t!("\"{a}\" já é o domínio principal deste projeto", a = a)));
        }
        for (i, other) in self.projects.iter().enumerate() {
            if i == index {
                continue;
            }
            if let Some(a) = clean.iter().find(|a| **a == other.domain || other.aliases.iter().any(|o| o.eq_ignore_ascii_case(a))) {
                return Err(Error::Invalid(crate::t!("o domínio \"{a}\" já é usado por outro projeto", a = a)));
            }
        }
        self.projects[index].aliases = clean;
        Ok(())
    }

    pub fn remove(&mut self, path: &Path, roots: &[PathBuf]) {
        self.projects.retain(|p| !same_path(&p.path, path));
        if under_root(path, roots).is_some() && !self.is_ignored(path) {
            self.ignored.push(path.to_path_buf());
        }
    }

    /// Ignora um projeto novo (sem importar).
    pub fn ignore(&mut self, path: &Path) {
        if !self.is_ignored(path) {
            self.ignored.push(path.to_path_buf());
        }
    }

    pub fn unignore(&mut self, path: &Path) {
        self.ignored.retain(|i| !same_path(i, path));
    }
}

impl ProjectConfig {
    /// Hosts completos dos aliases (`<alias>.<tld>`), normalizados, sem
    /// repetir o domínio principal. Aceita o alias com ou sem o TLD.
    pub fn alias_hosts(&self, tld: &str) -> Vec<String> {
        let main = format!("{}.{tld}", self.domain);
        let suffix = format!(".{tld}");
        let mut out: Vec<String> = Vec::new();
        for alias in &self.aliases {
            let label = alias.trim().trim_end_matches('.').to_lowercase();
            let label = label.strip_suffix(&suffix).unwrap_or(&label);
            if validate_alias(label).is_err() {
                continue;
            }
            let host = format!("{label}.{tld}");
            if host != main && !out.contains(&host) {
                out.push(host);
            }
        }
        out
    }
}

fn default_config(d: &DetectedProject, domain: String) -> ProjectConfig {
    ProjectConfig {
        path: d.path.clone(),
        domain,
        aliases: Vec::new(),
        php: None,
        docroot: d.docroot.clone(),
        node: d.kind == ProjectKind::Node && d.node_command.is_some(),
        node_command: d.node_command.clone(),
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
        queue_redis_env: None,
        online: false,
    }
}

fn view_from(d: &DetectedProject, root: Option<PathBuf>, cfg: &ProjectConfig) -> ProjectView {
    ProjectView {
        path: cfg.path.clone(),
        name: d.name.clone(),
        root,
        kind: d.kind,
        status: ProjectStatus::New,
        domain: cfg.domain.clone(),
        aliases: cfg.aliases.clone(),
        docroot: cfg.docroot.clone(),
        php_pin: cfg.php.clone(),
        php_request: d.php.clone(),
        has_node: d.has_node,
        node_enabled: cfg.node,
        node_command: cfg.node_command.clone().or_else(|| d.node_command.clone()),
        node_request: d.node.clone(),
        node_pin: cfg.node_version.clone(),
        database: cfg.database.clone(),
        db_version: cfg.db_version.clone(),
        db_port: cfg.db_port,
        mail: cfg.mail.clone(),
        storage: cfg.storage.clone(),
        workers: cfg.workers.clone(),
        reverb_port: cfg.reverb_port,
        queues: cfg.queues.clone(),
        queue_connection: cfg.queue_connection.clone(),
        online: cfg.online,
    }
}

fn missing_view(cfg: &ProjectConfig, root: Option<PathBuf>) -> ProjectView {
    ProjectView {
        path: cfg.path.clone(),
        name: cfg.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        root,
        kind: ProjectKind::Php,
        status: ProjectStatus::Missing,
        domain: cfg.domain.clone(),
        aliases: cfg.aliases.clone(),
        docroot: cfg.docroot.clone(),
        php_pin: cfg.php.clone(),
        php_request: None,
        has_node: false,
        node_enabled: cfg.node,
        node_command: cfg.node_command.clone(),
        node_request: None,
        node_pin: cfg.node_version.clone(),
        database: cfg.database.clone(),
        db_version: cfg.db_version.clone(),
        db_port: cfg.db_port,
        mail: cfg.mail.clone(),
        storage: cfg.storage.clone(),
        workers: cfg.workers.clone(),
        reverb_port: cfg.reverb_port,
        queues: cfg.queues.clone(),
        queue_connection: cfg.queue_connection.clone(),
        online: false,
    }
}

/// `loja` → `loja-2` se `loja` já estiver em uso.
pub fn unique_domain(base: &str, used: &[String]) -> String {
    if !used.iter().any(|u| u == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}-{n}")).find(|c| !used.contains(c)).unwrap()
}

/// Limite de domínios extras por projeto (evita pedidos de UAC enormes no hosts).
pub const MAX_ALIASES: usize = 20;

/// Alias: rótulos de domínio separados por ponto (`finder.noveck`).
pub fn validate_alias(alias: &str) -> Result<()> {
    // O gateway reserva esses prefixos para os serviços locais.
    if matches!(alias.split('.').next(), Some("db" | "mail" | "s3")) {
        return Err(Error::Invalid(crate::t!("domínio extra \"{a}\" reservado para um serviço do Larafim", a = alias)));
    }
    let ok = alias.len() <= 200
        && alias.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        });
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(crate::t!(
            "domínio extra \"{a}\" inválido: use letras minúsculas, números, hífen e ponto",
            a = alias
        )))
    }
}

#[cfg(test)]
#[test]
fn aliases_reject_reserved_service_names() {
    for alias in ["db", "db.algo", "mail", "mail.algo", "s3", "s3.algo"] {
        assert!(validate_alias(alias).is_err(), "{alias}");
    }
    for alias in ["db-algo", "app.db", "mailbox", "s3-cache"] {
        assert!(validate_alias(alias).is_ok(), "{alias}");
    }
}

pub fn validate_domain(label: &str) -> Result<()> {
    let ok = !label.is_empty()
        && label.len() <= 63
        && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "domínio \"{label}\" inválido: use letras minúsculas, números e hífen (sugestão: \"{}\")",
            domain_label(label)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn laravel(dir: &Path) {
        fs::create_dir_all(dir.join("public")).unwrap();
        fs::write(dir.join("artisan"), "").unwrap();
        fs::write(dir.join("composer.json"), r#"{"require":{"php":"^8.3"}}"#).unwrap();
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("www");
        let b = tmp.path().join("outros");
        laravel(&a.join("loja"));
        laravel(&b.join("loja"));
        fs::create_dir_all(a.join("blog")).unwrap();
        fs::write(a.join("blog").join("index.php"), "").unwrap();
        (tmp, a, b)
    }

    #[test]
    fn new_projects_get_unique_domains() {
        let (_tmp, a, b) = setup();
        let store = ProjectsFile::default();
        let views = store.views(&[a, b]);
        let mut domains: Vec<_> = views.iter().map(|v| v.domain.as_str()).collect();
        domains.sort();
        assert_eq!(domains, ["blog", "loja", "loja-2"]);
        assert!(views.iter().all(|v| v.status == ProjectStatus::New));
    }

    #[test]
    fn import_edit_remove_ignore() {
        let (tmp, a, b) = setup();
        let roots = vec![a.clone(), b.clone()];
        let mut store = ProjectsFile::default();

        assert_eq!(store.import(&a.join("loja")).unwrap(), "loja");
        assert_eq!(store.import(&b.join("loja")).unwrap(), "loja-2");
        assert!(store.import(&a.join("loja")).is_err(), "não importa duas vezes");

        // Domínio repetido é recusado.
        let mut cfg = store.find(&b.join("loja")).unwrap().clone();
        cfg.domain = "loja".into();
        assert!(store.upsert(cfg.clone()).is_err());
        cfg.domain = "Loja-Nova".into();
        cfg.php = Some("8.3".into());
        store.upsert(cfg).unwrap();
        assert_eq!(store.find(&b.join("loja")).unwrap().domain, "loja-nova");

        // Pasta pública fora do projeto é recusada.
        let mut bad = store.find(&a.join("loja")).unwrap().clone();
        bad.docroot = r"..\..".into();
        assert!(store.upsert(bad).is_err());

        // Salvar e ler de volta.
        let file = tmp.path().join("projects.toml");
        store.save(&file).unwrap();
        let loaded = ProjectsFile::load(&file).unwrap();
        assert_eq!(loaded, store);

        // Remover um projeto de raiz = ignorar (não volta como novo).
        store.remove(&a.join("loja"), &roots);
        let views = store.views(&roots);
        assert!(!views.iter().any(|v| same_path(&v.path, &a.join("loja"))));
        store.unignore(&a.join("loja"));
        let views = store.views(&roots);
        let v = views.iter().find(|v| same_path(&v.path, &a.join("loja"))).unwrap();
        assert_eq!(v.status, ProjectStatus::New);
    }

    #[test]
    fn standalone_and_missing() {
        let (tmp, a, _b) = setup();
        let solo = tmp.path().join("avulso");
        laravel(&solo);
        let mut store = ProjectsFile::default();
        store.import(&solo).unwrap();
        let views = store.views(std::slice::from_ref(&a));
        let v = views.iter().find(|v| v.name == "avulso").unwrap();
        assert_eq!(v.status, ProjectStatus::Imported);
        assert_eq!(v.root, None);
        assert_eq!(v.php_request.as_deref(), Some("^8.3"));

        fs::remove_dir_all(&solo).unwrap();
        let views = store.views(&[a]);
        assert_eq!(views.iter().find(|v| v.name == "avulso").unwrap().status, ProjectStatus::Missing);
    }

    #[test]
    fn paths_compare_like_windows() {
        assert!(same_path(Path::new(r"D:\WWW\Loja\"), Path::new("d:/www/loja")));
        assert!(!same_path(Path::new(r"D:\www\loja"), Path::new(r"D:\www\loja2")));
    }

    #[test]
    fn alias_hosts_normalize_and_skip_main() {
        let d = DetectedProject {
            name: "finder".into(),
            domain: "finder".into(),
            path: PathBuf::from("x"),
            root: PathBuf::from("."),
            kind: ProjectKind::Laravel,
            docroot: "public".into(),
            php: None,
            has_node: false,
            node: None,
            node_command: None,
        };
        let mut cfg = default_config(&d, "finder".into());
        assert!(cfg.alias_hosts("local").is_empty());
        cfg.aliases = vec![" Finder.Noveck ".into(), "finder.hubbi.local".into(), "finder".into(), "".into(), "finder.noveck".into(), "DB.Reservado.local".into(), "mail".into()];
        assert_eq!(cfg.alias_hosts("local"), vec!["finder.noveck.local".to_string(), "finder.hubbi.local".to_string()]);
    }

    #[test]
    fn set_aliases_validates_and_detects_conflicts() {
        let dir = std::env::temp_dir().join(format!("larafim-aliases-{}", std::process::id()));
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let d = |p: &Path, name: &str| DetectedProject {
            name: name.into(),
            domain: name.into(),
            path: p.to_path_buf(),
            root: dir.clone(),
            kind: ProjectKind::Php,
            docroot: String::new(),
            php: None,
            has_node: false,
            node: None,
            node_command: None,
        };
        let mut store = ProjectsFile::default();
        store.upsert(default_config(&d(&a, "finder"), "finder".into())).unwrap();
        store.upsert(default_config(&d(&b, "loja"), "loja".into())).unwrap();

        store.set_aliases(&a, &[" Finder.Noveck.local ".into(), "finder.noveck".into(), "".into()], "local").unwrap();
        assert_eq!(store.find(&a).unwrap().aliases, vec!["finder.noveck".to_string()]);

        assert!(store.set_aliases(&a, &["loja".into()], "local").is_err(), "domínio de outro projeto");
        assert!(store.set_aliases(&b, &["finder.noveck".into()], "local").is_err(), "alias de outro projeto");
        assert!(store.set_aliases(&a, &["finder".into()], "local").is_err(), "domínio principal");
        assert!(store.set_aliases(&a, &["a..b".into()], "local").is_err());
        assert!(store.set_aliases(&a, &["-x".into()], "local").is_err());
        // Falha não altera a lista atual.
        assert_eq!(store.find(&a).unwrap().aliases, vec!["finder.noveck".to_string()]);

        store.set_aliases(&a, &[], "local").unwrap();
        assert!(store.find(&a).unwrap().aliases.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
