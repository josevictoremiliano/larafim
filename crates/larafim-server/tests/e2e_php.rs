//! Teste de ponta a ponta com o php-cgi de verdade.
//! Rode com: `LARAFIM_TEST_PHP=<pasta do PHP instalado> cargo test -p larafim-server -- --ignored`

use std::sync::{Arc, RwLock};
use std::time::Duration;

use larafim_server::{Gateway, PhpRuntime, PoolSettings, Pools, Routes, Site};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn http(port: u16, host: &str, request_line: &str, body: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "{request_line} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nX-Teste: larafim\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn laravel_like_site_with_real_php() {
    let php_dir = std::env::var("LARAFIM_TEST_PHP").expect("defina LARAFIM_TEST_PHP");
    let version = std::path::Path::new(&php_dir).file_name().unwrap().to_string_lossy().to_string();

    let site_dir = tempfile::tempdir().unwrap();
    let public = site_dir.path().join("public");
    std::fs::create_dir_all(public.join("css")).unwrap();
    std::fs::write(public.join("css").join("app.css"), "body{color:red}").unwrap();
    std::fs::write(site_dir.path().join(".env"), "SEGREDO=1").unwrap();
    std::fs::write(
        public.join("index.php"),
        r#"<?php
if ($_SERVER['REQUEST_URI'] === '/redirecionar') { header('Location: /destino'); exit; }
setcookie('a', '1'); setcookie('b', '2');
header('Content-Type: text/plain');
echo 'PHP ', PHP_VERSION, "\n";
echo 'URI ', $_SERVER['REQUEST_URI'], "\n";
echo 'HOST ', $_SERVER['HTTP_HOST'], "\n";
echo 'HEADER ', $_SERVER['HTTP_X_TESTE'] ?? '-', "\n";
echo 'POST ', $_POST['nome'] ?? '-', "\n";
echo 'GET ', $_GET['q'] ?? '-', "\n";
echo 'EXT ', extension_loaded('mbstring') ? 'mbstring' : 'sem', "\n";
"#,
    )
    .unwrap();

    let routes = Arc::new(RwLock::new(Routes::default()));
    routes.write().unwrap().sites.insert(
        "loja.offline".into(),
        Site {
            name: "loja".into(),
            docroot: public.clone(),
            php: Some(PhpRuntime { version: version.clone(), dir: php_dir.clone().into() }),
            php_missing: false,
            online: true,
            proxy: None,
            ws: None,
        },
    );
    routes.write().unwrap().sites.insert(
        "parado.offline".into(),
        Site { name: "parado".into(), docroot: public.clone(), php: None, php_missing: false, online: false, proxy: None, ws: None },
    );

    let pools = Pools::new(PoolSettings { workers: 2, ..Default::default() });
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let gw = Gateway::start(port, routes, pools.clone()).await.unwrap();

    // Rota do Laravel (front controller) com query string.
    let r = http(port, "loja.offline", "GET /produtos/42?q=caneca", "").await;
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    assert!(r.contains(&format!("PHP {version}")), "{r}");
    assert!(r.contains("URI /produtos/42?q=caneca"));
    assert!(r.contains("HOST loja.offline"));
    assert!(r.contains("HEADER larafim"));
    assert!(r.contains("GET caneca"));
    assert!(r.contains("EXT mbstring"));
    assert_eq!(r.matches("set-cookie:").count() + r.matches("Set-Cookie:").count(), 2, "{r}");

    // POST com formulário.
    let r = http(port, "loja.offline", "POST /salvar", "nome=Jos%C3%A9").await;
    assert!(r.contains("POST José"), "{r}");

    // Redirecionamento.
    let r = http(port, "loja.offline", "GET /redirecionar", "").await;
    assert!(r.starts_with("HTTP/1.1 302"), "{r}");
    assert!(r.to_lowercase().contains("location: /destino"));

    // Estático direto do disco.
    let r = http(port, "loja.offline", "GET /css/app.css", "").await;
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("body{color:red}") && r.contains("text/css"), "{r}");

    // Proteções e páginas do Larafim.
    assert!(http(port, "loja.offline", "GET /../.env", "").await.starts_with("HTTP/1.1 403"));
    assert!(http(port, "parado.offline", "GET /", "").await.starts_with("HTTP/1.1 503"));
    assert!(http(port, "nada.offline", "GET /", "").await.starts_with("HTTP/1.1 404"));

    // Várias requisições ao mesmo tempo (mais que os 2 processos do pool).
    let mut tasks = Vec::new();
    for i in 0..8 {
        tasks.push(tokio::spawn(async move { http(port, "loja.offline", &format!("GET /item/{i}"), "").await }));
    }
    for (i, t) in tasks.into_iter().enumerate() {
        let r = t.await.unwrap();
        assert!(r.contains(&format!("URI /item/{i}")), "{r}");
    }

    assert_eq!(pools.running().await, vec![version]);
    gw.stop();
    pools.stop_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}
