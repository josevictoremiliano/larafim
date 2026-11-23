//! HTTPS local: uma CA do Larafim, restrita ao domínio local (ex.: só
//! `*.offline`, por restrição de nome), e um certificado por domínio gerado
//! na hora, pelo nome pedido no TLS (SNI).

use larafim_core::t;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, NameConstraints,
};
use tokio_rustls::rustls;
pub use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::server::{ClientHello, ResolvesServerCert};
use tokio_rustls::rustls::sign::CertifiedKey;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("{}", larafim_core::i18n::tf("certificado: {0}", &[("0", &.0 as &dyn std::fmt::Display)]))]
    Cert(String),
}

fn err(e: impl std::fmt::Display) -> TlsError {
    TlsError::Cert(e.to_string())
}

/// CA em PEM (certificado e chave).
#[derive(Clone)]
pub struct CaPem {
    pub cert: String,
    pub key: String,
}

fn days_from_now(days: u64) -> time::OffsetDateTime {
    time::OffsetDateTime::now_utc() + std::time::Duration::from_secs(days * 86_400)
}

/// Gera a CA. Ela só pode assinar nomes dentro de `tld` (ex.: "offline").
pub fn generate_ca(tld: &str, owner: &str) -> Result<CaPem, TlsError> {
    let key = KeyPair::generate().map_err(err)?;
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("Larafim CA local ({owner}) · .{tld}"));
    dn.push(DnType::OrganizationName, "Larafim (desenvolvimento local)");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![GeneralSubtree::DnsName(tld.to_string())],
        excluded_subtrees: Vec::new(),
    });
    p.not_before = days_from_now(0) - std::time::Duration::from_secs(86_400);
    p.not_after = days_from_now(3650);
    let cert = p.self_signed(&key).map_err(err)?;
    Ok(CaPem { cert: cert.pem(), key: key.serialize_pem() })
}

/// Emite certificados por domínio, assinados pela CA, e guarda em memória.
pub struct SniResolver {
    issuer: Issuer<'static, KeyPair>,
    ca_der: CertificateDer<'static>,
    tld: String,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl std::fmt::Debug for SniResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniResolver").field("tld", &self.tld).finish()
    }
}

impl SniResolver {
    pub fn new(ca: &CaPem, tld: &str) -> Result<Self, TlsError> {
        let key = KeyPair::from_pem(&ca.key).map_err(err)?;
        let issuer = Issuer::from_ca_cert_pem(&ca.cert, key).map_err(err)?;
        let ca_der = rustls_pemfile_der(&ca.cert)?;
        Ok(Self { issuer, ca_der, tld: tld.to_string(), cache: Mutex::new(HashMap::new()) })
    }

    fn covers(&self, host: &str) -> bool {
        host.strip_suffix(&self.tld).is_some_and(|rest| rest.ends_with('.') && rest.len() > 1)
            && host.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    }

    fn issue(&self, host: &str) -> Result<Arc<CertifiedKey>, TlsError> {
        let key = KeyPair::generate().map_err(err)?;
        let mut p = CertificateParams::new(vec![host.to_string()]).map_err(err)?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, host);
        p.distinguished_name = dn;
        p.is_ca = IsCa::NoCa;
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        p.not_before = days_from_now(0) - std::time::Duration::from_secs(86_400);
        // Navegadores recusam certificados de servidor com mais de 398 dias.
        p.not_after = days_from_now(397);
        let cert = p.signed_by(&key, &self.issuer).map_err(err)?;
        let der_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let signer = rustls::crypto::aws_lc_rs::sign::any_supported_type(&der_key).map_err(err)?;
        let chain = vec![cert.der().clone(), self.ca_der.clone()];
        Ok(Arc::new(CertifiedKey::new(chain, signer)))
    }

    /// Certificado para o domínio (gera na primeira vez).
    pub fn get(&self, host: &str) -> Option<Arc<CertifiedKey>> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if !self.covers(&host) {
            return None;
        }
        if let Some(c) = self.cache.lock().unwrap().get(&host) {
            return Some(c.clone());
        }
        match self.issue(&host) {
            Ok(c) => {
                self.cache.lock().unwrap().insert(host, c.clone());
                Some(c)
            }
            Err(e) => {
                tracing::warn!(host, "certificado não gerado: {e}");
                None
            }
        }
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.get(hello.server_name()?)
    }
}

fn rustls_pemfile_der(pem: &str) -> Result<CertificateDer<'static>, TlsError> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let bytes = base64_decode(&body).ok_or_else(|| TlsError::Cert(t!("PEM inválido")))?;
    Ok(CertificateDer::from(bytes))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// DER do certificado da CA (para instalar no repositório do Windows).
pub fn ca_der(ca_cert_pem: &str) -> Result<Vec<u8>, TlsError> {
    rustls_pemfile_der(ca_cert_pem).map(|d| d.to_vec())
}

/// Configuração do servidor TLS (HTTP/1.1).
pub fn server_config(resolver: Arc<SniResolver>) -> Arc<rustls::ServerConfig> {
    let mut cfg = rustls::ServerConfig::builder().with_no_client_auth().with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_issues_only_inside_tld() {
        let ca = generate_ca("offline", "teste").unwrap();
        let r = SniResolver::new(&ca, "offline").unwrap();
        assert!(r.get("loja.offline").is_some());
        assert!(r.get("api.loja.offline").is_some());
        assert!(r.get("google.com").is_none());
        assert!(r.get("offline").is_none());
        assert!(r.get("evil-offline").is_none());
        // Mesmo domínio: mesmo certificado (cache).
        assert!(Arc::ptr_eq(&r.get("loja.offline").unwrap(), &r.get("LOJA.offline.").unwrap()));
        assert_eq!(r.get("loja.offline").unwrap().cert.len(), 2, "folha + CA");
    }

    /// Gateway em HTTPS de verdade: cliente que só confia na CA do Larafim.
    #[tokio::test(flavor = "multi_thread")]
    async fn https_gateway_serves_site() {
        use crate::gateway::{Gateway, Routes, Site};
        use crate::pool::{PoolSettings, Pools};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.html"), "ola https").unwrap();
        let mut routes = Routes::default();
        routes.sites.insert(
            "loja.offline".into(),
            Site { name: "loja".into(), docroot: tmp.path().to_path_buf(), php: None, php_missing: false, online: true, proxy: None, ws: None },
        );
        let ca = generate_ca("offline", "teste").unwrap();
        let resolver = Arc::new(SniResolver::new(&ca, "offline").unwrap());
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let gw = Gateway::start_tls(port, Arc::new(std::sync::RwLock::new(routes)), Pools::new(PoolSettings::default()), server_config(resolver))
            .await
            .unwrap();

        let mut roots = rustls::RootCertStore::empty();
        roots.add(rustls_pemfile_der(&ca.cert).unwrap()).unwrap();
        let client = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let name = rustls::pki_types::ServerName::try_from("loja.offline").unwrap();
        let mut tls = connector.connect(name, tcp).await.expect("certificado aceito pela CA");
        tls.write_all(b"GET / HTTP/1.1\r\nHost: loja.offline\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut out = String::new();
        tls.read_to_string(&mut out).await.ok();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.ends_with("ola https"), "{out}");

        // Fora do domínio local: nenhum certificado (handshake falha).
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let name = rustls::pki_types::ServerName::try_from("google.com").unwrap();
        assert!(connector.connect(name, tcp).await.is_err());
        gw.stop();
    }
}
