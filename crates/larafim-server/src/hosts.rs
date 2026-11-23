//! Bloco do Larafim no arquivo `hosts` do Windows:
//!
//! ```text
//! # BEGIN LARAFIM — gerenciado pelo Larafim, não edite
//! 127.0.0.1 loja.offline
//! # END LARAFIM
//! ```
//!
//! Escrever no `hosts` exige administrador. O Larafim roda a si mesmo
//! elevado (UAC) só para essa tarefa, e o ajudante só aceita linhas no formato
//! acima — nada além do bloco do Larafim é alterado.

use larafim_core::t;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const HELPER_FLAG: &str = "--larafim-write-hosts";
const BEGIN: &str = "# BEGIN LARAFIM";
const END: &str = "# END LARAFIM";
const MAX_ENTRIES: usize = 2000;

#[derive(Debug, thiserror::Error)]
pub enum HostsError {
    #[error("{}", larafim_core::i18n::tr("permissão negada: sem ela os endereços .offline não funcionam neste PC"))]
    Denied,
    #[error("{}", larafim_core::i18n::tf("não foi possível atualizar o arquivo hosts ({0}). Um antivírus pode estar bloqueando.", &[("0", &.0 as &dyn std::fmt::Display)]))]
    Failed(String),
}

pub fn hosts_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("drivers").join("etc").join("hosts")
}

/// Nome de host aceito no bloco (letras minúsculas, números, hífen e pontos).
pub fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// Hosts que estão hoje no bloco do Larafim.
pub fn block_entries(text: &str) -> BTreeSet<String> {
    let mut inside = false;
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with(BEGIN) {
            inside = true;
        } else if t.starts_with(END) {
            inside = false;
        } else if inside && let Some(host) = t.strip_prefix("127.0.0.1").map(str::trim) {
            out.insert(host.to_string());
        }
    }
    out
}

fn render_block(hosts: &BTreeSet<String>) -> String {
    let mut s = format!("{BEGIN} — gerenciado pelo Larafim, não edite\r\n");
    for h in hosts {
        s.push_str(&format!("127.0.0.1 {h}\r\n"));
    }
    s.push_str(END);
    s.push_str("\r\n");
    s
}

/// Troca o bloco do Larafim (ou acrescenta no fim). O resto fica intacto.
/// Lista vazia remove o bloco.
pub fn apply_block(text: &str, hosts: &BTreeSet<String>) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut replaced = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with(BEGIN) {
            inside = true;
            if !hosts.is_empty() {
                out.push_str(&render_block(hosts));
            }
            replaced = true;
            continue;
        }
        if inside {
            if t.starts_with(END) {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    if !replaced && !hosts.is_empty() {
        if !out.is_empty() && !out.ends_with("\r\n\r\n") {
            out.push_str("\r\n");
        }
        out.push_str(&render_block(hosts));
    }
    out
}

/// `true` se o `hosts` atual já tem exatamente esses domínios.
pub fn is_up_to_date(wanted: &BTreeSet<String>) -> bool {
    std::fs::read_to_string(hosts_path()).map(|t| block_entries(&t) == *wanted).unwrap_or(false)
}

/// Pede ao Windows (UAC) para rodar o ajudante e gravar o bloco.
/// `work_dir` é onde fica o arquivo com a lista pedida; `exe` é o Larafim a
/// rodar elevado (no pacote MSIX, o alias — o caminho do pacote não serve).
pub fn request_update(wanted: &BTreeSet<String>, work_dir: &Path, exe: &Path) -> Result<(), HostsError> {
    if wanted.iter().any(|h| !valid_host(h)) || wanted.len() > MAX_ENTRIES {
        return Err(HostsError::Failed(t!("lista de domínios inválida")));
    }
    let request = work_dir.join("hosts-request.txt");
    let list: Vec<&str> = wanted.iter().map(String::as_str).collect();
    std::fs::write(&request, list.join("\n")).map_err(|e| HostsError::Failed(e.to_string()))?;

    let status = runas::Command::new(exe)
        .arg(HELPER_FLAG)
        .arg(&request)
        .show(false)
        .gui(true)
        .status();
    let _ = std::fs::remove_file(&request);
    match status {
        Ok(s) if s.success() => {
            tracing::info!(domains = wanted.len(), "arquivo hosts atualizado");
            Ok(())
        }
        Ok(s) => Err(HostsError::Failed(t!("código {a}", a = format!("{:?}", s.code())))),
        // 1223 = ERROR_CANCELLED: a pessoa clicou em "Não" no UAC.
        Err(e) if e.raw_os_error() == Some(1223) => Err(HostsError::Denied),
        Err(e) => Err(HostsError::Failed(e.to_string())),
    }
}

/// Ponto de entrada do ajudante elevado. Devolve o código de saída se os
/// argumentos forem do ajudante; `None` = execução normal do app.
pub fn run_helper_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(HELPER_FLAG) {
        return None;
    }
    let Some(request) = args.get(2) else { return Some(2) };
    Some(match write_from_request(Path::new(request)) {
        Ok(()) => 0,
        Err(_) => 1,
    })
}

fn write_from_request(request: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(request)?;
    let wanted: BTreeSet<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    // O ajudante roda como administrador: valida tudo de novo.
    if wanted.len() > MAX_ENTRIES || wanted.iter().any(|h| !valid_host(h)) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "lista inválida"));
    }
    let path = hosts_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    // Cópia de segurança do hosts original, feita uma única vez.
    let backup = path.with_file_name("hosts.larafim-backup");
    if !backup.exists() {
        let _ = std::fs::write(&backup, &current);
    }
    std::fs::write(&path, apply_block(&current, &wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    const ORIGINAL: &str = "# Copyright Microsoft\r\n127.0.0.1 localhost\r\n10.0.0.5 servidor.empresa\r\n";

    #[test]
    fn add_replace_remove_block() {
        let added = apply_block(ORIGINAL, &set(&["loja.offline", "blog.offline"]));
        assert!(added.starts_with(ORIGINAL));
        assert_eq!(block_entries(&added), set(&["blog.offline", "loja.offline"]));

        let replaced = apply_block(&added, &set(&["loja.offline"]));
        assert_eq!(block_entries(&replaced), set(&["loja.offline"]));
        assert_eq!(replaced.matches(BEGIN).count(), 1);
        assert!(replaced.contains("10.0.0.5 servidor.empresa"));

        let removed = apply_block(&replaced, &BTreeSet::new());
        assert!(!removed.contains(BEGIN));
        assert!(removed.contains("127.0.0.1 localhost"));
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("loja-virtual.offline"));
        assert!(!valid_host("Loja.offline"));
        assert!(!valid_host("loja.offline\r\n0.0.0.0 banco.com"));
        assert!(!valid_host("-x.offline"));
        assert!(!valid_host("a..b"));
    }
}
