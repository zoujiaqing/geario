use std::io;

use geario::codec::BytesCodec;
use geario::io::Io;
use geario::service::cfg::SharedCfg;
use geario::service::fn_service;

#[geario::main]
async fn main() -> io::Result<()> {
    // Configurable because the default collides with whatever else the host
    // happens to be running.
    let addr = std::env::var("BENCH_ADDR").unwrap_or_else(|_| "127.0.0.1:18080".into());

    geario::server::net::build()
        .bind("echo", addr, SharedCfg::new("ECHO"), async |_| {
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
