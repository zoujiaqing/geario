//! Plain TCP echo server.
//!
//! Run with `cargo run --example echo`, then `nc 127.0.0.1 8080`.
use std::io;

use geario::codec::BytesCodec;
use geario::io::Io;
use geario::service::cfg::SharedCfg;
use geario::service::fn_service;

#[geario::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    let cfg = SharedCfg::new("ECHO");

    geario::server::net::build()
        .bind("echo", "127.0.0.1:8080", cfg, async |_| {
            fn_service(async |io: Io| {
                let codec = BytesCodec;
                loop {
                    match io.recv(&codec).await {
                        Ok(Some(item)) => {
                            if io.send(item, &codec).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
                Ok::<_, io::Error>(())
            })
        })?
        .run()
        .await
}
