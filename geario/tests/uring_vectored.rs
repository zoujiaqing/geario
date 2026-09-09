//! Correctness of the multi-page write path (the io_uring driver's vectored
//! `Writev`, the polling driver's `writev`).
//!
//! These run under whichever driver the build selects: on Linux with
//! `neon-uring` they exercise the `Writev` submission, partial-write requeue
//! and cancel/disconnect paths; on kqueue they exercise the polling
//! equivalent. Either way the invariant is the same: a large, multi-page
//! response arrives byte-exact, with nothing lost, duplicated or reordered,
//! even when the socket only accepts it in pieces.

use std::io::{Read, Write};

use geario::codec::BytesCodec;
use geario::service::cfg::SharedCfg;

/// Echo server: whatever a connection sends, it sends back. A 128 KB echo is
/// eight 16 KB write pages, so the response is vectored, and a small socket
/// buffer plus a slow reader forces the write to complete in several pieces.
fn echo_server() -> std::net::SocketAddr {
    let lst = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = lst.local_addr().unwrap();
    geario::rt::spawn(async move {
        loop {
            let accepted = geario::rt::spawn_blocking({
                let lst = lst.try_clone().unwrap();
                move || lst.accept()
            })
            .await;
            let Ok(Ok((stream, _))) = accepted else {
                return;
            };
            stream.set_nonblocking(true).ok();
            let Ok(io) = geario::net::from_tcp_stream(stream, SharedCfg::new("ECHO").into()) else {
                continue;
            };
            geario::rt::spawn(async move {
                let codec = BytesCodec;
                while let Ok(Some(item)) = io.recv(&codec).await {
                    if io.send(item, &codec).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

/// A payload whose every byte is a function of its position, so any loss,
/// duplication or reordering shows up as a mismatch at a known offset.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect()
}

#[geario::test]
async fn a_multipage_response_survives_a_piecewise_socket() {
    let addr = echo_server();
    let sent = pattern(128 * 1024);

    // A blocking client on its own thread, with a small receive buffer and a
    // deliberately slow drain, so the server's writev cannot complete in one
    // shot and has to requeue the tail.
    let result = geario::rt::spawn_blocking(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        let sock = socket2::SockRef::from(&s);
        let _ = sock.set_recv_buffer_size(8 * 1024);

        s.write_all(&sent).unwrap();
        let mut got = vec![0u8; sent.len()];
        let mut off = 0;
        while off < got.len() {
            // Small reads with a breath between them keep the socket buffer
            // full so the server keeps short-writing.
            let end = (off + 4096).min(got.len());
            match s.read(&mut got[off..end]) {
                Ok(0) => break,
                Ok(n) => {
                    off += n;
                    std::thread::sleep(std::time::Duration::from_micros(50));
                }
                Err(e) => panic!("read: {e}"),
            }
        }
        (off, got, sent)
    })
    .await;

    let (off, got, sent) = result.unwrap();
    assert_eq!(off, sent.len(), "short read: got {off} of {}", sent.len());
    // Find the first mismatch rather than dumping 128 KB.
    if let Some(i) = (0..sent.len()).find(|&i| got[i] != sent[i]) {
        panic!("byte {i} differs: got {} want {}", got[i], sent[i]);
    }
}

/// A client that disconnects in the middle of receiving a large response must
/// not crash or wedge the server: a later connection still works.
#[geario::test]
async fn a_disconnect_midresponse_leaves_the_server_healthy() {
    let addr = echo_server();
    let big = pattern(256 * 1024);

    // First client: send a large request, read a little, then drop.
    let b = big.clone();
    geario::rt::spawn_blocking(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        s.write_all(&b).unwrap();
        let mut buf = [0u8; 1024];
        let _ = s.read(&mut buf);
        // drop s -> disconnect mid-response
    })
    .await
    .unwrap();

    // Second client: a normal small round-trip must still succeed.
    let ok = geario::rt::spawn_blocking(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        let msg = b"still alive";
        s.write_all(msg).unwrap();
        let mut got = vec![0u8; msg.len()];
        s.read_exact(&mut got).unwrap();
        got == msg
    })
    .await
    .unwrap();
    assert!(
        ok,
        "server did not serve a new connection after a mid-response disconnect"
    );
}
