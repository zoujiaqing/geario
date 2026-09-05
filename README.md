# geario

Async IO library for the Neton ecosystem.

geario is the network layer extracted from the [ntex](https://github.com/ntex-rs/ntex)
framework and reorganized into a single crate. It gives you buffers, an event
loop, sockets and a connection dispatcher without pulling in a web framework.

The HTTP protocol layer lives in a separate crate, `geario-http`, which
currently speaks HTTP/1.1.

## Status

Early. The API is not stable yet.

| Crate | What works |
| --- | --- |
| `geario` | Buffers, runtime, sockets, dispatcher, server |
| `geario-http` | HTTP/1.1 server. HTTP/2 is not implemented yet |

## Modules

| Module | What it does |
| --- | --- |
| `bytes` | Reference-counted buffers and byte strings |
| `codec` | `Decoder` and `Encoder` traits |
| `error` | Error types shared across the stack |
| `util` | Timers, channels, future combinators, service utilities |
| `service` | The `Service` trait, pipelines and middleware |
| `rt` | Runtime, arbiters and the system builder |
| `io` | `Io` handles, filters and framed transports |
| `dispatcher` | Generic connection loop for framed protocols |
| `net` | Sockets, connectors and the platform drivers |
| `server` | Worker pool and accept loop |

## Platform drivers

| Driver | Platform |
| --- | --- |
| `polling` | kqueue on macOS, epoll on Linux |
| `uring` | io_uring on Linux |
| `iocp` | Windows |
| `tokio` | Any, via the `tokio` feature |
| `compio` | Any, via the `compio` feature |

Only one runtime feature may be enabled at a time; `build.rs` enforces this.

## Requirements

Rust 1.95 or newer, edition 2024.

## Features

| Feature | Default | What it pulls in |
| --- | --- | --- |
| `http1` | yes | HTTP/1.1 codec, decoder, encoder |
| `server` | yes | `HttpService`, dispatcher, control service |
| `client` | no | `Client`, request builder, connection pool |
| `full` | no | `http1` + `server` + `client` |
| `compress` | no | gzip/deflate transfer encoding |
| `cookie` | no | cookie parsing and building |

A server-only build is about 46% smaller than `full`, which matters when
the library is linked into an FFI target.

Reserved but not implemented: `http2`, `openssl`, `rustls`, `ws`, `test-server`.

## License

MIT OR Apache-2.0, matching upstream ntex. See `NOTICE` for attribution.

## Documentation

- [中文说明](README.zh-Hans.md)
