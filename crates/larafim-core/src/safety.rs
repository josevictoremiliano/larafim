//! Checagens para textos que vêm de fora do Larafim: catálogos baixados da
//! internet, arquivos dos projetos, nomes dentro de zips. Tudo que vira nome
//! de pasta, URL de download ou linha de arquivo de configuração passa por aqui.

/// Nomes que o Windows reserva para dispositivos (`CON`, `NUL.txt`…).
pub fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Um único nome de pasta/arquivo seguro para usar em `Path::join`, como a
/// versão de um runtime ("11.8.3", "v1.34.0", "8.4.28-nts").
/// Recusa separadores, `..`, nomes ocultos, `:` (fluxos alternativos do NTFS),
/// nomes reservados e qualquer caractere fora de `[A-Za-z0-9._+-]`.
pub fn is_safe_component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('.')
        && !s.ends_with('.')
        && !s.contains("..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        && !is_reserved_windows_name(s)
}

/// SHA-256 em hexadecimal (64 caracteres).
pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// URL `https://` cujo host é exatamente um dos permitidos (sem usuário,
/// senha nem porta, que poderiam disfarçar outro destino).
pub fn is_https_url_on(url: &str, hosts: &[&str]) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains(['@', ':', '\\']) || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    hosts.iter().any(|h| authority.eq_ignore_ascii_case(h))
}

/// Endereço que o Larafim pode abrir no navegador: `http(s)://<algo>.<tld>`
/// (um projeto) ou `http://localhost`/`127.0.0.1`, com porta opcional.
/// O host só pode ter letras, números, `.` e `-` (nada de `user@`, `\`, `%`).
pub fn is_local_url(url: &str, tld: &str) -> bool {
    let (https, rest) = match (url.strip_prefix("http://"), url.strip_prefix("https://")) {
        (Some(r), _) => (false, r),
        (_, Some(r)) => (true, r),
        _ => return false,
    };
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let host = host.to_ascii_lowercase();
    let clean = !host.is_empty() && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
    if !clean || host.contains("..") || host.starts_with('.') {
        return false;
    }
    host.ends_with(&format!(".{}", tld.to_ascii_lowercase())) || (!https && (host == "localhost" || host == "127.0.0.1"))
}

/// Chave de `.env`: letras, números e `_`, sem começar com número.
pub fn is_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && !key.as_bytes()[0].is_ascii_digit()
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Valor de `.env` aceitável: sem quebra de linha nem caracteres de controle
/// (uma quebra de linha permitiria criar outras variáveis no arquivo).
pub fn is_env_value(value: &str) -> bool {
    value.len() <= 4096 && !value.chars().any(char::is_control)
}

/// Nome de fila do Laravel aceitável como argumento (`--queue=a,b`): sem
/// vírgula (separaria outra fila), espaço, aspas ou `-` no começo (viraria opção).
pub fn is_queue_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

/// Nome de distribuição do WSL (sai de `wsl -l`): vira argumento de `wsl -d`.
pub fn is_wsl_distro(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components() {
        for ok in ["11.8.3", "v1.34.0", "8.4.28-nts", "17", "1.0.0-alpha.1", "a+b"] {
            assert!(is_safe_component(ok), "{ok}");
        }
        for bad in [
            "", ".", "..", "../x", "..\\x", "a/b", "a\\b", "C:", "x:stream", ".hidden", "1.0.", "a..b", "CON", "nul",
            "com1", "LPT9.txt", "x y", "x\0", "x\n", "%TEMP%", "~1", "é",
        ] {
            assert!(!is_safe_component(bad), "{bad:?}");
        }
        assert!(!is_safe_component(&"a".repeat(101)));
        assert!(is_safe_component("COM0") && is_safe_component("CONSOLE"));
    }

    #[test]
    fn urls() {
        let hosts = ["github.com", "downloads.php.net"];
        assert!(is_https_url_on("https://github.com/a/b.zip", &hosts));
        assert!(is_https_url_on("https://GitHub.com/a", &hosts));
        for bad in [
            "http://github.com/a",
            "https://github.com.evil.com/a",
            "https://evil.com/github.com",
            "https://github.com@evil.com/a",
            "https://github.com:443@evil.com/",
            "https://github.com:8080/a",
            "https://github.com\\@evil.com/",
            "https://github.com/a b",
            "https://github.com/a\r\nHost: x",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
        ] {
            assert!(!is_https_url_on(bad, &hosts), "{bad}");
        }
    }

    #[test]
    fn local_urls() {
        for ok in [
            "http://loja.offline",
            "https://loja.offline/admin?x=1",
            "http://loja.offline:8080/",
            "http://mail.offline",
            "http://localhost:5173",
            "http://127.0.0.1:3000/x",
        ] {
            assert!(is_local_url(ok, "offline"), "{ok}");
        }
        for bad in [
            "https://evil.com",
            "http://evil.com@loja.offline/",
            "http://loja.offline@evil.com/",
            "http://evil.com\\.offline",
            "http://evil.com%2f.offline",
            "http://evil.com#.offline",
            "http://evil.com?.offline",
            "http://evil.com:80.offline",
            "http://loja.offline:abc",
            "http://.offline",
            "http://a..offline",
            "https://localhost",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)//.offline",
            "http://loja.offline/\r\nx",
            "ms-settings:privacy",
        ] {
            assert!(!is_local_url(bad, "offline"), "{bad}");
        }
    }

    #[test]
    fn sha() {
        assert!(is_sha256(&"aB".repeat(32)));
        assert!(!is_sha256(&"a".repeat(63)));
        assert!(!is_sha256(&"g".repeat(64)));
        assert!(!is_sha256(&format!("{} ", "a".repeat(63))));
    }

    #[test]
    fn env() {
        assert!(is_env_key("DB_HOST") && is_env_key("_X1"));
        for bad in ["", "1A", "A-B", "A B", "A=B", "A\nB", "Á"] {
            assert!(!is_env_key(bad), "{bad:?}");
        }
        assert!(is_env_value("p@ss w0rd $x \"q\" \\"));
        for bad in ["a\nEVIL=1", "a\rb", "a\0", "\u{1b}[31m"] {
            assert!(!is_env_value(bad), "{bad:?}");
        }
    }

    #[test]
    fn queue_names() {
        for ok in ["default", "whatsapp-inbound", "suppliers_fast", "emails.high", "tenant:1"] {
            assert!(is_queue_name(ok), "{ok}");
        }
        for bad in ["", "a,b", "a b", "--tries=0", "-x", "a\"b", "a;b", "a&b", "a|b", "<script>", "a\r\nb", "%PATH%", "é"] {
            assert!(!is_queue_name(bad), "{bad:?}");
        }
        assert!(!is_queue_name(&"a".repeat(101)));
    }

    #[test]
    fn wsl_distros() {
        for ok in ["Ubuntu", "Ubuntu-24.04", "Debian", "my_distro"] {
            assert!(is_wsl_distro(ok), "{ok}");
        }
        for bad in ["", "-u root", "--exec", "a b", "a&calc", "a\"b", "..\\x", ".x", "a;b", "a\0"] {
            assert!(!is_wsl_distro(bad), "{bad:?}");
        }
    }
}
