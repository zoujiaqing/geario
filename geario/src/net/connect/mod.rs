//! Tcp connector service
use crate::error::Error;

mod error;
mod message;
mod resolve;
mod service;
mod uri;

pub use self::error::ConnectError;
pub use self::message::{Address, Connect};
pub use self::service::Connector;

use crate::io::Io;
use crate::service::cfg::SharedCfg;

/// Resolve and connect to remote host
pub async fn connect<A, U>(message: U) -> Result<Io, Error<ConnectError>>
where
    A: Address,
    Connect<A>: From<U>,
{
    Connector::<A>::new()
        .connect(message, &SharedCfg::default())
        .await
}

/// Resolve and connect to remote host
pub async fn connect_with<A, U>(message: U, cfg: &SharedCfg) -> Result<Io, Error<ConnectError>>
where
    A: Address,
    Connect<A>: From<U>,
{
    Connector::<A>::new().connect(message, cfg).await
}
