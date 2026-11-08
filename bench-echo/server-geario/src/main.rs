use std::io;

use geario::codec::BytesCodec;
use geario::io::{Io, IoConfig};
use geario::service::cfg::SharedCfg;
use geario::service::fn_service;

#[geario::main]
async fn main() -> io::Result<()> {
    // Configurable because the default collides with whatever else the host
    // happens to be running.
    let addr = std::env::var("BENCH_ADDR").unwrap_or_else(|_| "127.0.0.1:18080".into());

    let mut server = geario::server::net::build();
    if let Ok(workers) = std::env::var("BENCH_WORKERS") {
        let workers = workers
            .parse::<std::num::NonZeroUsize>()
            .map_err(io::Error::other)?;
        server = server.workers(workers.get());
    }
    let mut cfg = SharedCfg::new("ECHO");
    if let Ok(capacity) = std::env::var("BENCH_READ_CAPACITY") {
        let capacity = capacity
            .parse::<std::num::NonZeroUsize>()
            .map_err(io::Error::other)?;
        cfg = cfg.add(IoConfig::default().set_read_buf(capacity.get(), 536, 128));
    }
    let segments = match std::env::var("BENCH_SEGMENTS").as_deref() {
        Ok("vectored") => Some(false),
        Ok("serial") => Some(true),
        Err(std::env::VarError::NotPresent) => None,
        _ => {
            return Err(io::Error::other(
                "BENCH_SEGMENTS must be vectored or serial",
            ));
        }
    };
    let direct = std::env::var_os("BENCH_DIRECT_WRITE").is_some();
    server
        .bind("echo", addr, cfg, async move |_| {
            fn_service(async move |io: Io| {
                let codec = BytesCodec;
                while let Ok(Some(mut item)) = io.recv(&codec).await {
                    if let Some(serial) = segments {
                        if serial {
                            while !item.is_empty() {
                                let chunk = item.split_to(item.len().min(16 * 1024));
                                io.get_ref().encode_bytes(chunk)?;
                                io.flush(true).await?;
                            }
                        } else {
                            // Queue the whole batch before the write hook can run.
                            io.get_ref().with_write_buf(|pages| {
                                while !item.is_empty() {
                                    let chunk = item.split_to(item.len().min(16 * 1024));
                                    pages.append(chunk);
                                }
                            })?;
                            io.flush(true).await?;
                        }
                        continue;
                    }
                    if direct {
                        let n = io
                            .get_ref()
                            .try_write_vectored(&[io::IoSlice::new(&item)])?;
                        if n == item.len() {
                            continue;
                        }
                        use geario::bytes::Buf;
                        item.advance(n);
                    }
                    if io.send(item, &codec).await.is_err() {
                        break;
                    }
                }
                Ok::<_, io::Error>(())
            })
        })?
        .run()
        .await
}
