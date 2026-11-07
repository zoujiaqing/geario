use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let addr = std::env::var("BENCH_ADDR").unwrap_or_else(|_| "127.0.0.1:18080".into());
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        stream.set_nodelay(true)?;
        tokio::spawn(async move {
            let mut buf = [0; 64 * 1024];
            while let Ok(n) = stream.read(&mut buf).await {
                if n == 0 || stream.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        });
    }
}
