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

#[geario::test]
async fn dropping_a_socket_with_a_pending_receive_releases_its_port() {
    use geario::util::time::sleep;

    let socket = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = socket.local_addr().unwrap();
    // Drop before the worker next enters the kernel, so a backend can have
    // both its registration and its cancellation queued at the same time.
    poll_fn(|cx| {
        assert!(socket.poll_recv_from(cx, &mut [0; 1]).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(socket);

    timeout(Millis(2000), async {
        loop {
            match UdpSocket::bind(addr) {
                Ok(replacement) => {
                    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
                    sender.send_to(b"x", addr).unwrap();
                    let mut buf = [0; 1];
                    let (n, peer) = replacement.recv_from(&mut buf).await.unwrap();
                    assert_eq!(n, 1);
                    assert_eq!(buf, *b"x");
                    assert_eq!(peer, sender.local_addr().unwrap());
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                    sleep(Millis(10)).await;
                }
                Err(err) => panic!("UDP bind after drop: {err}"),
            }
        }
    })
    .await
    .expect("dropped UDP socket kept its port or replacement did not receive");
}
