#![cfg(all(unix, any(not(target_os = "linux"), feature = "neon-polling")))]

use std::future::poll_fn;
use std::task::Poll;

use geario::net::UdpSocket;
use geario::util::time::{Millis, timeout};

#[geario::test]
async fn udp_preserves_datagrams_after_a_pending_receive_is_dropped() {
    let loopback = "127.0.0.1:0".parse().unwrap();
    let receiver = UdpSocket::bind(loopback).unwrap();
    let sender = UdpSocket::from_std(std::net::UdpSocket::bind(loopback).unwrap()).unwrap();
    let destination = receiver.local_addr().unwrap();
    let source = sender.local_addr().unwrap();

    let mut buf = [0; 64];
    // Drive a receive to Pending, then end that wait before data arrives.
    poll_fn(|cx| {
        assert!(receiver.poll_recv_from(cx, &mut buf).is_pending());
        Poll::Ready(())
    })
    .await;

    timeout(Millis(2000), async {
        let send = geario::rt::spawn(async move {
            for datagram in [b"first".as_slice(), b"", b"last"] {
                assert_eq!(
                    sender.send_to(datagram, destination).await.unwrap(),
                    datagram.len()
                );
            }
        });
        for expected in [b"first".as_slice(), b"", b"last"] {
            let (len, addr) = receiver.recv_from(&mut buf).await.unwrap();
            assert_eq!(addr, source);
            assert_eq!(&buf[..len], expected);
        }
        send.await.unwrap();
    })
    .await
    .expect("UDP receive did not wake");
}
