//! Winsock readiness for borrowed datagram buffers. A system thread-pool
//! wait wakes the task, which posts the usual notification to its IOCP worker.
use std::cell::Cell;
use std::ffi::c_void;
use std::io;
use std::net::UdpSocket;
use std::os::windows::io::AsRawSocket;
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use windows_sys::Win32::Networking::WinSock::{
    FD_READ, FD_WRITE, SOCKET_ERROR, WSA_INVALID_EVENT, WSACloseEvent, WSACreateEvent, WSAEVENT,
    WSAEnumNetworkEvents, WSAEventSelect, WSAGetLastError, WSANETWORKEVENTS,
};
use windows_sys::Win32::System::Threading::{
    CloseThreadpoolWait, CreateThreadpoolWait, PTP_CALLBACK_INSTANCE, PTP_WAIT, SetThreadpoolWait,
    WaitForThreadpoolWaitCallbacks,
};

use crate::net::udp::{Direction, Readiness};

#[derive(Debug)]
pub(super) struct Registration {
    socket: Rc<UdpSocket>,
    event: WSAEVENT,
    wait: PTP_WAIT,
    signal: Arc<Signal>,
    armed: Cell<bool>,
}

#[derive(Debug, Default)]
struct Signal {
    fired: AtomicBool,
    read: AtomicWaker,
    write: AtomicWaker,
}

impl Registration {
    pub(super) fn new(socket: Rc<UdpSocket>) -> io::Result<Box<dyn Readiness>> {
        let event = unsafe { WSACreateEvent() };
        if event == WSA_INVALID_EVENT {
            return Err(winsock_error());
        }

        let signal = Arc::<Signal>::default();
        // SAFETY: signal has a stable address until Drop cancels the wait and
        // joins its callbacks. Only its atomic fields are accessed off-worker.
        let wait = unsafe {
            CreateThreadpoolWait(
                Some(on_ready),
                Arc::as_ptr(&signal).cast_mut().cast(),
                ptr::null(),
            )
        };
        if wait == 0 {
            let error = io::Error::last_os_error();
            unsafe { WSACloseEvent(event) };
            return Err(error);
        }

        let registration = Self {
            socket,
            event,
            wait,
            signal,
            armed: Cell::new(false),
        };
        if unsafe {
            WSAEventSelect(
                registration.socket.as_raw_socket() as _,
                event,
                (FD_READ | FD_WRITE).cast_signed(),
            )
        } == SOCKET_ERROR
        {
            return Err(winsock_error());
        }
        Ok(Box::new(registration))
    }
}

impl Readiness for Registration {
    fn poll_wait(&self, cx: &mut Context<'_>, direction: Direction) -> Poll<io::Result<()>> {
        match direction {
            Direction::Read => self.signal.read.register(cx.waker()),
            Direction::Write => self.signal.write.register(cx.waker()),
        }

        if self.signal.fired.swap(false, Ordering::AcqRel) {
            self.armed.set(false);
            let mut events = WSANETWORKEVENTS::default();
            if unsafe {
                WSAEnumNetworkEvents(
                    self.socket.as_raw_socket() as _,
                    self.event,
                    &raw mut events,
                )
            } == SOCKET_ERROR
            {
                return Poll::Ready(Err(winsock_error()));
            }
            // Retry the syscall after resetting the event: a datagram could
            // have arrived between the previous syscall and the reset.
            return Poll::Ready(Ok(()));
        }

        if !self.armed.replace(true) {
            // The event stays signaled until WSAEnumNetworkEvents resets it.
            unsafe { SetThreadpoolWait(self.wait, self.event as _, ptr::null()) };
        }
        Poll::Pending
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            // Disarm and join before freeing the callback's context or event.
            SetThreadpoolWait(self.wait, ptr::null_mut(), ptr::null());
            WaitForThreadpoolWaitCallbacks(self.wait, 1);
            CloseThreadpoolWait(self.wait);
            WSAEventSelect(self.socket.as_raw_socket() as _, 0, 0);
            WSACloseEvent(self.event);
        }
    }
}

unsafe extern "system" fn on_ready(
    _: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _: PTP_WAIT,
    _: u32,
) {
    // SAFETY: Registration waits for callbacks before releasing Signal.
    let signal = unsafe { &*context.cast::<Signal>() };
    signal.fired.store(true, Ordering::Release);
    // A caller-supplied waker must not unwind across the system callback.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| signal.read.wake()));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| signal.write.wake()));
}

fn winsock_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}
