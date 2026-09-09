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

/// Server that sends a large blob back in a single `send()` on any request.
/// Writing the whole blob at once queues many write pages together, so the
/// write task gathers them into one vectored send; a small socket send buffer
/// then makes that send complete in pieces, exercising the short-write
/// requeue. `resp_len` bytes go out per request.
fn blob_server(resp_len: usize) -> std::net::SocketAddr {
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
            // A small send buffer makes the kernel accept a big response only
            // in pieces, so the writev returns short and the requeue path runs.
            let sref = socket2::SockRef::from(&stream);
            let _ = sref.set_send_buffer_size(16 * 1024);
            stream.set_nonblocking(true).ok();
            let Ok(io) = geario::net::from_tcp_stream(stream, SharedCfg::new("BLOB").into()) else {
                continue;
            };
            let blob = pattern(resp_len);
            geario::rt::spawn(async move {
                let codec = BytesCodec;
                while let Ok(Some(_req)) = io.recv(&codec).await {
                    // extend_from_slice fragments the blob across write pages,
                    // so the write task has several pages queued at once and
                    // gathers them into one vectored send -- unlike the codec's
                    // append, which wraps the whole buffer as a single page.
                    if io
                        .get_ref()
                        .with_write_buf(|b| b.extend_from_slice(&blob))
                        .is_err()
                    {
                        break;
                    }
                    if io.flush(true).await.is_err() {
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
    let resp_len = 128 * 1024;
    let addr = blob_server(resp_len);
    let sent = pattern(resp_len);

    // A blocking client on its own thread, with a small receive buffer and a
    // deliberately slow drain, so the server's writev cannot complete in one
    // shot and has to requeue the tail.
    let result = geario::rt::spawn_blocking(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        s.set_write_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        let sock = socket2::SockRef::from(&s);
        let _ = sock.set_recv_buffer_size(8 * 1024);

        // Trigger one blob.
        s.write_all(b"go").unwrap();
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

    // On the io_uring build, prove the paths under test actually ran: a
    // multi-page Writev was submitted and at least one of them short-wrote and
    // requeued. Without this the integrity pass alone could not distinguish
    // the vectored path from any other.
    #[cfg(all(target_os = "linux", feature = "neon-uring"))]
    {
        let (submits, short, _cancels) = geario::net::uring::write_stats::write_path();
        eprintln!("WRITE_PATH submits={submits} short={short} cancels={_cancels}");
        assert!(submits > 0, "no multi-page Writev was submitted");
        assert!(
            short > 0,
            "no short write was forced; the requeue path was not exercised"
        );
    }
}

/// A client that disconnects in the middle of receiving a large response must
/// not crash or wedge the server: a later connection still works.
#[geario::test]
async fn a_disconnect_midresponse_leaves_the_server_healthy() {
    let addr = blob_server(256 * 1024);

    // First client: trigger a big response, read a little, then drop.
    geario::rt::spawn_blocking(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        s.set_write_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        let sock = socket2::SockRef::from(&s);
        let _ = sock.set_recv_buffer_size(8 * 1024);
        s.write_all(b"go").unwrap();
        let mut buf = [0u8; 1024];
        let _ = s.read(&mut buf);
        // drop s -> disconnect mid-response
    })
    .await
    .unwrap();

    // Second client: the server must still accept and serve. It reads the
    // first chunk of the blob and checks it matches the known pattern, which
    // proves the server survived the mid-response disconnect and is serving
    // correct data, not that it echoes.
    let ok = geario::rt::spawn_blocking(move || {
        let want = pattern(256 * 1024);
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_nodelay(true).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        s.set_write_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        s.write_all(b"go").unwrap();
        let mut got = vec![0u8; 4096];
        s.read_exact(&mut got).unwrap();
        got[..] == want[..got.len()]
    })
    .await
    .unwrap();
    assert!(
        ok,
        "server did not serve a new connection after a mid-response disconnect"
    );
}
