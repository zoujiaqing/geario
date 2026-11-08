use std::future::poll_fn;
use std::io;
use std::net::{SocketAddr, UdpSocket as StdSocket};
use std::rc::Rc;
use std::task::{Context, Poll};

#[derive(Clone, Copy, Debug)]
pub(super) enum Direction {
    Read,
    Write,
}

pub(super) trait Readiness: std::fmt::Debug {
    fn poll_wait(&self, cx: &mut Context<'_>, direction: Direction) -> Poll<io::Result<()>>;
}

/// A worker-local, nonblocking UDP socket.
///
/// A single task may wait for reads and another for writes. A pending receive
/// can be dropped without consuming a datagram. Keep receive buffers large
/// enough for the entire datagram: as with std UDP, excess bytes can be lost.
#[derive(Debug)]
pub struct UdpSocket {
    readiness: Box<dyn Readiness>,
    socket: Rc<StdSocket>,
}

impl UdpSocket {
    /// Binds on the current geario worker.
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        Self::from_std(StdSocket::bind(addr)?)
    }

    /// Adopts a socket on the current geario worker.
    ///
    /// The socket must not be registered with another runtime or polled
    /// through another handle while this wrapper owns it.
    pub fn from_std(socket: StdSocket) -> io::Result<Self> {
        crate::net::with_current(|driver| driver.from_udp_socket(socket))
    }

    pub(super) fn register(
        socket: StdSocket,
        register: impl FnOnce(Rc<StdSocket>) -> io::Result<Box<dyn Readiness>>,
    ) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        let socket = Rc::new(socket);
        let readiness = register(socket.clone())?;
        Ok(Self { readiness, socket })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        poll_fn(|cx| self.poll_recv_from(cx, buf)).await
    }

    pub async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        poll_fn(|cx| self.poll_send_to(cx, buf, addr)).await
    }

    pub fn poll_recv_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, SocketAddr)>> {
        self.poll_io(cx, Direction::Read, |socket| socket.recv_from(buf))
    }

    pub fn poll_send_to(
        &self,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        self.poll_io(cx, Direction::Write, |socket| socket.send_to(buf, addr))
    }

    fn poll_io<T>(
        &self,
        cx: &mut Context<'_>,
        direction: Direction,
        mut f: impl FnMut(&StdSocket) -> io::Result<T>,
    ) -> Poll<io::Result<T>> {
        loop {
            match f(&self.socket) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::task::ready!(self.readiness.poll_wait(cx, direction))?;
                }
                result => return Poll::Ready(result),
            }
        }
    }
}
