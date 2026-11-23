//! Laravel Reverb por projeto: cada projeto ganha uma porta própria (para
//! dois projetos não brigarem pela 8080) e o `.env` passa a apontar para ela.
//! Aqui fica só a parte pura (porta e chaves do `.env`), testável sem o app.
//!
//! Dois caminhos até o Reverb:
//! - o Laravel (PHP) publica eventos direto em `127.0.0.1:<porta>` (`REVERB_*`);
//! - o navegador conecta pelo domínio do projeto (`VITE_REVERB_*`), e o
//!   gateway repassa `/app/...` para a porta. Numa página https o navegador
//!   só aceita `wss://` no mesmo nível de segurança, e o certificado do
//!   Larafim cobre o domínio, não `127.0.0.1`.

use std::ops::RangeInclusive;
use std::path::Path;

use crate::envfile::{EnvChange, get};

/// Faixa onde as portas do Reverb são escolhidas (8080 é o padrão do Reverb).
pub const PORTS: RangeInclusive<u16> = 8080..=8179;

/// O Reverb escuta só na máquina.
pub const HOST: &str = "127.0.0.1";

/// O projeto tem o Reverb pronto para uso: pacote baixado e conhecido pelo
/// autoload (junto com o cliente Pusher, que o broadcaster usa).
pub fn is_installed(project: &Path) -> bool {
    project.join("vendor").join("laravel").join("reverb").is_dir()
        && crate::composer::autoloads(project, "Laravel\\Reverb")
        && crate::composer::autoloads(project, "Pusher")
}

/// Primeira porta da faixa fora de `reserved` e livre segundo `is_free`.
pub fn pick_port(reserved: &[u16], is_free: impl Fn(u16) -> bool) -> Option<u16> {
    PORTS.clone().find(|p| !reserved.contains(p) && is_free(*p))
}

/// O Reverb consulta `laravel:reverb:restart` ao iniciar com cache no banco.
pub fn missing_cache_table(line: &str) -> bool {
    line.contains("laravel:reverb:restart")
        && ["SQLSTATE[42S02]", "SQLSTATE[42P01]", "no such table:"].iter().any(|code| line.contains(code))
}

/// Credenciais novas, usadas só quando o `.env` ainda não tem as suas.
pub struct Credentials {
    pub app_id: String,
    pub app_key: String,
    pub app_secret: String,
}

/// Onde o navegador encontra o Reverb: o domínio do projeto no gateway.
pub struct Public {
    /// `loja.offline`
    pub host: String,
    pub port: u16,
    pub https: bool,
}

/// Valor atual não vazio de uma chave.
fn current(text: &str, key: &str) -> Option<String> {
    get(text, key).filter(|v| !v.is_empty())
}

/// Mudanças no `.env` para usar o Reverb do Larafim na porta `port`.
///
/// Mantém as credenciais que já existem (trocar a chave quebraria clientes
/// já configurados) e respeita `VITE_REVERB_APP_KEY="${REVERB_APP_KEY}"`.
pub fn env_changes(text: &str, port: u16, public: &Public, fresh: &Credentials) -> Vec<EnvChange> {
    let port = port.to_string();
    let mut changes = vec![EnvChange::set("BROADCAST_CONNECTION", "reverb")];
    // Laravel 10 usa o nome antigo.
    if get(text, "BROADCAST_DRIVER").is_some() {
        changes.push(EnvChange::set("BROADCAST_DRIVER", "reverb"));
    }
    let mut cred = |key: &str, new: &str| -> String {
        current(text, key).unwrap_or_else(|| {
            changes.push(EnvChange::set(key, new));
            new.to_string()
        })
    };
    cred("REVERB_APP_ID", &fresh.app_id);
    let app_key = cred("REVERB_APP_KEY", &fresh.app_key);
    cred("REVERB_APP_SECRET", &fresh.app_secret);

    let public_port = public.port.to_string();
    let public_scheme = if public.https { "https" } else { "http" };
    for (key, value) in [
        // Laravel → Reverb, direto na porta.
        ("REVERB_HOST", HOST),
        ("REVERB_PORT", port.as_str()),
        ("REVERB_SCHEME", "http"),
        ("REVERB_SERVER_HOST", HOST),
        ("REVERB_SERVER_PORT", port.as_str()),
        // Navegador → gateway → Reverb.
        ("VITE_REVERB_HOST", public.host.as_str()),
        ("VITE_REVERB_PORT", public_port.as_str()),
        ("VITE_REVERB_SCHEME", public_scheme),
    ] {
        changes.push(EnvChange::set(key, value));
    }
    if get(text, "VITE_REVERB_APP_KEY").as_deref() != Some("${REVERB_APP_KEY}") {
        changes.push(EnvChange::set("VITE_REVERB_APP_KEY", &app_key));
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envfile::{apply, validate};

    fn fresh() -> Credentials {
        Credentials { app_id: "123456".into(), app_key: "chavenova".into(), app_secret: "segredonovo".into() }
    }

    fn public(https: bool) -> Public {
        Public { host: "loja.offline".into(), port: if https { 443 } else { 80 }, https }
    }

    #[test]
    fn picks_first_free_port_outside_reserved() {
        assert_eq!(pick_port(&[], |_| true), Some(8080));
        assert_eq!(pick_port(&[8080, 8081], |_| true), Some(8082));
        assert_eq!(pick_port(&[8080], |p| p != 8081), Some(8082));
        assert_eq!(pick_port(&[], |_| false), None);
    }

    #[test]
    fn identifies_missing_database_cache_in_reverb_output() {
        assert!(missing_cache_table("SQLSTATE[42S02]: Table 'demo.cache' doesn't exist (SQL: select * from `cache` where `key` = laravel:reverb:restart)"));
        assert!(missing_cache_table("SQLSTATE[HY000]: General error: 1 no such table: cache (SQL: select * where key = laravel:reverb:restart)"));
        assert!(!missing_cache_table("SQLSTATE[42S02]: Table 'demo.sessions' doesn't exist"));
        assert!(!missing_cache_table("SQLSTATE[HY000] [2002]: Connection refused (key = laravel:reverb:restart)"));
    }

    #[test]
    fn fills_everything_on_empty_env() {
        let (out, _) = apply("APP_NAME=Loja\n", &env_changes("APP_NAME=Loja\n", 8081, &public(true), &fresh()));
        for (k, v) in [
            ("BROADCAST_CONNECTION", "reverb"),
            ("REVERB_APP_ID", "123456"),
            ("REVERB_APP_KEY", "chavenova"),
            ("REVERB_APP_SECRET", "segredonovo"),
            ("REVERB_HOST", "127.0.0.1"),
            ("REVERB_PORT", "8081"),
            ("REVERB_SCHEME", "http"),
            ("REVERB_SERVER_HOST", "127.0.0.1"),
            ("REVERB_SERVER_PORT", "8081"),
            ("VITE_REVERB_APP_KEY", "chavenova"),
            ("VITE_REVERB_HOST", "loja.offline"),
            ("VITE_REVERB_PORT", "443"),
            ("VITE_REVERB_SCHEME", "https"),
        ] {
            assert_eq!(get(&out, k).as_deref(), Some(v), "{k}");
        }
        assert_eq!(get(&out, "BROADCAST_DRIVER"), None, "não cria a chave antiga");
    }

    #[test]
    fn keeps_credentials_and_fixes_browser_side() {
        // Caso real: REVERB_PORT repetido e VITE_ seguindo o valor do servidor.
        let env = "BROADCAST_DRIVER=log\nREVERB_APP_ID=999\nREVERB_APP_KEY=antiga\nREVERB_APP_SECRET=s\nREVERB_PORT=8080\n\
                   REVERB_PORT=6009\nVITE_REVERB_APP_KEY=\"${REVERB_APP_KEY}\"\nVITE_REVERB_PORT=\"${REVERB_PORT}\"\nVITE_REVERB_HOST=localhost\n";
        let (out, diff) = apply(env, &env_changes(env, 8082, &public(false), &fresh()));
        assert_eq!(get(&out, "BROADCAST_DRIVER").as_deref(), Some("reverb"));
        assert_eq!(get(&out, "REVERB_APP_KEY").as_deref(), Some("antiga"));
        assert_eq!(get(&out, "REVERB_PORT").as_deref(), Some("8082"));
        assert!(!out.contains("6009"), "{out}");
        assert_eq!(get(&out, "VITE_REVERB_PORT").as_deref(), Some("80"));
        assert_eq!(get(&out, "VITE_REVERB_HOST").as_deref(), Some("loja.offline"));
        assert_eq!(get(&out, "VITE_REVERB_SCHEME").as_deref(), Some("http"));
        assert_eq!(get(&out, "VITE_REVERB_APP_KEY").as_deref(), Some("${REVERB_APP_KEY}"));
        assert!(!diff.iter().any(|d| d.key.starts_with("REVERB_APP_") || d.key == "VITE_REVERB_APP_KEY"));
    }

    // ---------- Entrada hostil ----------

    #[test]
    fn hostile_env_cannot_inject_lines() {
        // Valores do .env do projeto voltam para o arquivo (a chave vai para o
        // VITE_): aspas, `;`, `${}` e `<script>` ficam literais numa linha só.
        for evil in ["a\";EVIL=1", "${APP_KEY}", "<script>x</script>", "x' ; rm -rf", "..\\..\\CON"] {
            let env = format!("REVERB_APP_KEY='{}'\n", evil.replace('\'', ""));
            let changes = env_changes(&env, 8080, &public(true), &fresh());
            assert!(validate(&changes).is_ok());
            let (out, _) = apply(&env, &changes);
            assert!(get(&out, "EVIL").is_none(), "{out}");
            assert_eq!(get(&out, "VITE_REVERB_APP_KEY"), get(&out, "REVERB_APP_KEY"), "{out}");
        }
        // Quebra de linha nas credenciais novas é recusada pelo envfile.
        let bad = Credentials { app_id: "1\r\nAPP_DEBUG=true".into(), app_key: "k".into(), app_secret: "s".into() };
        assert!(validate(&env_changes("", 8080, &public(true), &bad)).is_err());
    }
}
