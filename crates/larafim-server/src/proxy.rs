//! Proxy reverso para servidores de desenvolvimento (Vite, Next, Nuxt…),
//! incluindo WebSocket — o recarregamento automático (HMR) depende dele.

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{HOST, HeaderValue, UPGRADE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::gateway::{Body, html};
use crate::pages;

/// Muitos servidores de dev escutam só em IPv6 (`::1`) ou só em IPv4.
async fn connect(port: u16) -> std::io::Result<TcpStream> {
    match TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => Ok(s),
        Err(_) => TcpStream::connect(("::1", port)).await,
    }
}

pub async fn forward(mut req: Request<Incoming>, port: u16, original_host: &str, https: bool) -> Response<Body> {
    let client_upgrade = req.headers().contains_key(UPGRADE).then(|| hyper::upgrade::on(&mut req));

    let stream = match connect(port).await {
        Ok(s) => s,
        Err(e) => return html(StatusCode::BAD_GATEWAY, pages::proxy_error(port, &e.to_string())),
    };
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(TokioIo::new(stream)).await {
        Ok(x) => x,
        Err(e) => return html(StatusCode::BAD_GATEWAY, pages::proxy_error(port, &e.to_string())),
    };
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });

    // Servidores como o Vite recusam domínios desconhecidos: para eles a
    // requisição vem de "localhost". O domínio original vai em X-Forwarded-Host.
    let headers = req.headers_mut();
    if let Ok(v) = HeaderValue::from_str(original_host) {
        headers.insert("x-forwarded-host", v);
    }
    headers.insert("x-forwarded-proto", HeaderValue::from_static(if https { "https" } else { "http" }));
    if let Ok(v) = HeaderValue::from_str(&format!("localhost:{port}")) {
        headers.insert(HOST, v);
    }

    let mut resp = match sender.send_request(req).await {
        Ok(r) => r,
        Err(e) => return html(StatusCode::BAD_GATEWAY, pages::proxy_error(port, &e.to_string())),
    };

    if resp.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_upgrade) = client_upgrade
    {
        let backend_upgrade = hyper::upgrade::on(&mut resp);
        tokio::spawn(async move {
            if let (Ok(client), Ok(backend)) = (client_upgrade.await, backend_upgrade.await) {
                let mut client = TokioIo::new(client);
                let mut backend = TokioIo::new(backend);
                let _ = tokio::io::copy_bidirectional(&mut client, &mut backend).await;
            }
        });
    }

    let (parts, body) = resp.into_parts();
    let body: Body = body.map_err(std::io::Error::other).boxed();
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::{Gateway, PoolSettings, Pools, Routes, Site};

    /// "Servidor de dev" falso: responde HTTP e, se pedirem upgrade,
    /// vira um eco de bytes (como um WebSocket simplificado).
    async fn fake_dev_server() -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = l.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let host = req.lines().find(|l| l.to_lowercase().starts_with("host:")).unwrap_or("").to_string();
                    let fwd = req.lines().find(|l| l.to_lowercase().starts_with("x-forwarded-host:")).unwrap_or("").to_string();
                    if req.to_lowercase().contains("upgrade: websocket") {
                        s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await.unwrap();
                        loop {
                            let n = s.read(&mut buf).await.unwrap_or(0);
                            if n == 0 { break; }
                            s.write_all(&buf[..n]).await.unwrap();
                        }
                    } else {
                        let body = format!("dev ok|{host}|{fwd}");
                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        s.write_all(resp.as_bytes()).await.unwrap();
                    }
                });
            }
        });
        port
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_and_websocket_through_gateway() {
        let dev_port = fake_dev_server().await;
        let routes = Arc::new(RwLock::new(Routes::default()));
        let cell = Arc::new(Mutex::new(None));
        routes.write().unwrap().sites.insert(
            "app.offline".into(),
            Site { name: "app".into(), docroot: ".".into(), php: None, php_missing: false, online: true, proxy: Some(cell.clone()), ws: None },
        );
        let gw_port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let gw = Gateway::start(gw_port, routes, Pools::new(PoolSettings::default())).await.unwrap();

        let get = || async {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", gw_port)).await.unwrap();
            s.write_all(b"GET / HTTP/1.1\r\nHost: app.offline\r\nConnection: close\r\n\r\n").await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };

        // Ainda sem porta: página "Iniciando…".
        assert!(get().await.starts_with("HTTP/1.1 503"));

        *cell.lock().unwrap() = Some(dev_port);
        let r = get().await;
        assert!(r.contains(&format!("dev ok|host: localhost:{dev_port}|x-forwarded-host: app.offline")), "{r}");

        // WebSocket: depois do 101, os bytes vão e voltam.
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", gw_port)).await.unwrap();
        s.write_all(b"GET /hmr HTTP/1.1\r\nHost: app.offline\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 101"));
        s.write_all(b"ping-hmr").await.unwrap();
        let n = s.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"ping-hmr");
        gw.stop();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reverb_socket_goes_to_its_port_only() {
        let reverb_port = fake_dev_server().await;
        let docroot = tempfile::tempdir().unwrap();
        let routes = Arc::new(RwLock::new(Routes::default()));
        routes.write().unwrap().sites.insert(
            "loja.offline".into(),
            Site {
                name: "loja".into(),
                docroot: docroot.path().to_path_buf(),
                php: None,
                php_missing: false,
                online: true,
                proxy: None,
                ws: Some(reverb_port),
            },
        );
        let gw_port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let gw = Gateway::start(gw_port, routes, Pools::new(PoolSettings::default())).await.unwrap();
        let send = |req: &'static [u8]| async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", gw_port)).await.unwrap();
            s.write_all(req).await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = s.read(&mut buf).await.unwrap();
            (s, String::from_utf8_lossy(&buf[..n]).to_string())
        };

        // WebSocket do Echo em /app/<chave>: vai para o Reverb.
        let (mut s, head) =
            send(b"GET /app/chave?protocol=7 HTTP/1.1\r\nHost: loja.offline\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await;
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        s.write_all(b"pusher").await.unwrap();
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"pusher");

        // Sem upgrade, ou upgrade fora de /app/: não vai para o Reverb.
        let (_, plain) = send(b"GET /app/chave HTTP/1.1\r\nHost: loja.offline\r\nConnection: close\r\n\r\n").await;
        assert!(!plain.contains("dev ok"), "{plain}");
        let (_, other) =
            send(b"GET /../app/x HTTP/1.1\r\nHost: loja.offline\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await;
        assert!(!other.starts_with("HTTP/1.1 101"), "{other}");
        gw.stop();
    }
}
