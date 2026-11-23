//! Descoberta de projetos: lê as subpastas das pastas raiz e identifica o
//! tipo de cada uma só olhando arquivos (barato, sem executar nada).

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Laravel,
    Php,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedProject {
    /// Nome da pasta.
    pub name: String,
    /// Nome usado no domínio (`loja-virtual` → `loja-virtual.offline`).
    pub domain: String,
    pub path: PathBuf,
    /// Pasta raiz onde foi encontrado.
    pub root: PathBuf,
    pub kind: ProjectKind,
    /// Pasta pública relativa ao projeto (`public` ou vazio para a raiz).
    pub docroot: String,
    /// Versão do PHP pedida: `.php-version` tem prioridade sobre `composer.json`.
    pub php: Option<String>,
    pub has_node: bool,
    /// Versão do Node pedida (`.nvmrc` ou `engines.node`).
    pub node: Option<String>,
    /// Comando de desenvolvimento sugerido (`npm run dev`, `pnpm dev`…).
    pub node_command: Option<String>,
}

/// Pastas que nunca são projetos.
const SKIP: &[&str] = &["node_modules", "vendor", "$recycle.bin", "system volume information"];

/// Varre as raízes (só o primeiro nível). Pastas inacessíveis são ignoradas.
pub fn scan_roots(roots: &[PathBuf]) -> Vec<DetectedProject> {
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            tracing::warn!(root = %root.display(), "pasta raiz inacessível");
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.to_lowercase().as_str()) {
                continue;
            }
            if let Some(project) = detect(&path, root) {
                found.push(project);
            }
        }
    }
    found.sort_by_key(|p| p.name.to_lowercase());
    found
}

/// Identifica um projeto numa pasta. `None` se não parecer projeto.
pub fn detect(dir: &Path, root: &Path) -> Option<DetectedProject> {
    let has = |f: &str| dir.join(f).exists();
    let composer = read_json(&dir.join("composer.json"));
    let package = read_json(&dir.join("package.json"));

    let is_laravel = has("artisan") && composer.is_some();
    let is_php = is_laravel || composer.is_some() || has("index.php") || has("public/index.php");
    let has_node = package.is_some();

    let kind = if is_laravel {
        ProjectKind::Laravel
    } else if is_php {
        ProjectKind::Php
    } else if has_node {
        ProjectKind::Node
    } else {
        return None;
    };

    let docroot = if is_php && has("public") { "public".into() } else { String::new() };

    let php = if is_php {
        read_trimmed(&dir.join(".php-version")).or_else(|| {
            composer
                .as_ref()
                .and_then(|c| c.pointer("/require/php"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
    } else {
        None
    };

    let node = read_trimmed(&dir.join(".nvmrc")).or_else(|| {
        package
            .as_ref()
            .and_then(|p| p.pointer("/engines/node"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    });

    let node_command = package.as_ref().and_then(|p| node_command(dir, p));

    let name = dir.file_name()?.to_string_lossy().to_string();
    Some(DetectedProject {
        domain: domain_label(&name),
        name,
        path: dir.to_path_buf(),
        root: root.to_path_buf(),
        kind,
        docroot,
        php,
        has_node,
        node,
        node_command,
    })
}

/// Como `detect`, mas para uma pasta escolhida à mão: se não reconhecer o
/// tipo, trata como site PHP simples na raiz.
pub fn detect_any(dir: &Path) -> Option<DetectedProject> {
    let parent = dir.parent().unwrap_or(dir);
    if let Some(p) = detect(dir, parent) {
        return Some(p);
    }
    let name = dir.file_name()?.to_string_lossy().to_string();
    dir.is_dir().then(|| DetectedProject {
        domain: domain_label(&name),
        name,
        path: dir.to_path_buf(),
        root: parent.to_path_buf(),
        kind: ProjectKind::Php,
        docroot: String::new(),
        php: None,
        has_node: false,
        node: None,
        node_command: None,
    })
}

/// Script de desenvolvimento conforme o gerenciador de pacotes do projeto.
fn node_command(dir: &Path, package: &serde_json::Value) -> Option<String> {
    let scripts = package.get("scripts")?;
    let script = ["dev", "start", "serve"].into_iter().find(|s| scripts.get(*s).is_some())?;
    let manager = if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("yarn.lock").exists() {
        "yarn"
    } else if dir.join("bun.lock").exists() || dir.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    };
    Some(match (manager, script) {
        ("npm", "start") => "npm start".into(),
        ("npm" | "bun", s) => format!("{manager} run {s}"),
        (m, s) => format!("{m} {s}"),
    })
}

/// Pasta escolhida à mão → raiz do projeto. Quem escolhe a `public` de um
/// Laravel quer o projeto inteiro: senão o Larafim o veria como PHP simples
/// (sem `artisan`, sem workers) e serviria a pasta errada.
pub fn project_root(dir: &Path) -> PathBuf {
    let Some(parent) = dir.parent() else { return dir.to_path_buf() };
    if detect(dir, parent).is_some_and(|d| d.kind == ProjectKind::Laravel) {
        return dir.to_path_buf();
    }
    let grand = parent.parent().unwrap_or(parent);
    match (dir.file_name(), detect(parent, grand)) {
        (Some(name), Some(p)) if p.kind == ProjectKind::Laravel && name.eq_ignore_ascii_case(p.docroot.as_str()) => {
            parent.to_path_buf()
        }
        _ => dir.to_path_buf(),
    }
}

/// Pastas que podem servir de pasta pública. `""` é a raiz do projeto.
pub fn docroot_options(dir: &Path) -> Vec<String> {
    let mut options = vec![String::new()];
    for name in ["public", "public_html", "web", "www", "htdocs", "dist", "build"] {
        if dir.join(name).is_dir() {
            options.push(name.into());
        }
    }
    options
}

/// Transforma o nome da pasta num rótulo de domínio válido.
pub fn domain_label(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { crate::t!("projeto") } else { out }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim().trim_start_matches('v');
    (!line.is_empty()).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, file: &str, content: &str) {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn detects_kinds_and_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let loja = root.join("Loja Virtual");
        write(&loja, "artisan", "");
        write(&loja, "composer.json", r#"{"require":{"php":"^8.3"}}"#);
        write(&loja, "public/index.php", "");
        write(&loja, "package.json", r#"{"scripts":{"dev":"vite"}}"#);

        let blog = root.join("blog");
        write(&blog, "index.php", "");
        write(&blog, ".php-version", "8.1\n");

        let site = root.join("site");
        write(&site, "package.json", r#"{"engines":{"node":">=20"}}"#);
        write(&site, ".nvmrc", "v22\n");

        fs::create_dir_all(root.join("fotos")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        write(&root.join(".git"), "index.php", "");

        let found = scan_roots(&[root.to_path_buf()]);
        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["blog", "Loja Virtual", "site"]);

        let loja = &found[1];
        assert_eq!(loja.kind, ProjectKind::Laravel);
        assert_eq!(loja.domain, "loja-virtual");
        assert_eq!(loja.docroot, "public");
        assert_eq!(loja.php.as_deref(), Some("^8.3"));
        assert!(loja.has_node);

        let blog = &found[0];
        assert_eq!(blog.kind, ProjectKind::Php);
        assert_eq!(blog.php.as_deref(), Some("8.1"));
        assert_eq!(blog.docroot, "");

        let site = &found[2];
        assert_eq!(site.kind, ProjectKind::Node);
        assert_eq!(site.node.as_deref(), Some("22"));
        assert_eq!(site.php, None);
        assert_eq!(site.node_command, None, "sem scripts no package.json");
        assert_eq!(loja.node_command.as_deref(), Some("npm run dev"));
    }

    #[test]
    fn node_command_follows_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        write(&app, "package.json", r#"{"scripts":{"dev":"vite"}}"#);
        write(&app, "pnpm-lock.yaml", "");
        assert_eq!(detect(&app, tmp.path()).unwrap().node_command.as_deref(), Some("pnpm dev"));

        let api = tmp.path().join("api");
        write(&api, "package.json", r#"{"scripts":{"start":"node index.js"}}"#);
        assert_eq!(detect(&api, tmp.path()).unwrap().node_command.as_deref(), Some("npm start"));
    }

    #[test]
    fn detect_any_and_docroots() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("pasta-qualquer");
        fs::create_dir_all(plain.join("public")).unwrap();
        fs::create_dir_all(plain.join("web")).unwrap();
        let p = detect_any(&plain).unwrap();
        assert_eq!(p.kind, ProjectKind::Php);
        assert_eq!(docroot_options(&plain), ["", "public", "web"]);
        assert!(detect_any(&tmp.path().join("nao-existe")).is_none());
    }

    #[test]
    fn missing_root_is_ignored() {
        assert!(scan_roots(&[PathBuf::from(r"Z:\nao\existe")]).is_empty());
    }

    #[test]
    fn public_folder_points_to_laravel_root() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("autoai");
        write(&app, "artisan", "");
        write(&app, "composer.json", "{}");
        write(&app, "public/index.php", "");
        assert_eq!(project_root(&app.join("public")), app);
        assert_eq!(project_root(&app), app);
        // Outra subpasta, ou `public` de um PHP simples: fica como escolhida.
        fs::create_dir_all(app.join("storage")).unwrap();
        assert_eq!(project_root(&app.join("storage")), app.join("storage"));
        let site = tmp.path().join("site");
        write(&site, "public/index.php", "");
        assert_eq!(project_root(&site.join("public")), site.join("public"));
        // Nomes hostis não sobem de pasta.
        for evil in ["..", "public.", "CON", "PUBLIC~1"] {
            assert_ne!(project_root(&app.join(evil)), app, "{evil}");
        }
    }

    #[test]
    fn domain_labels() {
        assert_eq!(domain_label("Loja_Virtual 2"), "loja-virtual-2");
        assert_eq!(domain_label("--api--"), "api");
        assert_eq!(domain_label("çã"), "projeto");
    }
}
