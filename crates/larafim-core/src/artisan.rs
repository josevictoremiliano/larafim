//! Comandos rápidos do projeto (`php artisan …`, `composer …`) digitados ou
//! escolhidos no painel. Viram argumentos de processo (sem shell), então
//! aqui só se separa o texto em argumentos e se recusa o que não cabe.

/// Tamanho máximo do comando digitado.
pub const MAX_LEN: usize = 500;

/// Comandos que ficam rodando (servidor, worker, console): sem terminal eles
/// travariam o botão. Para esses há os workers ou o terminal do projeto.
const LONG_RUNNING: &[&str] = &[
    "serve", "tinker", "queue:work", "queue:listen", "schedule:work", "reverb:start", "horizon", "pail",
    "octane:start", "dev",
];

/// Separa `texto` em argumentos, como um terminal faria com aspas simples ou
/// duplas (sem expansão de variáveis nem nada do shell). `Err` com o motivo.
pub fn split_args(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    if text.len() > MAX_LEN {
        return Err(crate::t!("comando grande demais"));
    }
    if text.chars().any(char::is_control) {
        return Err(crate::t!("o comando não pode ter quebra de linha ou caracteres de controle"));
    }
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(crate::t!("aspas sem fechar no comando"));
    }
    if started {
        args.push(cur);
    }
    Ok(args)
}

/// Argumentos de `php artisan` para o texto digitado (sem o `php artisan`).
pub fn artisan_args(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    let text = text.strip_prefix("php artisan ").or_else(|| text.strip_prefix("artisan ")).unwrap_or(text);
    let args = split_args(text)?;
    let Some(name) = args.first() else { return Err(crate::t!("digite um comando do artisan")) };
    // O primeiro argumento é o nome do comando, nunca uma opção (`--env=...`
    // antes do nome, por exemplo, confunde a leitura do que vai rodar).
    if name.starts_with('-') || !name.chars().all(|c| c.is_ascii_alphanumeric() || ":-_.".contains(c)) {
        return Err(crate::t!("comando do artisan inválido: {a}", a = name.escape_debug()));
    }
    if LONG_RUNNING.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(crate::t!(
            "o {a} fica rodando: use o terminal do projeto ou os workers",
            a = name
        ));
    }
    Ok(args)
}

/// Ações rápidas do Composer (lista fechada: nada digitado).
pub fn composer_args(action: &str) -> Option<Vec<String>> {
    let args: &[&str] = match action {
        "install" => &["install"],
        "dump-autoload" => &["dump-autoload"],
        _ => return None,
    };
    Some(args.iter().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_terminal() {
        assert_eq!(split_args("migrate --seed").unwrap(), ["migrate", "--seed"]);
        assert_eq!(split_args("  make:model  Post -m ").unwrap(), ["make:model", "Post", "-m"]);
        assert_eq!(split_args(r#"db:seed --class="Database\Seeders\UserSeeder""#).unwrap(), ["db:seed", r"--class=Database\Seeders\UserSeeder"]);
        assert_eq!(split_args("x 'a b' \"\"").unwrap(), ["x", "a b", ""]);
        assert!(split_args("").unwrap().is_empty());
    }

    #[test]
    fn artisan_presets_and_prefix() {
        assert_eq!(artisan_args("php artisan optimize:clear").unwrap(), ["optimize:clear"]);
        assert_eq!(artisan_args("cache:clear").unwrap(), ["cache:clear"]);
        assert!(artisan_args("   ").is_err());
    }

    // ---------- Entrada hostil ----------

    #[test]
    fn hostile_commands_are_refused_or_stay_literal() {
        // Sem shell: `&`, `|`, `;` e `%VAR%` ficam como texto de um argumento.
        assert_eq!(artisan_args("route:list & calc").unwrap(), ["route:list", "&", "calc"]);
        assert_eq!(artisan_args("about %PATH% | more").unwrap(), ["about", "%PATH%", "|", "more"]);
        for bad in [
            "migrate\r\nEVIL", "migrate\0", "--env=production migrate", "-q", "\"sem fechar", "..\\..\\x", "a;b",
            "<script>", "serve", "tinker", "queue:work --once", "SERVE",
        ] {
            assert!(artisan_args(bad).is_err(), "{bad:?}");
        }
        assert!(artisan_args(&format!("about {}", "x".repeat(MAX_LEN))).is_err());
    }

    #[test]
    fn composer_is_a_closed_list() {
        assert_eq!(composer_args("install").unwrap(), ["install"]);
        for bad in ["require evil/pkg", "install; calc", "", "run-script x"] {
            assert!(composer_args(bad).is_none(), "{bad:?}");
        }
    }
}
