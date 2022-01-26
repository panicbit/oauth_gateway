use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use parking_lot::RwLock;
use rustls::ServerConfig;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio_rustls::TlsAcceptor;
use unicase::Ascii;
use webpki::DnsNameRef;

pub struct TlsManager {
    acceptors: HashMap<SocketAddr, (TlsAcceptor, Arc<CertResolver>)>,
}

impl TlsManager {
    pub fn new() -> Self {
        Self {
            acceptors: <_>::default(),
        }
    }

    pub fn add_certified_key(
        &mut self,
        listen_addr: SocketAddr,
        server_name: String,
        certified_key: CertifiedKey,
    ) -> Result<()> {
        let (_tls_acceptor, cert_resolver) = self.acceptors.entry(listen_addr)
            .or_insert_with(|| {
                let cert_resolver = Arc::new(CertResolver::new());

                let server_config = ServerConfig::builder()
                    .with_safe_defaults()
                    .with_no_client_auth()
                    .with_cert_resolver(Arc::clone(&cert_resolver) as _);

                let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

                (tls_acceptor, cert_resolver)
            });

        cert_resolver.add_certified_key(server_name, certified_key)?;

        Ok(())
    }

    pub fn acceptor(&self, listen_addr: &SocketAddr) -> Option<TlsAcceptor> {
        let (tls_acceptor, _cert_resolver) = self.acceptors.get(listen_addr)?;

        Some(tls_acceptor.clone())
    }
}

struct CertResolver {
    certified_keys: RwLock<HashMap<Ascii<Cow<'static, str>>, Arc<CertifiedKey>>>,
}

impl CertResolver {
    pub fn new() -> Self {
        Self {
            certified_keys: <_>::default(),
        }
    }

    pub fn add_certified_key(&self,
        server_name: String,
        certified_key: CertifiedKey,
    ) -> Result<()> {
        DnsNameRef::try_from_ascii_str(&server_name)
            .map_err(|_| anyhow!("Bad DNS name: {:?}", server_name))?;

        let server_name = Ascii::new(Cow::Owned(server_name));
        let certified_key = Arc::new(certified_key);

        // TODO: implement cross check
        // certified_key.cross_check_end_entity_cert(Some(checked_name))?;

        self.certified_keys.write().insert(server_name, certified_key);

        Ok(())
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let server_name = client_hello.server_name()?; 
        let server_name = Ascii::new(Cow::Borrowed(server_name));

        let certified_key = self.certified_keys.read().get(&server_name).map(Arc::clone);

        if certified_key.is_none() {
            eprintln!("No certchain found for {:?}", server_name.as_ref());
            dbg!(self.certified_keys.read().keys().collect::<Vec<_>>());
        }

        certified_key
    }
}
