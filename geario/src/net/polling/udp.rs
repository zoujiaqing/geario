use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::net::UdpSocket as StdSocket;
use std::os::fd::AsRawFd;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use super::{Event, Handler, Reactor, ReactorApi};
use crate::net::udp::{Direction, Readiness};
use crate::rt::Arbiter;

#[derive(Debug)]
pub(super) struct Registration {
    state: Rc<State>,
    ops: Rc<Ops>,
    id: u32,
}

#[derive(Debug)]
struct State {
    socket: Rc<StdSocket>,
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

impl Registration {
    pub(super) fn new(driver: &Reactor, socket: Rc<StdSocket>) -> io::Result<Box<dyn Readiness>> {
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
        Ok(Box::new(Self { state, ops, id }))
    }
}

impl Readiness for Registration {
    fn poll_wait(&self, cx: &mut Context<'_>, direction: Direction) -> Poll<io::Result<()>> {
        let mut interests = self.state.interests.borrow_mut();
        if let Some(error) = &interests.error {
            return Poll::Ready(Err(error.raw_os_error().map_or_else(
                || io::Error::new(error.kind(), error.to_string()),
                io::Error::from_raw_os_error,
            )));
        }
        let slot = match direction {
            Direction::Read => &mut interests.read,
            Direction::Write => &mut interests.write,
        };
        let needs_arm = slot.is_none();
        *slot = Some(cx.waker().clone());
        if needs_arm {
            self.ops.arm(self.id, &self.state, &interests);
        }
        Poll::Pending
    }
}

impl Drop for Registration {
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
