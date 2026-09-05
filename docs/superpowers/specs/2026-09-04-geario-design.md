# geario 阶段一设计:从 ntex 移植 IO 栈

- 日期:2026-09-04
- 状态:待实现
- 范围:仅 geario。`geario-http` 另开设计。

## 1. 目标

把 ntex 的底层 IO 栈(10 个 crate、42,062 行)整体移植成一个单包 `geario`,
目录结构与命名规则改为 geario 自有,不改任何代码逻辑。

阶段一**不追求性能提升**。移植后性能应与 ntex 基本相同;数字对不上说明移植出错。
精简与优化留到后续阶段。

### 非目标(写在这里当护栏)

阶段一明确**不做**以下任何一项:

- 删除代码或做任何精简
- 修改类型名或任何公开 API 形状
- 引入 rent 模型(`AsyncReadRent`/`IoBufMut`)、自研内存池、`core_affinity` 绑核
  ——这些属于"重写"路线,与本移植路线冲突
- vendor `ntex-polling` / `ntex-io-uring`
- 触碰 TLS 或 HTTP

## 2. 上游 fork 点

| 项 | 值 |
|---|---|
| 仓库 | https://github.com/ntex-rs/ntex |
| commit | `48eef5bd`(Add .readiness() and .shutdown() callbacks to service, #1007) |
| 版本 | ntex 4.0.0-beta.2 |
| 许可证 | MIT OR Apache-2.0 |

fork 点固定。所有基准对比都以此 commit 的 ntex 为对照组;上游后续变更不影响阶段一。

### 许可证与归属

- 原样保留 `LICENSE-MIT` 与 `LICENSE-APACHE`
- 新增 `NOTICE`,声明衍生自 ntex、记录 fork 点 commit、保留原作者署名
- `README.md` 与 `src/lib.rs` 头部注明灵感与代码来源于 ntex

## 3. 仓库结构

`geario` 是独立 git 仓库,位于 `Neton/geario/`。

```
geario/
├── Cargo.toml                     # workspace,2 个成员
├── LICENSE-MIT
├── LICENSE-APACHE
├── NOTICE
├── README.md
├── geario/                        # 唯一对外的包
│   ├── Cargo.toml
│   ├── build.rs                   # 搬自 ntex-rt/build.rs
│   ├── src/                       # 见第 4 节
│   ├── tests/                     # 搬自 ntex-bytes/tests，7 文件 1,072 行
│   ├── benches/                   # 搬自 ntex-bytes/benches
│   └── examples/
│       └── echo.rs
├── geario-macros/                 # proc-macro 必须独立 crate
└── bench-echo/                    # 对标压测，不属于 workspace
    ├── server-ntex/               # 依赖 ntex@48eef5bd
    ├── server-geario/             # 依赖本仓库 geario
    └── client/                    # 同一份客户端打两边
```

### 工具链

沿用上游:`edition = "2024"`,`rust-version = "1.95"`。阶段一不做 MSRV 下调。

### build.rs

必须从 `ntex-rt/build.rs` 移植。它做的是**运行时特性互斥检查**:同时启用
`tokio` 和 `compio` 时 `panic!("Only one runtime feature could be selected")`。
合并为单包后该约束依然成立,遗漏会导致难以诊断的链接错误。

### geario-macros

`ntex-macros` 共 773 行:`lib.rs`(445)、`route.rs`(151)、`sys.rs`(177)。
其中 **`route.rs` 是 web 路由宏,geario 不需要**,不移植。

只移植 `#[main]` / `#[test]` 相关部分。这不是可选项:9 个源 crate 中有 68 个文件
含 `#[cfg(test)]`,`#[ntex::test]` 使用 **129 处**,不移植则全部测试无法编译。

## 4. 模块映射

| ntex crate | geario 模块 | 行数 |
|---|---|---|
| `ntex-bytes` | `src/bytes/` | 8,050 |
| `ntex-codec` | `src/codec/` | 101 |
| `ntex-error` | `src/error/` | 2,029 |
| `ntex-util` | `src/util/` | 6,510 |
| `ntex-service` | `src/service/` | 6,119 |
| `ntex-rt` | `src/rt/` | 2,831 |
| `ntex-io` | `src/io/` | 5,634 |
| `ntex-dispatcher` | `src/dispatcher/` | 1,385 |
| `ntex-net` | `src/net/` | 6,092 |
| `ntex-server` | `src/server/` | 3,311 |
| | **合计** | **42,062** |

**全部 10 个 crate 与模块 1:1 对应**,包括 `bytes` 与 `codec`——即使 `codec` 只有
101 行也开目录,`dispatcher` 也保持顶层独立(理由见下)。

`codec` 的路径是**稳定 API**:`geario-http` 将直接实现 `crate::codec::{Decoder, Encoder}`。
阶段二起不得移动或改名。

### `dispatcher` 为什么保持顶层独立

`Dispatcher<U, Err>` 是**通用帧式协议的连接循环**,不是 `io` 的实现细节:

```rust
pub fn new<Io>(io: Io, codec: U, service: Pipeline<DispatchItem<U>, Option<Response<U>>, Err>)
where U: Decoder + Encoder
```

只要提供一个 `Codec` 和一个 `Service`,整条循环就白送——读帧、派发、写回、
keep-alive 超时、read 超时、写背压(`Control::WBackPressureEnabled/Disabled`)、
优雅关闭(`Reason`)全部内置。这是快速封装新协议的入口。

**依赖方向是 `dispatcher → {io, codec, service, util}`**,它是 `io` 的消费者而非下游
实现,放进 `io/` 会让同一目录混装抽象层与其消费者。

**与 h1 dispatcher 的关系:无。** `ntex-dispatcher` 内**没有任何 trait**
(全文只有 `impl Future for Dispatcher`),`ntex/src/http/h1/dispatcher.rs` 的
`Dispatcher<F, B, Err>` 与之零类型关联,是重名的两个独立 struct。

h1 不复用它的原因是**模型不匹配**:通用 dispatcher 是"一帧进 → 一个 response 出"
(`Option<Response<U>>`),而 HTTP/1.1 的请求体与响应体都是流,还需要独立的 control
service 通道(见 h1 的 `State` 枚举:`ReadRequest`/`ReadPayload`/`SendPayload`/
`CallPublish`/`CallControl`),因此另写了 1,310 行。

实际用户:`ntex/src/ws/client.rs`、`ntex/src/web/ws.rs`,以及生态外部的
`ntex-mqtt`/`ntex-amqp`。`ntex/src/http/` 对 `DispatchItem`/`ntex_dispatcher` 零引用。
**`geario-http`(阶段二)不需要它**;它面向 WebSocket、RPC 与自定义 TCP 协议。

#### 对外路径

主路径改为 `geario::dispatcher::*`,更符合"协议脚手架入口"的定位。
同时在 `geario::io` 下**保留 re-export**,照抄上游 `ntex/src/lib.rs:102-105`:

```rust
pub mod io {
    pub use crate::dispatcher::*;
    pub use crate::io::*;   // 实际写法见实现计划
}
```

保留别名的目的是让移植过来的代码与示例(`crate::io::Dispatcher::new(...)`)一行不改,
不破坏"不改公开 API 形状"的护栏;日后要弃用 `io` 下的别名也干净。
该改动零行为、零性能影响,不威胁 benchmark 可比性。

`util/` 保留原名。其内部 `time`/`channel`/`future`/`services` 四块共 6,510 行被
`io`/`net`/`service` 广泛引用,阶段一拆分风险最高,推迟到后续阶段。

### `src/bytes/` 的嵌套注意事项

`ntex-bytes` 自身已有一个内部模块 `src/bytes.rs`,且同时依赖外部 `bytes` crate
(`bvec.rs:606`、`bytes.rs:550` 使用 `bytes::buf::Buf`)。移植后:

- 外部 crate 仍写 `bytes::buf::Buf` —— Rust 2018+ 的 `use`/路径首段解析为外部 crate,
  **不与 `crate::bytes` 冲突**,上游本来就是这么共存的
- 但 `ntex-bytes/src/lib.rs:80` 的 `pub use crate::bytes::Bytes;` 会变成三层嵌套
  `crate::bytes::bytes::Bytes`。在 `src/bytes/mod.rs` 中改写为 `pub use self::bytes::Bytes;`

### 依赖边界

`service` 无法剥离:`ntex-io` 深度依赖 `ntex_service::cfg::{Cfg, SharedCfg}`,
且 `SharedCfg` 出现在 `net` 的公开 API 上(`tcp_connect(addr, cfg: SharedCfg)`)。

以下外部 crate **保持原名引用**,不 vendor、不改名:

- `ntex-polling`(3.10)— unix readiness 驱动
- `ntex-io-uring`(0.7.120)— Linux io_uring
- 其余第三方:`bytes`、`serde`、`libc`、`socket2`、`slab` 等

依赖包名带 `ntex` 不影响 geario 自身的包名与命名规则,与 hyper 依赖 `httparse` 同理。

#### 必须切断的一条依赖:`ntex-http`

`ntex-net/Cargo.toml:49` 声明了 `ntex-http = { workspace = true }`。全部 10 个 crate 内
该依赖只被用到**一处**:

- `ntex-net/src/connect/uri.rs:1` — `use ntex_http::Uri;`(整个文件 58 行,`Uri` 仅用于
  `impl Address for Uri`)
- 而 `ntex_http::Uri` 就是 `pub use http::uri::{self, Uri};`(`ntex-http/src/lib.rs:26`)

保留这条依赖会违反第 1 节"不触碰 HTTP"的护栏,并把整个 `ntex-http`(2,988 行)
拖进阶段一。处理:

1. `connect/uri.rs:1` 改为 `use http::uri::Uri;`
2. `geario/Cargo.toml` 增加 `http` 依赖(版本与上游 workspace 对齐)
3. `connect/uri.rs` 其余 57 行不动

这是阶段一唯一一处**主动切断**的上游依赖,必须显式记录,否则实施时会编译失败且
错误信息不指向根因。

### 平台驱动

`ntex-net/src/` 已内置全部驱动,移植后直接可用:

| 目录 | cfg | 平台 |
|---|---|---|
| `polling/` | `unix` | macOS kqueue / Linux epoll |
| `uring/` | `target_os = "linux"` | io_uring |
| `iocp/` | `windows` | IOCP |
| `compio/` | feature | — |
| `tokio/` | feature | — |

### Feature 表

沿用上游,合并到单个 `[features]`:

```
default = []
tokio        # 与 compio 互斥，build.rs 强制
compio       # 与 tokio 互斥
neon-polling
neon-uring
neon-iocp
trace
simd         # 原 ntex-bytes/simd
overuse      # 原 ntex-bytes/overuse
```

## 5. 重命名规程

分五层,风险递增。层 1a 与 1b 都是机械替换,但**执行顺序不可交换**。

### 层 1a · 内部 `crate::` 提升(必须在合并目录之前做)

每个源 crate 内部的 `crate::X` 引用,合并后都要变成 `crate::<模块>::X`。
以下为 `src/` 内计数,共 **348 处**:

| 源 crate | `crate::` → | 处数 |
|---|---|---|
| `ntex-service` | `crate::service::` | 95 |
| `ntex-util` | `crate::util::` | 65 |
| `ntex-io` | `crate::io::` | 42 |
| `ntex-rt` | `crate::rt::` | 40 |
| `ntex-net` | `crate::net::` | 40 |
| `ntex-bytes` | `crate::bytes::` | 38 |
| `ntex-error` | `crate::error::` | 17 |
| `ntex-server` | `crate::server::` | 11 |
| `ntex-codec` / `ntex-dispatcher` | — | 0 |
| | **合计** | **348** |

**关键约束:替换成什么取决于文件来自哪个源 crate,因此必须按源目录分 10 趟执行,
且必须在把文件挪进 `geario/src/` 之前完成。** 合并后再做就无法区分来源了。

`super::` 与 `self::` 是相对路径,不受影响,不动。

已知例外:`ntex-bytes/src/lib.rs:80` 的 `pub use crate::bytes::Bytes;` 机械替换后会得到
三层嵌套 `crate::bytes::bytes::Bytes`。手工改为 `pub use self::bytes::Bytes;`。

### 层 1b · 跨 crate 路径替换

共 **378 处**(`src/` 内计数;`tests/` 与 `benches/` 另计,见层三):

| 原 | 新 | 处数 |
|---|---|---|
| `ntex_bytes::` | `crate::bytes::` | 144 |
| `ntex_service::` | `crate::service::` | 63 |
| `ntex_rt::` | `crate::rt::` | 59 |
| `ntex_util::` | `crate::util::` | 38 |
| `ntex_io::` | `crate::io::` | 34 |
| `ntex_error::` | `crate::error::` | 16 |
| `ntex_codec::` | `crate::codec::` | 11 |
| `ntex_net::` | `crate::net::` | 3 |
| `ntex_http::Uri` | `http::uri::Uri` | 1 |
| `ntex_io_uring::` / `ntex_polling::` | **不动**(外部 crate) | 9 |

两层合计 **726 处**机械替换。

**类型名一律不动。** 已核实上游零个 `Ntex*` 前缀标识符;`Io`、`IoRef`、`Bytes`、
`BytesMut`、`Framed`、`Cfg`、`Service` 等全部原样保留。这使重命名从重构降级为替换。

### 层二 · 人工处理字符串字面量(约 20 处)

需逐个判断,重点:

| 位置 | 内容 | 处理 |
|---|---|---|
| `ntex-rt/src/driver.rs:118` | `"ntex/ntex-rt/src/driver.rs"` | 硬编码路径,改目录后失效,必须更新 |
| `ntex-server/src/net/accept.rs:79` | `"ntex:accept"` | 线程名,会出现在 `ps`/profiler 输出 |
| `ntex-net/src/lib.rs:84` | `panic!("not in a ntex driver")` | 改为 geario |
| `ntex-server/src/signals.rs:76,116` | `"ntex-server signals"` | 线程名 |
| `ntex-server/src/manager.rs:310` | `"Stopping ntex system, {:?} server"` | 日志 |
| `ntex-server/src/pool.rs:38` | `name: "ntex".to_string()` | `WorkerPool` 默认线程池名(可被 `.name()` 覆盖)→ `"geario"` |
| `ntex-rt/src/builder.rs:34` | `name: "ntex".into()` | `System` 默认名(可被 `.name()` 覆盖)→ `"geario"` |
| `ntex-rt/src/builder.rs:12` | `/// Defaults to "ntex" if unset.` | doc 注释,随上一行同步改 |
| `ntex-bytes/src/lib.rs:50` | `#![doc(html_root_url = "https://docs.rs/ntex-bytes/")]` | **crate 级属性**,不是普通文档链接。合并后移到 `geario/src/lib.rs` 顶层,改为 `https://docs.rs/geario/` |
| `ntex-error/src/lib.rs:229,233` | `"ntex_error::tests::test_error"` | 测试断言字符串,随模块路径改 |
| `ntex-net/src/connect/error.rs:45-80` | `"ntex-connect-*"` | 8 处错误标识 |

注:`"IoContext"`(`ntex-io/src/ctx.rs:13,324`)是误匹配,不需改动。

### 层三 · 测试(唯一有真实失败风险的一层)

**单元测试**(`src/` 内 `#[cfg(test)] mod`,68 个文件):

- 129 处 `#[ntex::test]` → `#[geario::test]`
- 测试体内 `ntex::rt::spawn` → `crate::rt::spawn`(集中在 `ntex-util/src/services/`
  的 `buffer.rs`、`inflight.rs`、`onerequest.rs` 与 `time/mod.rs`)
- 文档注释中的 `use ntex::...` 示例需逐个更新

**集成测试**(`ntex-bytes/tests/`,7 个文件 1,072 行):

| 文件 | 行数 |
|---|---|
| `test_bytes.rs` | 738 |
| `test_buf.rs` | 160 |
| `test_buf_mut.rs` | 64 |
| `test_bytes_stress.rs` | 39 |
| `test_debug.rs` | 35 |
| `test_iter.rs` | 21 |
| `test_serde.rs` | 15 |

**决定:整体移到 `geario/tests/`,保留集成测试身份**(而非并入 `src/bytes/` 的
`#[cfg(test)] mod`)。理由:改动最小,且保留"从 crate 外部使用公开 API"这一语义——
这正是它们存在的价值,合并进 `src/` 会让它们能看见私有项,削弱测试强度。

替换:`use ntex_bytes::` → `use geario::bytes::`。`tests/` 内 **10 处**,
`benches/` 内 **2 处**,均不在层 1b 的 378 处计数内。

风险点:`#[ntex::test]` 宏展开后引用 `ntex::rt::System` 等绝对路径,宏内部路径必须
跟随模块结构改动。这是移植过程中最可能卡住的地方,应最先打通一个最小样例验证。

### 层四 · lint 配置

- **`unreachable_pub`**:在 `geario/src/lib.rs` 顶部加 `#![allow(unreachable_pub)]`
  单点关闭。**不动 workspace lints 表。**
- **`warnings = "deny"`**:**保留**。

理由:10 个 crate 合一后,原本跨 crate 的公开项(约 600 个 `pub fn`、365 个
`pub struct/enum/trait`)变为 crate 内公开,`unreachable_pub` 会大量误报,必须关。

但 `warnings = "deny"` 不能一起关。阶段一"移植不改逻辑"恰恰是**验证告警是否退化的
最佳窗口期**:此时任何新出现的告警都必然来自移植操作本身,而不是新写的代码。
把这个闸门关掉,阶段二想恢复时会发现告警已经堆积,难以区分历史债与新债。
用 crate 级 `allow` 精确豁免一条,比用 workspace 级放开一整类更可控。

**例外处理**:若合并后出现 `unreachable_pub` 之外的**大批**告警(预计主要是
`dead_code`——某些公开项原本只被上层 `ntex` crate 使用,移植后在 geario 内无调用方),
逐条记录清单后再决定豁免哪些,不做无差别关闭。

## 6. 验收标准

因 Linux 环境后续才可用,阶段一验收分两批。

### 批次一 · macOS(本机,立即可做)

1. `cargo build` 通过(`polling` 驱动)
2. `cargo test` 全绿,129 个 `#[geario::test]` 一个不少
3. `cargo bench` 的 buf/bytes 微基准 vs ntex@`48eef5bd`,差异 **±3% 以内**
4. TCP echo server 压测 vs ntex@`48eef5bd`,同机同参数,差异 **±3% 以内**
5. `grep -ri ntex geario/src/` 只剩两类结果:外部 crate 名
   (`ntex-polling`/`ntex-io-uring`)、`NOTICE`/`README` 中的归属声明

### 批次二 · Linux(服务器到位后)

6. `cargo build` 通过(`polling` 与 `uring` 两个驱动)
7. `cargo test` 全绿(两个驱动)
8. 微基准与 echo 压测 vs ntex@`48eef5bd`,**±3% 以内**(两个驱动)

### 关于 ±3%

采用"±3% 以内"而非"追平",因为移植不改逻辑,性能差异理论上应为零。唯一变量是
合并单包后跨 crate 调用变为 crate 内调用,LLVM 内联更自由(可能略快)。
**数字显著偏离即说明移植出错**——这是免费的正确性检验,比模糊的"追平"更有用。

## 7. 基准测试设计

上游 ntex **几乎没有基准测试**:整个 workspace 仅 `ntex-bytes/benches/{buf.rs, bytes.rs}`
两个文件,无 `[[bench]]` 段,io/net/rt/server 链路零基准。因此网络层基准需自建。

采用双轨:

### 轨道一 · criterion 微基准

直接移植 `ntex-bytes/benches/` 到 `geario/benches/`。零成本、结果稳定、可进 CI。
验证 buf 层未移植出错。

### 轨道二 · TCP echo 压测

位于本仓库 `bench-echo/`,**不属于 geario workspace**(否则 `server-ntex` 会把上游
ntex 拖进主 workspace 的依赖树)。三个独立 crate:`server-ntex`、`server-geario`、
`client`。两版 server 除 import 路径外代码完全相同,基于 `ntex/examples/echo.rs` 改写。
同一份客户端分别打两边,验证 io/net/rt/server 整条链路。

两轨缺一不可:轨道一稳定但碰不到 syscall 路径;轨道二覆盖真实链路但噪声大,
数字异常时需要轨道一来区分是 buf 层还是驱动层的问题。

## 8. 已知风险

| # | 风险 | 严重度 | 缓解 |
|---|---|---|---|
| 1 | 测试层移植(129 处 + geario-macros 路径改写) | 高 | 最先打通一个最小 `#[geario::test]` 样例再批量改 |
| 2 | **层 1a 顺序错误**:若先合并目录再做 `crate::` 提升,348 处将无法区分来源,只能逐文件人工判断 | 高 | 严格按层 1a → 层 1b 顺序;每个源目录替换完成后立即 `git commit`,留下可回退点 |
| 3 | `uring` 分支本机无法验证 | 中 | 验收拆两批;批次一只覆盖 macOS/polling |
| 4 | 编译时间劣化(42k 行单包,失去 crate 级并行) | 中 | 接受;阶段一不开 `lto` |
| 5 | 上游 4.0.0-beta 仍在快速变动 | 低 | fork 点已固定,阶段一不跟随上游 |

## 9. 后续阶段(仅记录方向,不在本 spec 范围)

- **阶段二**:`geario-http`。借鉴 hyper 的分层(`proto/h1`、`proto/h2`、薄 `service`),
  基于 `geario::codec` 的 `Decoder`/`Encoder` trait。h1 codec 在上游已是 IO-less,
  可直接移植;h2 需处理外部 `ntex-h2` 仓库。
- **阶段三**:精简。拆 `util`、恢复 `unreachable_pub`、削减不需要的驱动分支。
- **阶段四**:C ABI 与 Kotlin/Native 对接,复用 hyper4k 已验证的 ABI 设计。

### 为什么 `geario-http` 排在精简之前

精简属于整理内务,对上层价值有限,而且**在不知道 http 层需要什么的前提下精简,
很可能删掉后续要用的抽象**。

`geario-http` 才是验证 geario 抽象是否合理的唯一手段——`codec::{Decoder, Encoder}`、
`service::cfg::SharedCfg`、`io::Io` 这几处边界画得对不对,只有真的被 http 用起来才知道。
先做 http、再照着实际用法精简,顺序更稳。

C ABI 放最后,因为 hyper4k 已经把这条路径踩通,风险最低。

**精简阶段的护栏**:不得改动 `codec`、`service::cfg`、`io` 的公开 API 形状,
否则 `geario-http` 需要回炉。
