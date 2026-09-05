use std::io;

use ntex_codec::BytesCodec;
use ntex_io::Io;
use ntex_service::cfg::SharedCfg;
use ntex_service::fn_service;

fn main() -> io::Result<()> {
    ntex_rt::System::build()
        .name("bench")
        .build(ntex_net::DefaultRuntime)
        .block_on(async {
            ntex_server::net::build()
                .bind("echo", "127.0.0.1:8081", SharedCfg::new("ECHO"), async |_| {
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
        })
}
