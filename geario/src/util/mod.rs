//! Assorted utilities: timers, channels, futures and services.
#![deny(clippy::pedantic)]
#![allow(
    async_fn_in_trait,
    clippy::missing_fields_in_debug,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::unused_async_trait_impl
)]
use std::{error::Error, io, rc::Rc};

pub mod channel;
pub mod future;
pub mod services;
pub mod task;
pub mod time;

pub use futures_core::Stream;
pub use crate::rt::spawn;

#[doc(hidden)]
pub use hashbrown::{Equivalent, hash_map, hash_set};

pub type HashMap<K, V> = hash_map::HashMap<K, V, foldhash::fast::RandomState>;
pub type HashSet<V> = hash_set::HashSet<V, foldhash::fast::RandomState>;
pub type HashRandomState = foldhash::fast::RandomState;

pub fn dyn_err<E: Error + 'static>(e: E) -> Box<dyn Error> {
    let e: Box<dyn Error> = Box::new(e);
    e
}

pub fn dyn_rc_err<T: Error + 'static>(err: T) -> Rc<dyn Error> {
    Rc::new(err)
}

pub fn str_rc_err(s: String) -> Rc<dyn Error> {
    #[derive(thiserror::Error, Debug)]
    #[error("{_0}")]
    struct StringError(String);

    Rc::new(StringError(s))
}

pub fn clone_io_error(err: &io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{err:?}"))
}

#[doc(hidden)]
pub fn enable_test_logging() {
    #[cfg(not(feature = "no-test-logging"))]
    if std::env::var("GEARIO_NO_TEST_LOG").is_err() {
        if std::env::var("RUST_LOG").is_err() {
            unsafe {
                std::env::set_var("RUST_LOG", "trace");
            }
        }
        let _ = env_logger::builder().is_test(true).try_init();
    }
}
