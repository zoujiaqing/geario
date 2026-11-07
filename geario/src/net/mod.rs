//! Utility for async runtime abstraction
#![deny(clippy::pedantic)]
#![allow(
    clippy::clone_on_copy,
    clippy::cast_possible_truncation,
    clippy::missing_fields_in_debug,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unused_async_trait_impl
)]
use std::{any::Any, io, net, net::SocketAddr, panic};

use crate::io::Io;
use crate::rt::{BlockFuture, Driver, Runner};
use crate::service::cfg::SharedCfg;

pub mod channel;
pub mod connect;

#[cfg(unix)]
pub mod polling;

#[cfg(unix)]
pub use polling::UdpSocket;

#[cfg(target_os = "linux")]
pub mod uring;

#[cfg(windows)]
pub mod iocp;

#[cfg(any(unix, windows))]
mod helpers;

#[allow(clippy::wrong_self_convention)]
pub trait Reactor: Driver {
    /// Adopt UDP on backends with datagram support.
    #[cfg(unix)]
    fn from_udp_socket(&self, _: net::UdpSocket) -> io::Result<UdpSocket> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "UDP requires the polling reactor",
        ))
    }

    fn tcp_connect(&self, addr: net::SocketAddr, cfg: SharedCfg) -> channel::Receiver<Io>;

    fn unix_connect(&self, addr: std::path::PathBuf, cfg: SharedCfg) -> channel::Receiver<Io>;

    /// Convert std `TcpStream` to `Io`
    fn from_tcp_stream(&self, stream: net::TcpStream, cfg: SharedCfg) -> io::Result<Io>;

    #[cfg(unix)]
    /// Convert std `UnixStream` to `Io`
    fn from_unix_stream(&self, _: std::os::unix::net::UnixStream, _: SharedCfg) -> io::Result<Io>;
}

#[inline]
/// Opens a TCP connection to a remote host.
pub async fn tcp_connect(addr: SocketAddr, cfg: SharedCfg) -> io::Result<Io> {
    with_current(|driver| driver.tcp_connect(addr, cfg)).await
}

#[inline]
/// Opens a unix stream connection.
pub async fn unix_connect<'a, P>(addr: P, cfg: SharedCfg) -> io::Result<Io>
where
    P: AsRef<std::path::Path> + 'a,
{
    with_current(|driver| driver.unix_connect(addr.as_ref().into(), cfg)).await
}

#[inline]
/// Convert std `TcpStream` to `TcpStream`
pub fn from_tcp_stream(stream: net::TcpStream, cfg: SharedCfg) -> io::Result<Io> {
    with_current(|driver| driver.from_tcp_stream(stream, cfg))
}

#[cfg(unix)]
#[inline]
/// Convert std `UnixStream` to `UnixStream`
pub fn from_unix_stream(stream: std::os::unix::net::UnixStream, cfg: SharedCfg) -> io::Result<Io> {
    with_current(|driver| driver.from_unix_stream(stream, cfg))
}

fn with_current<T, F: FnOnce(&dyn Reactor) -> T>(f: F) -> T {
    #[cold]
    fn not_in_geario_driver() -> ! {
        panic!("not in a geario driver")
    }

    if CURRENT_DRIVER.is_set() {
        CURRENT_DRIVER.with(|d| f(&**d))
    } else {
        not_in_geario_driver()
    }
}

#[allow(clippy::borrowed_box)]
/// Sets the current reactor and runs the provided closure.
pub fn with_reactor<R, F: FnOnce() -> R>(r: &Box<dyn Reactor>, f: F) -> R {
    #[cold]
    fn reactor_is_set() -> ! {
        panic!("reactor is already set");
    }

    if CURRENT_DRIVER.is_set() {
        reactor_is_set()
    }
    CURRENT_DRIVER.set(r, f)
}

scoped_tls::scoped_thread_local!(static CURRENT_DRIVER: Box<dyn Reactor>);

/// The default runtime.
///
/// Automatically selects the runtime implementation based on the
/// configured features and the platform on which it runs.
#[derive(Copy, Clone, Debug)]
pub struct DefaultRuntime;

impl Runner for DefaultRuntime {
    fn block_on(&self, fut: BlockFuture) -> Result<(), Box<dyn Any + Send>> {
        run_with_driver(default_reactor(), fut)
    }
}

fn run_with_driver(driver: Box<dyn Reactor>, fut: BlockFuture) -> Result<(), Box<dyn Any + Send>> {
    with_reactor(&driver, || {
        panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let rt = crate::rt::Runtime::new(driver.handle());
            rt.block_on(fut, &*driver);
        }))
    })
}

fn default_reactor() -> Box<dyn Reactor> {
    #[cfg(windows)]
    {
        Box::new(crate::net::iocp::Reactor::new().expect("Cannot construct driver"))
    }

    #[cfg(unix)]
    {
        #[cfg(feature = "neon-polling")]
        {
            Box::new(crate::net::polling::Reactor::new().expect("Cannot construct polling reactor"))
        }

        #[cfg(all(
            target_os = "linux",
            feature = "neon-uring",
            not(feature = "neon-polling")
        ))]
        {
            // Asked for by name, so a failure is reported rather than
            // quietly answered with a different driver. The kernel's
            // reason is carried out: "cannot construct" on its own sends
            // whoever hits it looking in the wrong place.
            Box::new(
                crate::net::uring::Reactor::new(2048)
                    .unwrap_or_else(|e| panic!("Cannot construct io-uring reactor: {e}")),
            )
        }

        // Reached when nothing was asked for, and also when io_uring was
        // asked for on a platform that has none: the request cannot be
        // honoured, and the alternative to picking a driver here is a
        // build that does not compile at all.
        #[cfg(all(
            not(feature = "neon-polling"),
            any(not(feature = "neon-uring"), not(target_os = "linux"))
        ))]
        {
            // Nothing was asked for: prefer io_uring, fall back quietly.
            #[cfg(target_os = "linux")]
            let driver: Box<dyn Reactor> = match crate::net::uring::Reactor::new(2048) {
                Ok(reactor) => Box::new(reactor),
                Err(e) => {
                    log::debug!("No io-uring reactor ({e}), using polling");
                    Box::new(
                        crate::net::polling::Reactor::new()
                            .expect("Cannot construct polling reactor"),
                    )
                }
            };

            #[cfg(not(target_os = "linux"))]
            let driver: Box<dyn Reactor> = Box::new(
                crate::net::polling::Reactor::new().expect("Cannot construct polling reactor"),
            );

            driver
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn completed_runtime_releases_current_reactor() {
        for _ in 0..2 {
            let driver: Box<dyn Reactor> = Box::new(polling::Reactor::new().unwrap());
            assert!(!CURRENT_DRIVER.is_set());
            let result = run_with_driver(
                driver,
                Box::pin(async {
                    assert!(CURRENT_DRIVER.is_set());
                    with_current(|reactor| reactor.handle().notify().unwrap());
                }),
            );
            assert!(result.is_ok());
            assert!(!CURRENT_DRIVER.is_set());
        }
    }

    #[test]
    fn reactor_scope_is_restored_after_unwind() {
        let driver: Box<dyn Reactor> = Box::new(polling::Reactor::new().unwrap());
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            with_reactor(&driver, || panic!("test reactor unwind"));
        }));
        assert!(result.is_err());
        assert!(!CURRENT_DRIVER.is_set());
        with_reactor(&driver, || assert!(CURRENT_DRIVER.is_set()));
        assert!(!CURRENT_DRIVER.is_set());
    }

    #[test]
    fn nested_reactor_is_rejected_without_replacing_outer_scope() {
        let driver: Box<dyn Reactor> = Box::new(polling::Reactor::new().unwrap());
        with_reactor(&driver, || {
            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                with_reactor(&driver, || {});
            }));
            assert!(result.is_err());
            assert!(CURRENT_DRIVER.is_set());
        });
        assert!(!CURRENT_DRIVER.is_set());
    }
}
