//! Gateway HTTP do Larafim: escuta em 127.0.0.1, olha o domínio pedido
//! (`loja.offline`) e entrega o projeto certo — arquivos estáticos direto do
//! disco e PHP pelo pool da versão do projeto.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, Limited, StreamBody, combinators::BoxBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use larafim_core::t;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::io::ReaderStream;

use crate::pages;
use crate::pool::{PhpRuntime, Pools};
use crate::resolve::{Target, resolve};
use crate::cgi;

pub(crate) type Body = BoxBody<Bytes, std::io::Error>;

/// Maior corpo de requisição aceito (uploads).
const MAX_BODY: usize = 256 * 1024 * 1024;

/// Um projeto como o gateway enxerga.
#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    pub docroot: PathBuf,
    /// `None` = projeto sem PHP (site estático) ou PHP não instalado.
    pub php: Option<PhpRuntime>,
    /// O projeto usa PHP mas nenhuma versão compatível está instalada.
    pub php_missing: bool,
    pub online: bool,
    /// Projeto Node: o gateway repassa tudo para o servidor de dev nessa porta.
    pub proxy: Option<crate::devserver::PortCell>,
    /// Porta do Laravel Reverb do projeto: WebSocket em `/app/...` vai para lá.
    /// Assim o navegador fala com o Reverb pelo mesmo domínio (e pelo HTTPS do
    /// gateway): numa página https, `ws://127.0.0.1` seria bloqueado.
    pub ws: Option<u16>,
}

/// Tabela domínio → projeto. Trocada a quente, sem reiniciar o gateway.
#[derive(Debug, Default)]
pub struct Routes {
    pub sites: HashMap<String, Site>,
}

pub type SharedRoutes = Arc<RwLock<Routes>>;

struct Ctx {
    routes: SharedRoutes,
    pools: Arc<Pools>,
    port: u16,
    https: bool,
}

pub struct Gateway {
    port: u16,
    stop: watch::Sender<bool>,
}

impl Gateway {
    /// Começa a escutar. Erro de porta ocupada volta como `AddrInUse`.
    pub async fn start(port: u16, routes: SharedRoutes, pools: Arc<Pools>) -> std::io::Result<Self> {
        Self::listen(port, routes, pools, None).await
    }

    /// Mesmo gateway, em HTTPS (certificado por domínio via SNI).
    pub async fn start_tls(
        port: u16,
        routes: SharedRoutes,
        pools: Arc<Pools>,
        tls: Arc<tokio_rustls::rustls::ServerConfig>,
    ) -> std::io::Result<Self> {
        Self::listen(port, routes, pools, Some(tokio_rustls::TlsAcceptor::from(tls))).await
    }

    async fn listen(
        port: u16,
        routes: SharedRoutes,
        pools: Arc<Pools>,
        tls: Option<tokio_rustls::TlsAcceptor>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let (stop, mut stopped) = watch::channel(false);
        let ctx = Arc::new(Ctx { routes, pools, port, https: tls.is_some() });
        tracing::info!(port, https = tls.is_some(), "gateway escutando");

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, remote)) = accepted else { continue };
                        let ctx = ctx.clone();
                        let tls = tls.clone();
                        tokio::spawn(async move {
                            let service = service_fn(move |req| handle(req, ctx.clone(), remote));
                            let mut builder = http1::Builder::new();
                            match tls {
                                Some(acceptor) => {
                                    let Ok(stream) = acceptor.accept(stream).await else { return };
                                    let _ = builder
                                        .keep_alive(true)
                                        .serve_connection(TokioIo::new(stream), service)
                                        .with_upgrades()
                                        .await;
                                }
                                None => {
                                    let _ = builder
                                        .keep_alive(true)
                                        .serve_connection(TokioIo::new(stream), service)
                                        .with_upgrades() // WebSocket (HMR do Vite/Next)
                                        .await;
                                }
                            }
                        });
                    }
                    _ = stopped.changed() => break,
                }
            }
            tracing::info!("gateway parado");
        });
        Ok(Self { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(self) {
        let _ = self.stop.send(true);
    }
}

fn full(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Response<Body> {
    let mut resp = Response::new(Full::new(body.into()).map_err(|never| match never {}).boxed());
    *resp.status_mut() = status;
    if let Ok(v) = HeaderValue::from_str(content_type) {
        resp.headers_mut().insert(hyper::header::CONTENT_TYPE, v);
    }
    resp
}

pub(crate) fn html(status: StatusCode, body: String) -> Response<Body> {
    full(status, "text/html; charset=utf-8", body)
}

async fn handle(req: Request<Incoming>, ctx: Arc<Ctx>, remote: SocketAddr) -> Result<Response<Body>, Infallible> {
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .or_else(|| req.uri().host())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_lowercase();

    let site = ctx.routes.read().unwrap().sites.get(&host).cloned();
    let secure = ctx.https;
    let Some(site) = site else {
        let online: Vec<String> = {
            let routes = ctx.routes.read().unwrap();
            let mut v: Vec<String> = routes.sites.iter().filter(|(_, s)| s.online).map(|(h, _)| h.clone()).collect();
            v.sort();
            v
        };
        return Ok(html(StatusCode::NOT_FOUND, pages::unknown_site(&host, &online)));
    };
    if !site.online {
        return Ok(html(StatusCode::SERVICE_UNAVAILABLE, pages::offline(&site.name)));
    }

    if let Some(port) = site.ws
        && is_reverb_socket(&req)
    {
        return Ok(crate::proxy::forward(req, port, &host, secure).await);
    }

    if let Some(cell) = &site.proxy {
        let port = *cell.lock().unwrap();
        return Ok(match port {
            Some(port) => crate::proxy::forward(req, port, &host, secure).await,
            None => html(StatusCode::SERVICE_UNAVAILABLE, pages::node_starting(&site.name)),
        });
    }

    let path = req.uri().path().to_string();
    let uses_php = site.php.is_some() || site.php_missing;
    let response = match resolve(&site.docroot, &path, uses_php) {
        Target::Static(file) => serve_file(&req, file).await,
        Target::Php { script, script_name } => match &site.php {
            Some(runtime) => run_php(req, &ctx, &site, &host, runtime, script, script_name, remote, secure).await,
            None => html(StatusCode::SERVICE_UNAVAILABLE, pages::no_php(&site.name)),
        },
        Target::Forbidden => html(StatusCode::FORBIDDEN, pages::forbidden()),
        Target::NotFound => html(StatusCode::NOT_FOUND, pages::not_found(&path)),
    };
    tracing::debug!(host, %path, status = response.status().as_u16(), "requisição");
    Ok(response)
}

/// Conexão WebSocket do Pusher/Echo para o Reverb: `GET /app/<chave>` com
/// `Upgrade: websocket`. Requisições comuns em `/app/...` seguem para o PHP.
fn is_reverb_socket<B>(req: &Request<B>) -> bool {
    req.method() == Method::GET
        && req.uri().path().starts_with("/app/")
        && req
            .headers()
            .get(hyper::header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

async fn serve_file(req: &Request<Incoming>, file: PathBuf) -> Response<Body> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return full(StatusCode::METHOD_NOT_ALLOWED, "text/plain", t!("Método não permitido"));
    }
    let Ok(handle) = tokio::fs::File::open(&file).await else {
        return html(StatusCode::NOT_FOUND, pages::not_found(req.uri().path()));
    };
    let len = handle.metadata().await.map(|m| m.len()).unwrap_or(0);
    let mime = mime_guess::from_path(&file).first_or_octet_stream();
    let content_type = if mime.type_() == "text" || mime.subtype() == "javascript" || mime.subtype() == "json" {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    };

    let body: Body = if req.method() == Method::HEAD {
        Full::new(Bytes::new()).map_err(|never| match never {}).boxed()
    } else {
        StreamBody::new(ReaderStream::new(handle).map_ok(Frame::data)).boxed()
    };
    let mut resp = Response::new(body);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&content_type) {
        h.insert(hyper::header::CONTENT_TYPE, v);
    }
    h.insert(hyper::header::CONTENT_LENGTH, HeaderValue::from(len));
    // Desenvolvimento: sempre a versão mais nova do arquivo.
    h.insert(hyper::header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    resp
}

#[allow(clippy::too_many_arguments)]
async fn run_php(
    req: Request<Incoming>,
    ctx: &Ctx,
    site: &Site,
    host: &str,
    runtime: &PhpRuntime,
    script: PathBuf,
    script_name: String,
    remote: SocketAddr,
    secure: bool,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let body = match Limited::new(body, MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return full(StatusCode::PAYLOAD_TOO_LARGE, "text/plain", t!("Requisição grande demais")),
    };

    let uri = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/").to_string();
    let query = parts.uri.query().unwrap_or_default().to_string();
    let mut params: Vec<(String, String)> = vec![
        ("GATEWAY_INTERFACE".into(), "CGI/1.1".into()),
        ("SERVER_SOFTWARE".into(), "Larafim".into()),
        ("SERVER_PROTOCOL".into(), format!("{:?}", parts.version)),
        ("REQUEST_METHOD".into(), parts.method.to_string()),
        ("REQUEST_URI".into(), uri),
        ("QUERY_STRING".into(), query),
        ("SCRIPT_FILENAME".into(), script.display().to_string()),
        ("SCRIPT_NAME".into(), script_name.clone()),
        ("PHP_SELF".into(), script_name.clone()),
        ("DOCUMENT_URI".into(), script_name),
        ("DOCUMENT_ROOT".into(), site.docroot.display().to_string()),
        ("SERVER_NAME".into(), host.to_string()),
        ("SERVER_ADDR".into(), "127.0.0.1".into()),
        ("SERVER_PORT".into(), ctx.port.to_string()),
        ("REMOTE_ADDR".into(), remote.ip().to_string()),
        ("REMOTE_PORT".into(), remote.port().to_string()),
        ("REDIRECT_STATUS".into(), "200".into()),
        ("CONTENT_LENGTH".into(), body.len().to_string()),
        ("REQUEST_SCHEME".into(), if secure { "https" } else { "http" }.into()),
    ];
    if secure {
        params.push(("HTTPS".into(), "on".into()));
    }
    if let Some(ct) = parts.headers.get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        params.push(("CONTENT_TYPE".into(), ct.to_string()));
    }
    for (name, value) in &parts.headers {
        let lower = name.as_str();
        // "Proxy" fica de fora (falha conhecida "httpoxy").
        if lower == "content-type" || lower == "content-length" || lower == "proxy" {
            continue;
        }
        if let Ok(v) = value.to_str() {
            params.push((format!("HTTP_{}", lower.to_uppercase().replace('-', "_")), v.to_string()));
        }
    }

    match ctx.pools.execute(runtime, &params, &body).await {
        Ok(out) => {
            if !out.stderr.is_empty() {
                let text = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(site = site.name, "PHP: {}", text.chars().take(2000).collect::<String>());
            }
            let parsed = cgi::parse(&out.stdout);
            let mut resp = Response::new(Full::new(Bytes::from(parsed.body)).map_err(|never| match never {}).boxed());
            *resp.status_mut() = StatusCode::from_u16(parsed.status).unwrap_or(StatusCode::OK);
            for (name, value) in parsed.headers {
                if let (Ok(n), Ok(v)) = (HeaderName::try_from(name.as_str()), HeaderValue::from_str(&value)) {
                    resp.headers_mut().append(n, v);
                }
            }
            resp
        }
        Err(e) => {
            tracing::error!(site = site.name, "erro do PHP: {e}");
            html(StatusCode::BAD_GATEWAY, pages::php_error(&e.to_string()))
        }
    }
}
