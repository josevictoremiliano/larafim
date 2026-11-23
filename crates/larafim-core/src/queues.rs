//! Filas do Laravel: o Laravel não tem uma lista delas (o nome é só um
//! texto em `onQueue('...')`). O Larafim junta o que o projeto declara:
//! a fila da conexão padrão, os supervisores do Horizon do ambiente,
//! `config('queue.names')` e os `onQueue('literal')` do código em `app/`.
//! Tudo aqui é puro (texto → lista), para testar com entrada hostil.

use std::path::Path;

use serde_json::Value;

use crate::safety::is_queue_name;

/// Mais que isso não é uma lista de filas, é lixo.
pub const MAX_QUEUES: usize = 50;

/// Marca antes do JSON na saída do PHP (providers podem imprimir avisos antes).
pub const MARKER: &str = "@@LARAFIM-QUEUES@@";

/// Código para `php -r`, rodado na pasta do projeto: sobe o Laravel só até
/// a configuração e imprime o que interessa. Só aspas simples (vai como
/// argumento, sem shell).
pub const PHP_PROBE: &str = concat!(
    "require 'vendor/autoload.php';",
    "$app = require 'bootstrap/app.php';",
    "$app->make(Illuminate\\Contracts\\Console\\Kernel::class)->bootstrap();",
    "$conn = config('queue.default');",
    "echo PHP_EOL, '@@LARAFIM-QUEUES@@', json_encode([",
    "'default' => config('queue.connections.'.$conn.'.queue'),",
    "'names' => config('queue.names'),",
    "'horizon' => config('horizon.environments.'.app()->environment()),",
    "'horizon_defaults' => config('horizon.defaults'),",
    "]);",
);

fn push(out: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if is_queue_name(name) && !out.iter().any(|q| q == name) && out.len() < MAX_QUEUES {
        out.push(name.to_string());
    }
}

/// Valor de `queue` de um supervisor: `"a"`, `"a,b"` ou `["a", "b"]`.
fn push_queue_value(out: &mut Vec<String>, v: &Value) {
    match v {
        Value::String(s) => s.split(',').for_each(|q| push(out, q)),
        Value::Array(a) => a.iter().filter_map(Value::as_str).for_each(|q| push(out, q)),
        _ => {}
    }
}

/// Filas da saída de `PHP_PROBE`. Sem a marca ou com JSON inválido: nenhuma.
pub fn parse_probe(stdout: &str) -> Vec<String> {
    let Some((_, json)) = stdout.rsplit_once(MARKER) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(d) = v.get("default") {
        push_queue_value(&mut out, d);
    }
    // Supervisores do ambiente atual (e o padrão de todos).
    for key in ["horizon", "horizon_defaults"] {
        if let Some(Value::Object(sups)) = v.get(key) {
            for sup in sups.values() {
                if let Some(q) = sup.get("queue") {
                    push_queue_value(&mut out, q);
                }
            }
        }
    }
    // `config('queue.names')`: convenção comum de mapa nome → fila.
    if let Some(Value::Object(names)) = v.get("names") {
        names.values().filter_map(Value::as_str).for_each(|q| push(&mut out, q));
    }
    out
}

/// `onQueue('nome')` / `onQueue("nome")` com nome literal no código.
pub fn scan_source(text: &str, out: &mut Vec<String>) {
    let mut rest = text;
    while let Some(pos) = rest.find("onQueue(") {
        rest = &rest[pos + "onQueue(".len()..];
        let arg = rest.trim_start();
        let Some(quote) = arg.chars().next().filter(|c| *c == '\'' || *c == '"') else { continue };
        let body = &arg[1..];
        if let Some(end) = body.find(quote) {
            push(out, &body[..end]);
        }
    }
}

/// Varre `app/` do projeto atrás de `onQueue('...')` (limitado para não pesar).
pub fn scan_project(project: &Path) -> Vec<String> {
    const MAX_FILES: usize = 5000;
    const MAX_SIZE: u64 = 512 * 1024;
    let mut out = Vec::new();
    let mut stack = vec![project.join("app")];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue; // não sai do projeto por atalhos
            }
            let path = e.path();
            if ft.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "php") && e.metadata().is_ok_and(|m| m.len() <= MAX_SIZE) {
                seen += 1;
                if seen > MAX_FILES {
                    return out;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    scan_source(&text, &mut out);
                }
            }
        }
    }
    out
}

/// Junta as listas na ordem dada, sem repetir, e garante `default` se vazia.
pub fn merge(lists: &[Vec<String>]) -> Vec<String> {
    let mut out = Vec::new();
    for q in lists.iter().flatten() {
        push(&mut out, q);
    }
    if out.is_empty() {
        out.push("default".into());
    }
    out
}

/// Lista digitada pela pessoa (`a, b, c`) → filas válidas; `Err` com o nome ruim.
pub fn parse_list(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for q in text.split([',', '\n']).map(str::trim).filter(|q| !q.is_empty()) {
        if !is_queue_name(q) {
            return Err(q.to_string());
        }
        push(&mut out, q);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_like_autoai() {
        let out = format!(
            "Aviso qualquer\n{MARKER}{}",
            r#"{"default":"default","names":{"chat":"chat","suppliers":"suppliers","suppliers_fast":"suppliers-fast","hubbi":"hubbi"},
               "horizon":{"supervisor-whatsapp-inbound":{"queue":["whatsapp-inbound"]},"supervisor-whatsapp":{"queue":["whatsapp"]},"supervisor-default":{"queue":["default"]}},
               "horizon_defaults":{"s":{"queue":"default,emails"}}}"#
        );
        // Objetos do JSON vêm em ordem alfabética de chave (a ordem só muda a prioridade).
        assert_eq!(
            parse_probe(&out),
            ["default", "whatsapp", "whatsapp-inbound", "emails", "chat", "hubbi", "suppliers", "suppliers-fast"]
        );
        assert!(parse_probe("sem marca").is_empty());
        assert!(parse_probe(&format!("{MARKER}{{quebrado")).is_empty());
        // Sem Horizon nem queue.names: só a padrão.
        assert_eq!(parse_probe(&format!("{MARKER}{}", r#"{"default":"default","names":null,"horizon":null,"horizon_defaults":null}"#)), ["default"]);
    }

    #[test]
    fn scans_literal_on_queue() {
        let src = r#"$job->onQueue('whatsapp'); X::dispatch()->onQueue( "whatsapp-inbound" );
                     $this->onQueue(config('queue.names.chat')); $this->onQueue($q);"#;
        let mut out = Vec::new();
        scan_source(src, &mut out);
        assert_eq!(out, ["whatsapp", "whatsapp-inbound"]);
        scan_source("onQueue('whatsapp') onQueue('", &mut out); // repetida e sem fim
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn merge_and_defaults() {
        let a = vec!["default".to_string(), "x".into()];
        let b = vec!["x".to_string(), "y".into()];
        assert_eq!(merge(&[a, b]), ["default", "x", "y"]);
        assert_eq!(merge(&[]), ["default"]);
        assert_eq!(parse_list(" a, b ,,a\nc ").unwrap(), ["a", "b", "c"]);
    }

    // ---------- Entrada hostil ----------

    #[test]
    fn hostile_names_never_become_arguments() {
        // Viriam do projeto (config, código) ou da pessoa e iriam para
        // `--queue=...` na linha de comando do cmd.exe.
        let evil = ["a & calc", "x\" --force", "--tries=0", "a;b", "<script>", "a\r\nb", "%PATH%", "..\\..\\CON", "a|b", "a b"];
        for e in evil {
            let mut out = Vec::new();
            scan_source(&format!("onQueue('{e}')"), &mut out);
            assert!(out.is_empty(), "{e:?}");
            assert!(parse_list(e).is_err() || parse_list(e).unwrap().iter().all(|q| is_queue_name(q)), "{e:?}");
            let json = serde_json::json!({ "default": e, "names": { "x": e }, "horizon": { "s": { "queue": [e] } } });
            assert!(parse_probe(&format!("{MARKER}{json}")).is_empty(), "{e:?}");
        }
        // Vírgula num valor do Horizon separa filas; cada parte é validada.
        let json = serde_json::json!({ "default": "ok,a b,--x" });
        assert_eq!(parse_probe(&format!("{MARKER}{json}")), ["ok"]);
        // Lista gigante é cortada.
        let many: Vec<String> = (0..200).map(|i| format!("q{i}")).collect();
        assert_eq!(merge(&[many]).len(), MAX_QUEUES);
    }

    #[test]
    fn scan_project_ignores_outside_app() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("app/Jobs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("vendor/x")).unwrap();
        std::fs::write(tmp.path().join("app/Jobs/A.php"), "<?php $this->onQueue('emails');").unwrap();
        std::fs::write(tmp.path().join("vendor/x/B.php"), "<?php $this->onQueue('vendorq');").unwrap();
        assert_eq!(scan_project(tmp.path()), ["emails"]);
    }
}
