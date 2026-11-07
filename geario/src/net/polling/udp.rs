use std::cell::RefCell;
use std::collections::HashMap;
use std::future::poll_fn;
use std::io;
use std::net::{SocketAddr, UdpSocket as StdSocket};
use std::os::fd::AsRawFd;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use super::{Event, Handler, Reactor, ReactorApi};
use crate::rt::Arbiter;

/// A worker-local, nonblocking UDP socket registered with the polling reactor.
///
/// A single task may wait for reads and another for writes. A pending receive
/// can be dropped without consuming a datagram. Keep receive buffers large
/// enough for the entire datagram: as with std UDP, excess bytes can be lost.
#[derive(Debug)]
pub struct UdpSocket {
    state: Rc<State>,
    ops: Rc<Ops>,
    id: u32,
}

#[derive(Debug)]
struct State {
    socket: StdSocket,
    interests: RefCell<Interests>,
}

#[derive(Debug, Default)]
struct Interests {
    read: Option<Waker>,
    write: Option<Waker>,
    error: Option<io::Error>,
}

#[derive(Debug)]
struct Ops {
    api: ReactorApi,
    sockets: RefCell<HashMap<u32, Weak<State>>>,
    // Do not reuse a key while a stale notification could still refer to it.
    next: std::cell::Cell<u32>,
}

struct Events(Rc<Ops>);

impl UdpSocket {
    /// Binds on the current polling worker.
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        crate::net::with_current(|driver| driver.from_udp_socket(StdSocket::bind(addr)?))
    }

    /// Adopts an existing socket on the current polling worker.
    pub fn from_std(socket: StdSocket) -> io::Result<Self> {
        crate::net::with_current(|driver| driver.from_udp_socket(socket))
    }

    pub(super) fn register(driver: &Reactor, socket: StdSocket) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        let ops = Arbiter::get_value(|| {
            let mut ops = None;
            driver.register(|api| {
                let state = Rc::new(Ops {
                    api,
                    sockets: RefCell::new(HashMap::new()),
                    next: std::cell::Cell::new(0),
                });
                ops = Some(state.clone());
                Box::new(Events(state))
            });
            ops.unwrap()
        });
        let id = ops.next.get();
        ops.next.set(
            id.checked_add(1)
                .ok_or_else(|| io::Error::other("UDP registration IDs exhausted"))?,
        );
        let state = Rc::new(State {
            socket,
            interests: RefCell::new(Interests::default()),
        });
        ops.api.attach_udp(state.socket.as_raw_fd(), id)?;
        ops.sockets.borrow_mut().insert(id, Rc::downgrade(&state));
        Ok(Self { state, ops, id })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.state.socket.local_addr()
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
        self.poll_io(cx, true, |socket| socket.recv_from(buf))
    }

    pub fn poll_send_to(
        &self,
        cx: &mut Context<'_>,
        buf: &[u8],
        addr: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        self.poll_io(cx, false, |socket| socket.send_to(buf, addr))
    }

    fn poll_io<T>(
        &self,
        cx: &mut Context<'_>,
        read: bool,
        mut f: impl FnMut(&StdSocket) -> io::Result<T>,
    ) -> Poll<io::Result<T>> {
        let mut interests = self.state.interests.borrow_mut();
        if let Some(error) = &interests.error {
            return Poll::Ready(Err(error.raw_os_error().map_or_else(
                || io::Error::new(error.kind(), error.to_string()),
                io::Error::from_raw_os_error,
            )));
        }
        let slot = if read {
            &mut interests.read
        } else {
            &mut interests.write
        };
        loop {
            match f(&self.state.socket) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let needs_arm = slot.is_none();
                    *slot = Some(cx.waker().clone());
                    if needs_arm {
                        self.ops.arm(self.id, &self.state, &interests);
                    }
                    return Poll::Pending;
                }
                result => {
                    slot.take();
                    return Poll::Ready(result);
                }
            }
        }
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        self.ops.sockets.borrow_mut().remove(&self.id);
        self.ops.api.detach(self.state.socket.as_raw_fd(), self.id);
    }
}

impl Ops {
    fn arm(&self, id: u32, state: &State, interests: &Interests) {
        self.api.modify(
            state.socket.as_raw_fd(),
            id,
            Event::new(0, interests.read.is_some(), interests.write.is_some()),
        );
    }
}

impl Handler for Events {
    fn event(&mut self, id: usize, event: Event) {
        let id = id as u32;
        let Some(state) = self.0.sockets.borrow().get(&id).and_then(Weak::upgrade) else {
            return;
        };
        let mut interests = state.interests.borrow_mut();
        let read = if event.readable {
            interests.read.take()
        } else {
            None
        };
        let write = if event.writable {
            interests.write.take()
        } else {
            None
        };
        if interests.read.is_some() || interests.write.is_some() {
            self.0.arm(id, &state, &interests);
        }
        drop(interests);
        read.into_iter().chain(write).for_each(Waker::wake);
    }

    fn error(&mut self, id: usize, err: io::Error) {
        let Some(state) = self
            .0
            .sockets
            .borrow()
            .get(&(id as u32))
            .and_then(Weak::upgrade)
        else {
            return;
        };
        let mut interests = state.interests.borrow_mut();
        interests.error = Some(err);
        let read = interests.read.take();
        let write = interests.write.take();
        drop(interests);
        read.into_iter().chain(write).for_each(Waker::wake);
    }

    fn tick(&mut self) {}

    fn cleanup(&mut self) {}
}
