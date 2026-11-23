//! Geração e edição do `php.ini` (texto puro, preservando comentários).

use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

/// Extensões ligadas por padrão (só as que existirem na pasta `ext`).
/// `gd2` é o nome antigo do `gd` (PHP 7.4).
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "curl", "fileinfo", "gd", "gd2", "intl", "mbstring", "exif", "mysqli", "openssl",
    "pdo_mysql", "pdo_sqlite", "sqlite3", "sodium", "zip",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Extension {
    pub name: String,
    pub enabled: bool,
    /// `zend_extension` (ex.: opcache, xdebug).
    pub zend: bool,
}

/// Uma linha `extension=...` (ligada ou comentada). Linhas de documentação
/// (`;   extension=modulename`, com espaço depois do `;`) não contam.
struct ExtLine {
    name: String,
    enabled: bool,
    zend: bool,
}

fn parse_ext_line(line: &str) -> Option<ExtLine> {
    let trimmed = line.trim_end();
    let (enabled, rest) = match trimmed.strip_prefix(';') {
        Some(rest) => (false, rest),
        None => (true, trimmed),
    };
    let (zend, after_key) = match rest.strip_prefix("zend_extension") {
        Some(r) => (true, r),
        None => (false, rest.strip_prefix("extension")?),
    };
    let value = after_key.trim_start().strip_prefix('=')?.trim().trim_matches('"');
    let name = value.trim_start_matches("php_").trim_end_matches(".dll");
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then(|| ExtLine { name: name.to_string(), enabled, zend })
}

fn dll_exists(ext_dir: &Path, name: &str) -> bool {
    ext_dir.join(format!("php_{name}.dll")).is_file()
}

/// Troca (ou cria) uma diretiva `chave = valor`. Usa a primeira ocorrência,
/// comentada ou não.
pub fn set_directive(ini: &str, key: &str, value: &str) -> String {
    let mut done = false;
    let mut out: Vec<String> = ini
        .lines()
        .map(|line| {
            if done {
                return line.to_string();
            }
            let body = line.trim_start().trim_start_matches(';').trim_start();
            let is_key = body
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='));
            if is_key {
                done = true;
                format!("{key} = {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !done {
        out.push(format!("{key} = {value}"));
    }
    out.join("\r\n") + "\r\n"
}

/// Atualiza um `php.ini` gerado por versões antigas do Larafim: só troca
/// `error_reporting = E_ALL` (padrão do php.ini-development) para não exibir
/// avisos de obsolescência. Se a pessoa mudou o valor, nada é alterado.
pub fn upgrade(ini: &str) -> Option<String> {
    let old = ini.lines().any(|l| {
        let mut parts = l.splitn(2, '=');
        parts.next().map(str::trim) == Some("error_reporting") && parts.next().map(str::trim) == Some("E_ALL")
    });
    old.then(|| set_directive(ini, "error_reporting", "E_ALL & ~E_DEPRECATED"))
}

/// Atualiza o `php.ini` de cada PHP instalado (ver [`upgrade`]).
pub fn upgrade_all(php_root: &Path) {
    let Ok(dirs) = std::fs::read_dir(php_root) else { return };
    for dir in dirs.flatten() {
        let path = dir.path().join("php.ini");
        if let Some(new) = read(&path).ok().and_then(|t| upgrade(&t))
            && let Err(e) = write(&path, &new)
        {
            tracing::warn!(?path, "não deu para atualizar o php.ini: {e}");
        }
    }
}

/// Liga/desliga uma extensão. Se a linha não existir, acrescenta no fim.
pub fn set_extension(ini: &str, name: &str, enabled: bool, zend: bool) -> String {
    let mut done = false;
    let key = if zend { "zend_extension" } else { "extension" };
    let mut out: Vec<String> = ini
        .lines()
        .map(|line| match parse_ext_line(line) {
            Some(ext) if !done && ext.name.eq_ignore_ascii_case(name) => {
                done = true;
                let prefix = if enabled { "" } else { ";" };
                format!("{prefix}{key}={name}")
            }
            _ => line.to_string(),
        })
        .collect();
    if !done && enabled {
        out.push(format!("{key}={name}"));
    }
    out.join("\r\n") + "\r\n"
}

/// Extensões do `php.ini` que existem de fato na pasta `ext`, em ordem alfabética.
pub fn list_extensions(ini: &str, ext_dir: &Path) -> Vec<Extension> {
    let mut map = std::collections::BTreeMap::<String, Extension>::new();
    for ext in ini.lines().filter_map(parse_ext_line) {
        if !dll_exists(ext_dir, &ext.name) {
            continue;
        }
        let key = ext.name.to_lowercase();
        // Se aparecer mais de uma vez, vale estar ligada em qualquer uma.
        let entry = map.entry(key).or_insert(Extension { name: ext.name, enabled: false, zend: ext.zend });
        entry.enabled |= ext.enabled;
    }
    map.into_values().collect()
}

/// Cria o `php.ini` de uma instalação nova a partir do `php.ini-development`.
/// `php_dir` é onde o PHP vai ficar (vai para os caminhos do ini);
/// `files_dir` é onde os arquivos estão agora (pasta temporária da instalação).
pub fn generate(dev_ini: &str, php_dir: &Path, files_dir: &Path, cacert: &Path) -> String {
    let ext_dir = php_dir.join("ext");
    let present = files_dir.join("ext");
    let quoted = |p: &Path| format!("\"{}\"", p.display());

    let mut ini = format!(
        "; Gerado pelo Larafim a partir do php.ini-development.\r\n\
         ; Pode editar à vontade: as mudanças são mantidas nas atualizações.\r\n\r\n{dev_ini}"
    );
    ini = set_directive(&ini, "extension_dir", &quoted(&ext_dir));
    for name in DEFAULT_EXTENSIONS {
        if dll_exists(&present, name) {
            ini = set_extension(&ini, name, true, false);
        }
    }
    if dll_exists(&present, "opcache") {
        ini = set_extension(&ini, "opcache", true, true);
    }
    for (key, value) in [
        ("memory_limit", "512M"),
        // Avisos de obsolescência de versões novas do PHP quebram a página antes
        // do Laravel assumir; o Laravel ainda os registra no log dele.
        ("error_reporting", "E_ALL & ~E_DEPRECATED"),
        ("upload_max_filesize", "64M"),
        ("post_max_size", "64M"),
        ("max_execution_time", "120"),
        ("curl.cainfo", quoted(cacert).as_str()),
        ("openssl.cafile", quoted(cacert).as_str()),
    ] {
        ini = set_directive(&ini, key, value);
    }
    ini
}

/// Leva o `php.ini` de uma versão anterior para a nova (atualização de patch),
/// trocando os caminhos da pasta antiga pelos da nova.
pub fn migrate(old_ini: &str, old_dir: &Path, new_dir: &Path) -> String {
    old_ini.replace(&old_dir.display().to_string(), &new_dir.display().to_string())
}

pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path, e))
}

pub fn write(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("ini.tmp");
    std::fs::write(&tmp, text).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "[PHP]\r\n\
        ; If you wish to have an extension loaded automatically, use the following\r\n\
        ;   extension=modulename\r\n\
        ;   extension=mysqli\r\n\
        ; extension_dir = \"./\"\r\n\
        ; On windows:\r\n\
        ;extension_dir = \"ext\"\r\n\
        memory_limit = 128M\r\n\
        ;extension=curl\r\n\
        ;extension=gd\r\n\
        ;extension=mysqli\r\n\
        ;extension=xsl\r\n\
        ;zend_extension=opcache\r\n\
        ;curl.cainfo =\r\n";

    fn php_dir_with(exts: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("ext")).unwrap();
        for e in exts {
            std::fs::write(tmp.path().join("ext").join(format!("php_{e}.dll")), "").unwrap();
        }
        tmp
    }

    #[test]
    fn generate_enables_existing_defaults_only() {
        let dir = php_dir_with(&["curl", "mysqli", "xsl", "opcache"]);
        let cacert = dir.path().join("cacert.pem");
        let ini = generate(DEV, dir.path(), dir.path(), &cacert);

        assert!(ini.contains(&format!("extension_dir = \"{}\"", dir.path().join("ext").display())));
        assert!(ini.contains("\r\nextension=curl\r\n"));
        assert!(ini.contains("\r\nextension=mysqli\r\n"));
        assert!(ini.contains(";extension=gd\r\n"), "gd não existe na pasta ext");
        assert!(ini.contains(";extension=xsl\r\n"), "xsl não é padrão");
        assert!(ini.contains("\r\nzend_extension=opcache\r\n"));
        assert!(ini.contains("memory_limit = 512M"));
        assert!(ini.contains("error_reporting = E_ALL & ~E_DEPRECATED"));
        assert!(ini.contains(&format!("curl.cainfo = \"{}\"", cacert.display())));
        // a linha de documentação continua comentada
        assert!(ini.contains(";   extension=mysqli"));
    }

    #[test]
    fn upgrade_only_touches_default() {
        let new = upgrade("; error_reporting\r\nerror_reporting = E_ALL\r\n").unwrap();
        assert!(new.contains("error_reporting = E_ALL & ~E_DEPRECATED"));
        assert!(upgrade(&new).is_none());
        assert!(upgrade("error_reporting = E_ALL & ~E_NOTICE\r\n").is_none());
    }

    #[test]
    fn list_and_toggle_extensions() {
        let dir = php_dir_with(&["curl", "mysqli", "xsl"]);
        let ini = generate(DEV, dir.path(), dir.path(), Path::new("c.pem"));
        let exts = list_extensions(&ini, &dir.path().join("ext"));
        let names: Vec<_> = exts.iter().map(|e| (e.name.as_str(), e.enabled)).collect();
        assert_eq!(names, [("curl", true), ("mysqli", true), ("xsl", false)]);

        let ini = set_extension(&ini, "xsl", true, false);
        let ini = set_extension(&ini, "curl", false, false);
        let exts = list_extensions(&ini, &dir.path().join("ext"));
        let names: Vec<_> = exts.iter().map(|e| (e.name.as_str(), e.enabled)).collect();
        assert_eq!(names, [("curl", false), ("mysqli", true), ("xsl", true)]);
    }

    #[test]
    fn migrate_rewrites_paths() {
        let old = "extension_dir = \"C:\\S\\php\\8.4.27\\ext\"\r\nmemory_limit = 1G\r\n";
        let new = migrate(old, Path::new(r"C:\S\php\8.4.27"), Path::new(r"C:\S\php\8.4.28"));
        assert!(new.contains(r#""C:\S\php\8.4.28\ext""#));
        assert!(new.contains("memory_limit = 1G"), "customização mantida");
    }
}
