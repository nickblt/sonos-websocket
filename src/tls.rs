use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::error::Result;

/// Embedded Sonos Device Authentication Root CA certificate
const SONOS_CA_PEM: &[u8] = include_bytes!("certs/sonos-ca.pem");

/// Creates a TLS connector configured with the Sonos CA certificate.
pub fn create_tls_connector() -> Result<TlsConnector> {
    let mut reader = std::io::BufReader::new(SONOS_CA_PEM);

    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<std::result::Result<Vec<_>, _>>()?;

    let mut root_store = RootCertStore::empty();
    for cert in certs {
        root_store
            .add(cert)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(TlsConnector::from(Arc::new(config)))
}

/// Connects to a Sonos player with TLS.
///
/// The hostname should be the .local mDNS hostname (e.g., "Sonos-XXXX.local").
pub async fn connect_tls(hostname: &str, port: u16) -> Result<TlsStream<TcpStream>> {
    let connector = create_tls_connector()?;

    let addr = format!("{}:{}", hostname, port);
    let stream = TcpStream::connect(&addr).await?;

    // Strip trailing dot if present (DNS standard format)
    let hostname_clean = hostname.trim_end_matches('.');
    let server_name = ServerName::try_from(hostname_clean.to_string())?;

    let tls_stream = connector.connect(server_name, stream).await?;

    Ok(tls_stream)
}
