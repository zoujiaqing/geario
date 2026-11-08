use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use ntex_io_uring::{opcode::PollAdd, types::Fd};

use super::{Handler, Reactor, ReactorApi};
use crate::net::udp::{Direction, Readiness};
use crate::rt::Arbiter;

#[derive(Debug)]
pub(super) struct Registration {
    socket: Rc<UdpSocket>,
    ops: Rc<Operations>,
    read: Rc<Wait>,
    write: Rc<Wait>,
}

#[derive(Debug, Default)]
struct Wait {
    id: Cell<Option<u64>>,
    waker: RefCell<Option<Waker>>,
    error: RefCell<Option<io::Error>>,
}

struct Operation {
    // The raw fd must stay open through submission, even if the user drops
    // the socket before the reactor submits its queue.
    _socket: Rc<UdpSocket>,
    wait: Weak<Wait>,
}

struct Operations {
    api: ReactorApi,
    pending: RefCell<HashMap<u64, Operation>>,
    next: Cell<u64>,
}

struct Events(Rc<Operations>);

impl std::fmt::Debug for Operations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpOperations")
            .field("pending", &self.pending.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Registration {
    pub(super) fn new(reactor: &Reactor, socket: Rc<UdpSocket>) -> Box<dyn Readiness> {
        let ops = Arbiter::get_value(|| {
            let mut registered = None;
            reactor.register(|api| {
                let ops = Rc::new(Operations {
                    api,
                    pending: RefCell::new(HashMap::new()),
                    next: Cell::new(0),
                });
                registered = Some(ops.clone());
                Box::new(Events(ops))
            });
            registered.unwrap()
        });
        Box::new(Self {
            socket,
            ops,
            read: Rc::default(),
            write: Rc::default(),
        })
    }
}

impl Readiness for Registration {
    fn poll_wait(&self, cx: &mut Context<'_>, direction: Direction) -> Poll<io::Result<()>> {
        let (wait, mask) = match direction {
            Direction::Read => (&self.read, libc::POLLIN),
            Direction::Write => (&self.write, libc::POLLOUT),
        };
        if let Some(error) = wait.error.take() {
            return Poll::Ready(Err(error));
        }
        *wait.waker.borrow_mut() = Some(cx.waker().clone());
        if wait.id.get().is_none() {
            // A cancel can still be queued after a readiness CQE has removed
            // its operation. Do not give that key to a newer request.
            let id = self.ops.next.get();
            if id > ReactorApi::MAX_KEY {
                return Poll::Ready(Err(io::Error::other(
                    "io_uring UDP operation IDs exhausted",
                )));
            }
            self.ops.next.set(id + 1);
            self.ops.pending.borrow_mut().insert(
                id,
                Operation {
                    _socket: self.socket.clone(),
                    wait: Rc::downgrade(wait),
                },
            );
            wait.id.set(Some(id));
            self.ops.api.submit_key(
                id,
                PollAdd::new(Fd(self.socket.as_raw_fd()), u32::from(mask.cast_unsigned())).build(),
            );
        }
        Poll::Pending
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        for wait in [&self.read, &self.write] {
            if let Some(id) = wait.id.get() {
                self.ops.api.cancel_key(id);
            }
        }
    }
}

impl Handler for Events {
    fn completed(&mut self, id: usize, _: u32, result: io::Result<usize>) {
        let op = self.0.pending.borrow_mut().remove(&(id as u64)).unwrap();
        if let Some(wait) = op.wait.upgrade() {
            wait.id.set(None);
            wait.error.replace(result.err());
            let waker = wait.waker.borrow_mut().take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    fn canceled(&mut self, id: usize) {
        self.completed(id, 0, Err(io::Error::from_raw_os_error(libc::ECANCELED)));
    }

    fn tick(&mut self) {}

    fn cleanup(&mut self) {}
}
