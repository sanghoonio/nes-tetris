//! The deployed server is `wss://`: native links need rustls to have a cryptography provider, or
//! the connection's thread panics and the app waits on CONNECTING forever. No network needed.

#[test]
fn tls_has_a_cryptography_provider() {
    let _ = rustls::ClientConfig::builder().with_root_certificates(rustls::RootCertStore::empty()).with_no_client_auth();
}
