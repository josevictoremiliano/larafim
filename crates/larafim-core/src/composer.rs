//! Progresso de comandos do Composer/Artisan pela saída: o Composer não
//! informa porcentagem, então cada etapa conhecida vale uma fatia.

/// Porcentagem estimada para a linha, se for uma etapa conhecida.
pub fn progress(line: &str) -> Option<u8> {
    let l = line.trim().to_ascii_lowercase();
    const STEPS: &[(&str, u8)] = &[
        ("./composer.json has been", 10),
        ("running composer update", 12),
        ("loading composer repositories", 15),
        ("updating dependencies", 25),
        ("lock file operations", 35),
        ("writing lock file", 45),
        ("installing dependencies from lock file", 50),
        ("package operations", 55),
        ("- downloading", 65),
        ("- installing", 75),
        ("generating optimized autoload", 85),
        ("generating autoload", 85),
        ("package:discover", 90),
        ("discovering packages", 90),
        ("reverb", 95),
        ("broadcasting", 95),
    ];
    STEPS.iter().find(|(k, _)| l.contains(k)).map(|(_, p)| *p)
}

/// O autoload do Composer do projeto conhece o namespace (`Laravel\Reverb`)?
/// A pasta em `vendor/` pode existir com o autoload velho, se a instalação
/// parou no meio (aí a classe "não existe" para o Laravel).
pub fn autoloads(project: &std::path::Path, namespace: &str) -> bool {
    let file = project.join("vendor").join("composer").join("autoload_psr4.php");
    let needle = format!("'{}\\\\'", namespace.replace('\\', "\\\\"));
    std::fs::read_to_string(file).is_ok_and(|t| t.contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoload_lookup() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!autoloads(tmp.path(), "Predis"));
        std::fs::create_dir_all(tmp.path().join("vendor/composer")).unwrap();
        std::fs::write(
            tmp.path().join("vendor/composer/autoload_psr4.php"),
            "<?php return array(\n    'Laravel\\\\Reverb\\\\' => array($vendorDir . '/laravel/reverb/src'),\n    'Predis\\\\' => array(),\n);",
        )
        .unwrap();
        assert!(autoloads(tmp.path(), "Laravel\\Reverb"));
        assert!(autoloads(tmp.path(), "Predis"));
        assert!(!autoloads(tmp.path(), "Pusher"));
        assert!(!autoloads(tmp.path(), "Laravel"), "só o namespace inteiro");
    }

    #[test]
    fn known_steps() {
        assert_eq!(progress("./composer.json has been updated"), Some(10));
        assert_eq!(progress("Lock file operations: 1 install, 0 updates, 0 removals"), Some(35));
        assert_eq!(progress("  - Downloading predis/predis (v3.2.0)"), Some(65));
        assert_eq!(progress("  - Installing predis/predis (v3.2.0): Extracting archive"), Some(75));
        assert_eq!(progress("Generating optimized autoload files"), Some(85));
        assert_eq!(progress("qualquer coisa"), None);
        assert_eq!(progress(""), None);
    }
}
