//! Async IO stack for the Neton ecosystem.
#![doc(html_root_url = "https://docs.rs/geario/")]
#![allow(unreachable_pub)]

// Lets `geario::` paths resolve inside this crate, which is what the
// attribute macros in geario-macros expand to. The lint cannot see that
// use, so it has to be silenced here.
#[allow(unused_extern_crates)]
extern crate self as geario;

pub use geario_macros::{rt_main as main, rt_test as test};

pub mod bytes;
pub mod codec;
pub mod dispatcher;
pub mod error;
pub mod io;
pub mod net;
pub mod rt;
pub mod server;
pub mod service;
pub mod util;
