//! A real TLS handshake over a real socket, with a throwaway CA.
//!
//! Certificates are issued at run time, so nothing in the repository expires
//! and no fixture has to be regenerated.
#![cfg(feature = "rustls")]

use std::sync::Arc;

use geario::codec::BytesCodec;
use geario::service::cfg::SharedCfg;
use geario::tls::rustls::{TlsClientFilter, TlsServerFilter};

use tls_rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tls_rustls::{ClientConfig, RootCertStore, ServerConfig};

struct Pki {
    ca_der: CertificateDer<'static>,
    cert_der: CertificateDer<'static>,
    key_der: PrivateKeyDer<'static>,
}

fn issue(host: &str) -> Pki {
    let mut ca_params = rcgen::CertificateParams::new(Vec::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();

    let leaf_params = rcgen::CertificateParams::new(vec![host.to_owned()]).unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();

    Pki {
        ca_der: ca.der().clone(),
        cert_der: leaf.der().clone(),
        key_der: PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap(),
    }
}

#[geario::test]
async fn handshake_then_echo() {
    let _ = tls_rustls::crypto::aws_lc_rs::default_provider().install_default();
    let pki = issue("localhost");

    let server_cfg = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![pki.cert_der.clone()], pki.key_der.clone_key())
            .expect("server config"),
    );

    let lst = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = lst.local_addr().unwrap();

    let acceptor_cfg = server_cfg.clone();
    geario::rt::spawn(async move {
        let accepted = geario::rt::spawn_blocking(move || lst.accept()).await;
        let Ok(Ok((stream, _))) = accepted else { return };
        stream.set_nonblocking(true).ok();
        let Ok(io) = geario::net::from_tcp_stream(stream, SharedCfg::new("TLS-SRV").into())
        else {
            return;
        };
        let Ok(io) = TlsServerFilter::create(
            io,
            acceptor_cfg,
            geario::util::time::Millis(5_000),
        )
        .await
        else {
            return;
        };
        let codec = BytesCodec;
        while let Ok(Some(item)) = io.recv(&codec).await {
            if io.send(item, &codec).await.is_err() {
                break;
            }
        }
    });

    let mut roots = RootCertStore::empty();
    roots.add(pki.ca_der.clone()).unwrap();
    let client_cfg = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    let io = geario::net::tcp_connect(addr, SharedCfg::new("TLS-CLI").into())
        .await
        .expect("connect");
    let host = ServerName::try_from("localhost").unwrap();
    let io = TlsClientFilter::create(io, client_cfg, host)
        .await
        .expect("tls handshake");

    let codec = BytesCodec;
    io.send(geario::bytes::Bytes::from_static(b"over tls"), &codec)
        .await
        .expect("send");
    let echoed = io.recv(&codec).await.expect("recv").expect("no data");
    assert_eq!(&echoed[..], b"over tls");
}

/// The handshake must actually verify. A client that trusts an unrelated CA
/// has to be rejected, or the test above would be proving nothing.
#[geario::test]
async fn handshake_rejects_an_untrusted_chain() {
    let _ = tls_rustls::crypto::aws_lc_rs::default_provider().install_default();
    let pki = issue("localhost");
    let unrelated = issue("localhost");

    let server_cfg = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![pki.cert_der.clone()], pki.key_der.clone_key())
            .expect("server config"),
    );

    let lst = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = lst.local_addr().unwrap();

    geario::rt::spawn(async move {
        let accepted = geario::rt::spawn_blocking(move || lst.accept()).await;
        let Ok(Ok((stream, _))) = accepted else { return };
        stream.set_nonblocking(true).ok();
        let Ok(io) = geario::net::from_tcp_stream(stream, SharedCfg::new("TLS-SRV").into())
        else {
            return;
        };
        let _ = TlsServerFilter::create(io, server_cfg, geario::util::time::Millis(5_000)).await;
    });

    // Trust a CA that did not sign the server's certificate.
    let mut roots = RootCertStore::empty();
    roots.add(unrelated.ca_der.clone()).unwrap();
    let client_cfg = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    let io = geario::net::tcp_connect(addr, SharedCfg::new("TLS-CLI").into())
        .await
        .expect("connect");
    let host = ServerName::try_from("localhost").unwrap();
    let result = TlsClientFilter::create(io, client_cfg, host).await;

    assert!(
        result.is_err(),
        "the handshake accepted a certificate signed by an unrelated CA"
    );
}
