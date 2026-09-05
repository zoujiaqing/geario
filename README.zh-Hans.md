# geario

面向 Neton 生态的异步 IO 库。

geario 是从 [ntex](https://github.com/ntex-rs/ntex) 框架中提取出来的网络层，
重新整理成了单个 crate。它提供缓冲区、事件循环、套接字和连接调度器，
不附带任何 Web 框架。

HTTP 协议层在独立仓库 [geario-http](https://github.com/zoujiaqing/geario-http)。

## 状态

早期阶段，API 尚未稳定。

## Crates

| Crate | 内容 |
| --- | --- |
| `geario` | 全部：缓冲区、运行时、套接字、dispatcher、server |
| `geario-macros` | `#[geario::main]` 与 `#[geario::test]` |

`geario-macros` 单独成 crate 纯粹是 Rust 的硬性要求：`proc-macro` crate
除了宏之外不能导出任何东西。你不需要在自己的 `Cargo.toml` 里写它——
`geario` 已经把两个属性再导出了。

## 模块

| 模块 | 职责 |
| --- | --- |
| `bytes` | 引用计数缓冲区与字节串 |
| `codec` | `Decoder` 与 `Encoder` trait |
| `error` | 全栈共用的错误类型 |
| `util` | 定时器、通道、future 组合子、service 工具 |
| `service` | `Service` trait、pipeline 与中间件 |
| `rt` | 运行时、arbiter 与 system builder |
| `io` | `Io` 句柄、filter 与 framed 传输 |
| `dispatcher` | 帧式协议的通用连接循环 |
| `net` | 套接字、连接器与各平台驱动 |
| `server` | worker 池与 accept 循环 |

## 平台驱动

| 驱动 | 平台 |
| --- | --- |
| `polling` | macOS 用 kqueue，Linux 用 epoll |
| `uring` | Linux io_uring |
| `iocp` | Windows |
| `tokio` | 任意平台，需开启 `tokio` feature |
| `compio` | 任意平台，需开启 `compio` feature |

同一时间只能启用一个运行时 feature，`build.rs` 会强制检查。

## 环境要求

Rust 1.95 或更高，edition 2024。

## Feature

| Feature | 默认 | 内容 |
| --- | --- | --- |
| `http1` | 是 | HTTP/1.1 codec、decoder、encoder |
| `server` | 是 | `HttpService`、dispatcher、control service |
| `client` | 否 | `Client`、请求构建器、连接池 |
| `full` | 否 | `http1` + `server` + `client` |
| `compress` | 否 | gzip/deflate 传输编码 |
| `cookie` | 否 | cookie 解析与构建 |

只开 server 的构建比 `full` 小约 46%，这在把库链进 FFI 目标时是实际收益。

已预留但尚未实现：`http2`、`openssl`、`rustls`、`ws`、`test-server`。

## 许可证

MIT OR Apache-2.0，与上游 ntex 一致。归属信息见 `NOTICE`。
