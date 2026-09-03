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
| `ntex-bytes` | `src/buf/` | 8,050 |
| `ntex-codec` | `src/codec.rs` | 101 |
| `ntex-error` | `src/error/` | 2,029 |
| `ntex-util` | `src/util/` | 6,510 |
| `ntex-service` | `src/service/` | 6,119 |
| `ntex-rt` | `src/rt/` | 2,831 |
| `ntex-io` | `src/io/` | 5,634 |
| `ntex-dispatcher` | `src/io/dispatcher/` | 1,385 |
| `ntex-net` | `src/net/` | 6,092 |
| `ntex-server` | `src/server/` | 3,311 |
| | **合计** | **42,062** |

大部分 1:1 对应。三处偏离及理由:

- **`bytes` → `buf`**:与 hyper 的 `src/common/buf.rs` 命名对齐;`bytes` 与外部
  `bytes` crate 语义重名。
- **`codec` 不开目录**:全部 101 行、3 个公开项(`Decoder`/`Encoder` trait)。
  但必须保持公开且位置醒目——`geario-http` 将直接实现这两个 trait。
- **`dispatcher` 收进 `io/`**:仅 1 个 `pub fn`、4 个公开类型,是 `io` 的直接下游。

`util/` 保留原名。其内部 `time`/`channel`/`future`/`services` 四块共 6,510 行被
`io`/`net`/`service` 广泛引用,阶段一拆分风险最高,推迟到后续阶段。

### 依赖边界

`service` 无法剥离:`ntex-io` 深度依赖 `ntex_service::cfg::{Cfg, SharedCfg}`,
且 `SharedCfg` 出现在 `net` 的公开 API 上(`tcp_connect(addr, cfg: SharedCfg)`)。

以下外部 crate **保持原名引用**,不 vendor、不改名:

- `ntex-polling`(3.10)— unix readiness 驱动
- `ntex-io-uring`(0.7.120)— Linux io_uring
- 其余第三方:`bytes`、`serde`、`libc`、`socket2`、`slab` 等

依赖包名带 `ntex` 不影响 geario 自身的包名与命名规则,与 hyper 依赖 `httparse` 同理。

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

分三层,风险递增。

### 层一 · 机械替换(可脚本化)

共 380 处 crate 路径引用:

| 原 | 新 | 处数 |
|---|---|---|
| `ntex_bytes::` | `crate::buf::` | 144 |
| `ntex_service::` | `crate::service::` | 63 |
| `ntex_rt::` | `crate::rt::` | 59 |
| `ntex_util::` | `crate::util::` | 38 |
| `ntex_io::` | `crate::io::` | 34 |
| `ntex_error::` | `crate::error::` | 16 |
| `ntex_codec::` | `crate::codec::` | 11 |
| `ntex_net::` | `crate::net::` | 3 |
| `ntex_io_uring` / `ntex_polling` | **不动** | 9 |

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
| `ntex-server/src/pool.rs:38` | `"ntex"` | 需确认用途 |
| `ntex-rt/src/builder.rs:12,34` | `"ntex"` | 需确认用途 |
| `ntex-bytes/src/lib.rs:50` | `"https://docs.rs/ntex-bytes/"` | 文档链接 |
| `ntex-error/src/lib.rs:229,233` | `"ntex_error::tests::test_error"` | 测试断言字符串,随模块路径改 |
| `ntex-net/src/connect/error.rs:45-80` | `"ntex-connect-*"` | 8 处错误标识 |

注:`"IoContext"`(`ntex-io/src/ctx.rs:13,324`)是误匹配,不需改动。

### 层三 · 测试(唯一有真实失败风险的一层)

- 129 处 `#[ntex::test]` → `#[geario::test]`
- 测试体内 `ntex::rt::spawn` → `crate::rt::spawn`(集中在 `ntex-util/src/services/`
  的 `buffer.rs`、`inflight.rs`、`onerequest.rs` 与 `time/mod.rs`)
- 文档注释中的 `use ntex::...` 示例需逐个更新

风险点:`#[ntex::test]` 宏展开后引用 `ntex::rt::System` 等绝对路径,宏内部路径必须
跟随模块结构改动。这是移植过程中最可能卡住的地方,应最先打通一个最小样例验证。

### 层四 · lint 配置

阶段一在 workspace 中**关闭**以下两条:

- `unreachable_pub = "deny"`
- `warnings = "deny"`

理由:10 个 crate 合一后,原本跨 crate 的公开项(约 600 个 `pub fn`、365 个
`pub struct/enum/trait`)变为 crate 内公开,将大量触发 `unreachable_pub`。
阶段一不应在此消耗时间。后续阶段可逐步恢复。

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
| 2 | `uring` 分支本机无法验证 | 中 | 验收拆两批;批次一只覆盖 macOS/polling |
| 3 | 编译时间劣化(42k 行单包,失去 crate 级并行) | 中 | 接受;阶段一不开 `lto` |
| 4 | 上游 4.0.0-beta 仍在快速变动 | 低 | fork 点已固定,阶段一不跟随上游 |

## 9. 后续阶段(仅记录方向,不在本 spec 范围)

- **阶段二**:精简。拆 `util`、恢复 lint、削减不需要的驱动分支。
- **阶段三**:`geario-http`。借鉴 hyper 的分层(`proto/h1`、`proto/h2`、薄 `service`),
  基于 `geario::codec` 的 `Decoder`/`Encoder` trait。h1 codec 在上游已是 IO-less,
  可直接移植;h2 需处理外部 `ntex-h2` 仓库。
- **阶段四**:C ABI 与 Kotlin/Native 对接,复用 hyper4k 已验证的 ABI 设计。
