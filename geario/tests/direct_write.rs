//! `IoRef::try_write_vectored` hands bytes to the socket without going through
//! the write buffer. It is a fast path, so what matters is that it declines in
//! every case where taking it would be wrong.

use std::io::IoSlice;
use std::sync::Arc;

use geario::codec::BytesCodec;
use geario::service::cfg::SharedCfg;

/// Accept one connection and hand back everything it sends.
fn echo_server() -> std::net::SocketAddr {
    let lst = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = lst.local_addr().unwrap();
    geario::rt::spawn(async move {
        let accepted = geario::rt::spawn_blocking(move || lst.accept()).await;
        let Ok(Ok((stream, _))) = accepted else { return };
        stream.set_nonblocking(true).ok();
        let Ok(io) = geario::net::from_tcp_stream(stream, SharedCfg::new("ECHO").into()) else {
            return;
        };
        let codec = BytesCodec;
        while let Ok(Some(item)) = io.recv(&codec).await {
            if io.send(item, &codec).await.is_err() {
                break;
            }
        }
    });
    addr
}

#[geario::test]
async fn writes_straight_to_the_socket_without_buffering() {
    let addr = echo_server();
    let io = geario::net::tcp_connect(addr, SharedCfg::new("DIRECT").into())
        .await
        .expect("connect");

    let n = io
        .get_ref()
        .try_write_vectored(&[IoSlice::new(b"one "), IoSlice::new(b"two")])
        .expect("direct write");
    assert_eq!(n, 7, "the socket was empty, it should have taken it all");

    // The point of the path: nothing was copied into the write buffer, so
    // there is nothing left for a flush to do.
    assert_eq!(io.get_ref().with_write_buf(|b| b.len()).unwrap(), 0);

    let echoed = io.recv(&BytesCodec).await.expect("recv").expect("no data");
    assert_eq!(&echoed[..], b"one two");
}

/// Bytes already queued must go out first. Writing straight to the socket
/// while the buffer is occupied would reorder the stream.
#[geario::test]
async fn declines_when_bytes_are_already_queued() {
    let addr = echo_server();
    let io = geario::net::tcp_connect(addr, SharedCfg::new("QUEUED").into())
        .await
        .expect("connect");

    io.get_ref()
        .with_write_buf(|b| b.extend_from_slice(b"first"))
        .unwrap();
    let n = io
        .get_ref()
        .try_write_vectored(&[IoSlice::new(b"second")])
        .expect("try write");
    assert_eq!(n, 0, "it jumped the queue");

    io.get_ref()
        .with_write_buf(|b| b.extend_from_slice(b"second"))
        .unwrap();
    io.flush(true).await.expect("flush");

    let mut got = Vec::new();
    while got.len() < 11 {
        let item = io.recv(&BytesCodec).await.expect("recv").expect("no data");
        got.extend_from_slice(&item);
    }
    assert_eq!(&got[..], b"firstsecond");
}

/// A filter that transforms the write buffer has to see the bytes. Handing
/// them to the socket behind a TLS session would send plaintext.
#[cfg(feature = "rustls")]
#[geario::test]
async fn declines_behind_a_filter_that_transforms() {
    use geario::tls::rustls::{TlsClientFilter, TlsServerFilter};
    use tls_rustls::pki_types::{PrivateKeyDer, ServerName};
    use tls_rustls::{ClientConfig, RootCertStore, ServerConfig};

    let _ = tls_rustls::crypto::aws_lc_rs::default_provider().install_default();

    let mut ca_params = rcgen::CertificateParams::new(Vec::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let leaf_params = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();

    let server_cfg = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap(),
            )
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
        let Ok(io) =
            TlsServerFilter::create(io, server_cfg, geario::util::time::Millis(5_000)).await
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
    roots.add(ca.der().clone()).unwrap();
    let io = geario::net::tcp_connect(addr, SharedCfg::new("TLS-CLI").into())
        .await
        .expect("connect");
    let io = TlsClientFilter::create(
        io,
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ),
        ServerName::try_from("localhost").unwrap(),
    )
    .await
    .expect("tls handshake");

    // Drain first. The handshake leaves records queued, and an occupied
    // buffer would decline for that reason instead, leaving the filter check
    // untested.
    io.send(geario::bytes::Bytes::from_static(b"over tls"), &BytesCodec)
        .await
        .expect("send");
    let echoed = io.recv(&BytesCodec).await.expect("recv").expect("no data");
    assert_eq!(&echoed[..], b"over tls");
    io.flush(true).await.expect("flush");
    assert_eq!(
        io.get_ref().with_write_buf(|b| b.len()).unwrap(),
        0,
        "nothing must be queued, or this would decline for the wrong reason"
    );

    let n = io
        .get_ref()
        .try_write_vectored(&[IoSlice::new(b"plaintext")])
        .expect("try write");
    assert_eq!(n, 0, "plaintext was handed straight to the socket");
}
