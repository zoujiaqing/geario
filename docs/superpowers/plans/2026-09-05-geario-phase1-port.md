# geario 阶段一实现计划:从 ntex 移植 IO 栈

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 ntex 的底层 10 个 crate(42,062 行)移植成单包 `geario`,目录与命名改为 geario 自有,代码逻辑一行不改。

**Architecture:** 按依赖拓扑序逐个模块搬运,每搬一个模块 `cargo check` 必须通过。不采用"先全部替换再合并"——那样中途无法编译,错误会堆积到最后一次性爆发。拓扑序为 `bytes → service → codec → error → rt → util → io → dispatcher → net → server`,该序列已核实为严格线性无环。

**Tech Stack:** Rust edition 2024,rust-version 1.95(本机需 ≥1.95,见 Task 0),cargo workspace,criterion。

**Spec:** `docs/superpowers/specs/2026-09-04-geario-design.md`

## Global Constraints

以下约束适用于**每一个** Task,不再逐条重复:

- **fork 点固定为 ntex `48eef5bd`**(4.0.0-beta.2)。源码一律从 `/Users/zoujiaqing/projects/Neton/ntex/` 复制,不从 crates.io 拉。
- **不改代码逻辑。** 除本计划显式列出的路径改写、字符串改写、`Cargo.toml` 组装外,任何 `.rs` 内容改动都是缺陷。
- **不改类型名。** 上游零个 `Ntex*` 前缀标识符;`Io`、`IoRef`、`Bytes`、`BytesMut`、`Framed`、`Cfg`、`Service` 等一律原样。
- **不做精简、不删代码、不改公开 API 形状。**
- **禁止引入** rent 模型(`AsyncReadRent`/`IoBufMut`)、自研内存池、新的 `core_affinity` 用法(`server` 模块里上游已有的除外)。
- **不 vendor** `ntex-polling` / `ntex-io-uring`,保持原名作为外部依赖。
- **不碰 TLS、不碰 HTTP**(唯一例外:Task 9 切断 `ntex-http`,改用 `http` crate)。
- **edition = "2024"**,**rust-version = "1.95"**。
- 提交信息用英文,不含任何 AI 署名。
- 每个 Task 结束必须 commit,留下可回退点。

## 模块映射(全程参照)

| ntex crate | geario 模块 | 行数 | 内部 `crate::` 处数 |
|---|---|---|---|
| `ntex-bytes` | `src/bytes/` | 8,050 | 38 |
| `ntex-service` | `src/service/` | 6,119 | 95 |
| `ntex-codec` | `src/codec/` | 101 | 0 |
| `ntex-error` | `src/error/` | 2,029 | 17 |
| `ntex-rt` | `src/rt/` | 2,831 | 40 |
| `ntex-util` | `src/util/` | 6,510 | 65 |
| `ntex-io` | `src/io/` | 5,634 | 42 |
| `ntex-dispatcher` | `src/dispatcher/` | 1,385 | 0 |
| `ntex-net` | `src/net/` | 6,092 | 40 |
| `ntex-server` | `src/server/` | 3,311 | 11 |

## 两条通用改写规则

每个模块 Task 都要做这两件事,**顺序不可交换**:

**规则 A —— 内部 `crate::` 提升(先做)**

把该 crate 源码内所有 `crate::` 改成 `crate::<模块名>::`。必须在文件挪进 `geario/src/` **之前**做,合并后就分不清来源了。

**规则 B —— 跨 crate 路径替换(后做)**

| 原 | 新 |
|---|---|
| `ntex_bytes::` | `crate::bytes::` |
| `ntex_service::` | `crate::service::` |
| `ntex_codec::` | `crate::codec::` |
| `ntex_error::` | `crate::error::` |
| `ntex_rt::` | `crate::rt::` |
| `ntex_util::` | `crate::util::` |
| `ntex_io::` | `crate::io::` |
| `ntex_net::` | `crate::net::` |
| `ntex_io_uring::` / `ntex_polling::` | **不动**(外部 crate) |

**注意规则 A 与 B 的顺序陷阱**:先做 A 会把 `crate::` 变成 `crate::bytes::` 等;此时再做 B,B 产生的 `crate::bytes::` 不会被 A 二次处理,因为 A 已经跑完了。反过来先做 B 则会让 B 产生的 `crate::bytes::` 被 A 误伤成 `crate::bytes::bytes::`。**必须 A 先 B 后。**

---

## Task 0: 工具链前置检查

**Files:**
- Create: `rust-toolchain.toml`

**Interfaces:**
- Produces: 一个满足 `rust-version = "1.95"` 的可用工具链,后续所有 Task 依赖它。

- [ ] **Step 1: 确认 rustc 版本 ≥ 1.95**

```bash
rustc --version
```

预期:`1.95.0` 或更高。若低于 1.95,先跑 `rustup update stable`(本机 2026-09-05 实测可从 1.92.0 更新到 1.98.1)。

**这是硬阻塞**:ntex `48eef5bd` 的 `rust-version = "1.95"`,低于此版本 cargo 直接拒绝解析依赖,连 `cargo check` 都进不去。

- [ ] **Step 2: 写 rust-toolchain.toml**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: 验证**

```bash
cd /Users/zoujiaqing/projects/Neton/geario && rustc --version
```

预期:输出 ≥ 1.95 的版本号。

- [ ] **Step 4: Commit**

```bash
git add rust-toolchain.toml
git commit -m "Pin toolchain to stable for the 1.95 minimum"
```

---

## Task 1: 仓库骨架与 License 归属

**Files:**
- Create: `Cargo.toml`(workspace 根)
- Create: `LICENSE-MIT`、`LICENSE-APACHE`(从 ntex 复制)
- Create: `NOTICE`
- Create: `README.md`
- Create: `geario/Cargo.toml`
- Create: `geario/src/lib.rs`
- Create: `geario/build.rs`

**Interfaces:**
- Produces: 一个能 `cargo check` 通过的空 `geario` 包;workspace 依赖表(后续 Task 只往 `geario/Cargo.toml` 的 `[dependencies]` 加条目,不再改根 `Cargo.toml`)。

- [ ] **Step 1: 复制 License 文件**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
cp ../ntex/LICENSE-MIT ../ntex/LICENSE-APACHE .
```

- [ ] **Step 2: 写 NOTICE**

```
geario

This product includes software derived from ntex.

  ntex — https://github.com/ntex-rs/ntex
  Copyright (c) ntex contributors <team@ntex.rs>
  Licensed under MIT OR Apache-2.0

Derived at upstream commit 48eef5bd (ntex 4.0.0-beta.2).

The following geario modules are ports of ntex crates, with directory
layout and crate paths adapted to geario. Code logic is unchanged:

  src/bytes/       from ntex-bytes
  src/codec/       from ntex-codec
  src/error/       from ntex-error
  src/util/        from ntex-util
  src/service/     from ntex-service
  src/rt/          from ntex-rt
  src/io/          from ntex-io
  src/dispatcher/  from ntex-dispatcher
  src/net/         from ntex-net
  src/server/      from ntex-server

geario-macros is a port of ntex-macros, limited to the runtime
attribute macros; the web routing macros were not ported.
```

- [ ] **Step 3: 写根 Cargo.toml**

依赖版本全部照抄 ntex `Cargo.toml` 的 `[workspace.dependencies]`,不自行升级。

```toml
[workspace]
resolver = "2"
members = ["geario", "geario-macros"]

[workspace.package]
authors = ["zoujiaqing <zoujiaqing@gmail.com>"]
repository = "https://github.com/netonstream/geario"
license = "MIT OR Apache-2.0"
edition = "2024"
rust-version = "1.95"

[workspace.lints.rust]
async_fn_in_trait = { level = "allow", priority = -1 }
unknown_lints = { level = "allow", priority = -1 }
rust_2018_idioms = "deny"
warnings = "deny"
missing_debug_implementations = "deny"
unexpected_cfgs = { level = "warn", priority = -2, check-cfg = ['cfg(docsrs_dep)'] }

[workspace.dependencies]
geario = { path = "geario" }
geario-macros = { path = "geario-macros" }

ntex-polling = "3.10.0"
ntex-io-uring = "0.7.120"

async-channel = "2"
async-task = "4.5.0"
atomic-waker = "1.1"
backtrace = "0.3.76"
bitflags = "2"
bytes = "1.11.0"
cfg-if = "1.0.0"
core_affinity = "0.8"
crossbeam-channel = "0.5.8"
crossbeam-queue = "0.3.8"
ctrlc = "3.4"
env_logger = { version = "0.11", default-features = false }
foldhash = "0.2.0"
futures-core = { version = "0.3.33", default-features = false, features = ["alloc"] }
futures-timer = "3.0"
hashbrown = { version = "0.17.1", features = ["serde"] }
http = "1.5.0"
libc = "0.2.189"
log = "0.4"
nix = "0.31.3"
oneshot = { version = "0.2.1", features = ["std", "async"] }
parking_lot = "0.12.5"
pin-project-lite = "0.2"
proc-macro2 = "1.0.105"
quote = "1.0.43"
scoped-tls = "1.0.1"
serde = { version = "1", features = ["derive"] }
signal-hook = "0.4.4"
simdutf8 = "0.1.5"
slab = "0.4.9"
socket2 = "0.6.1"
swap-buffer-queue = "0.2.1"
syn = "3.0"
thiserror = "2"
tok-io = { version = "1", package = "tokio", default-features = false }
uuid = { version = "1.19", features = ["v7"] }
windows-sys = "0.61.0"

compio-buf = "0.8.0"
compio-io = "0.9.0"
compio-net = "0.11.0"
compio-driver = "0.11.1"
compio-runtime = "0.11.0"

criterion = "0.5"
rand = "0.9"
serde_json = "1"
serde_test = "1.0"
```

**注意:根 workspace lints 里故意没有 `unreachable_pub = "deny"`**(spec 层四:合并后会大量误报),但 **`warnings = "deny"` 保留**。

- [ ] **Step 4: 写 geario/Cargo.toml 骨架**

后续每个模块 Task 会往 `[dependencies]` 里加条目。此处只放最小集。

```toml
[package]
name = "geario"
version = "0.1.0"
description = "Async IO stack for the Neton ecosystem, ported from ntex"
keywords = ["network", "async", "io", "io-uring"]
categories = ["network-programming", "asynchronous"]
authors.workspace = true
repository.workspace = true
license.workspace = true
edition.workspace = true
rust-version.workspace = true
build = "build.rs"

[lib]
name = "geario"
path = "src/lib.rs"

[lints]
workspace = true

[features]
default = []
tokio = []
compio = []
neon-polling = []
neon-uring = []
neon-iocp = []
trace = []
simd = []
overuse = []

[dependencies]

[dev-dependencies]
```

- [ ] **Step 5: 写 build.rs**

逐字移植自 `ntex/ntex-rt/build.rs`,只改 panic 文案里的措辞不动语义。

```rust
use std::{collections::HashSet, env};

fn main() {
    let mut features = HashSet::<&'static str>::default();

    for (key, _) in env::vars() {
        let _ = match key.as_ref() {
            "CARGO_FEATURE_COMPIO" => features.insert("compio"),
            "CARGO_FEATURE_TOKIO" => features.insert("tokio"),
            _ => false,
        };
    }

    if features.len() > 1 {
        panic!("Only one runtime feature could be selected, current selection {features:?}");
    }
}
```

- [ ] **Step 6: 写 src/lib.rs 骨架**

`extern crate self as geario;` 是关键:它让 `geario::` 路径在 crate **内部**也能解析。
`geario-macros` 展开出的 `geario::rt::System` 等绝对路径依赖这一行(见 Task 2)。

```rust
//! Async IO stack for the Neton ecosystem.
//!
//! Ported from ntex (<https://github.com/ntex-rs/ntex>, MIT OR Apache-2.0)
//! at commit 48eef5bd. See NOTICE for details.
#![doc(html_root_url = "https://docs.rs/geario/")]
#![allow(unreachable_pub)]

extern crate self as geario;
```

- [ ] **Step 7: 写 README.md**

```markdown
# geario

Async IO stack for the [Neton](https://github.com/netonstream) ecosystem.

geario is derived from [ntex](https://github.com/ntex-rs/ntex)'s IO layer,
merged into a single crate with geario's own module layout. The protocol
layer lives in a separate crate, `geario-http`.

Licensed under MIT OR Apache-2.0. See NOTICE for attribution.
```

- [ ] **Step 8: 验证空包能编译**

```bash
cd /Users/zoujiaqing/projects/Neton/geario && cargo check
```

预期:`Checking geario v0.1.0` 后成功。若报 `geario-macros` 缺失,先把根 `Cargo.toml` 的 `members` 暂时改成 `["geario"]`,Task 2 再加回。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Add workspace skeleton, license files and attribution notice"
```

---

## Task 2: geario-macros

**Files:**
- Create: `geario-macros/Cargo.toml`
- Create: `geario-macros/src/lib.rs`(移植自 `ntex-macros/src/lib.rs`,去掉 web 路由部分)
- Create: `geario-macros/src/sys.rs`(移植自 `ntex-macros/src/sys.rs`)
- 不移植:`ntex-macros/src/route.rs`(151 行,web 路由宏)

**Interfaces:**
- Produces: `#[geario::test]`、`#[geario::main]` 两个属性宏。后续 129 处单元测试依赖它们。
- Consumes: Task 1 的 `extern crate self as geario;`。

**为什么这个 Task 排在所有模块之前:** spec 把测试层列为最高风险。宏展开出的是**硬编码绝对路径**,必须先把它跑通,否则模块搬到第 9 个才发现测试全废。

- [ ] **Step 1: 复制并裁剪源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario-macros/src
cp ../ntex/ntex-macros/src/lib.rs ../ntex/ntex-macros/src/sys.rs geario-macros/src/
```

然后编辑 `geario-macros/src/lib.rs`:
- 删除 `mod route;` 声明
- 删除全部 `web_*` proc macro 函数(`web_get`/`web_post`/`web_put`/`web_delete`/`web_head`/`web_connect`/`web_options`/`web_trace`/`web_patch`/`web_query`,位于原文件 58-218 行区间)
- 保留 `rt_main`、`rt_test`、`rt_test2` 及 `mod sys;`

- [ ] **Step 2: 把宏展开里的 ntex:: 改成 geario::**

`ntex-macros/src/lib.rs` 的 `quote!` 块里有硬编码路径,共 7 处 `DefaultRuntime` 引用点。改写规则:

| 原 | 新 |
|---|---|
| `ntex::util::enable_test_logging()` | `geario::util::enable_test_logging()` |
| `ntex::rt::System` | `geario::rt::System` |
| `ntex::rt::DefaultRuntime` | `geario::rt::DefaultRuntime` |
| `crate::rt::DefaultRuntime`(`rt_test2` 用) | 保持 `crate::rt::DefaultRuntime` |

`sys.rs:49` 的 `quote!(ntex::rt::DefaultRuntime)` 同样改成 `geario::rt::DefaultRuntime`。

**为什么 `geario::` 能在 geario 内部解析:** Task 1 的 `extern crate self as geario;`。

- [ ] **Step 3: 写 geario-macros/Cargo.toml**

```toml
[package]
name = "geario-macros"
version = "0.1.0"
description = "Runtime attribute macros for geario"
authors.workspace = true
repository.workspace = true
license.workspace = true
edition.workspace = true
rust-version.workspace = true

[lib]
proc-macro = true

[lints]
workspace = true

[dependencies]
quote = { workspace = true }
proc-macro2 = { workspace = true }
syn = { workspace = true, features = ["full", "parsing"] }
```

- [ ] **Step 4: 在 geario 里接上宏**

`geario/Cargo.toml` 的 `[dependencies]` 加:

```toml
geario-macros = { workspace = true }
```

`geario/src/lib.rs` 加:

```rust
pub use geario_macros::{main, test};
```

注意上游把 `rt_main` 导出成 `main`、`rt_test` 导出成 `test`。确认 `geario-macros/src/lib.rs` 里 `#[proc_macro_attribute]` 函数名与此处 `pub use` 一致;若函数名仍是 `rt_main`/`rt_test`,写成 `pub use geario_macros::{rt_main as main, rt_test as test};`。

- [ ] **Step 5: 验证宏能编译**

```bash
cd /Users/zoujiaqing/projects/Neton/geario && cargo check
```

预期:通过。此时 `geario::util` / `geario::rt` 尚不存在,但宏只在**展开时**才引用它们,未被使用则不报错。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Port ntex-macros runtime attributes as geario-macros"
```

---

## Task 3: `src/bytes/`(拓扑序第 1 位,无内部依赖)

**Files:**
- Create: `geario/src/bytes/`(全部来自 `ntex/ntex-bytes/src/`)
- Modify: `geario/src/lib.rs`
- Modify: `geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::bytes::{Bytes, BytesMut, ByteString, Buf, BufMut, BytePage, BytePageSize, BytePages}` 等。后续每个模块都依赖它。

- [ ] **Step 1: 复制源码,lib.rs 改名 mod.rs**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/bytes
cp -R ../ntex/ntex-bytes/src/. geario/src/bytes/
mv geario/src/bytes/lib.rs geario/src/bytes/mod.rs
```

- [ ] **Step 2: 应用规则 A(内部 `crate::` 提升,38 处)**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
find geario/src/bytes -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::bytes::/g' {} +
```

- [ ] **Step 3: 手工修三处 mod.rs 头部**

原 `ntex-bytes/src/lib.rs` 的 crate 级属性不能留在 `mod.rs` 里:

1. 删除 `#![doc(html_root_url = "https://docs.rs/ntex-bytes/")]` —— 已在 `geario/src/lib.rs` 顶层设置为 `docs.rs/geario/`
2. 把 `#![deny(clippy::pedantic)]` 和 `#![allow(...)]` 改成模块级 `#![...]`(在 `mod.rs` 内是合法的内部属性,保留即可)
3. **`pub use crate::bytes::bytes::Bytes;` 改成 `pub use self::bytes::Bytes;`** —— 规则 A 把原来的 `pub use crate::bytes::Bytes;`(其中 `bytes` 是内部模块)误提升成了三层嵌套

搜索确认第 3 点:

```bash
grep -n 'crate::bytes::bytes::' geario/src/bytes/*.rs
```

把命中的每一处改成 `self::` 形式。

- [ ] **Step 4: 声明模块并加依赖**

`geario/src/lib.rs` 追加:

```rust
pub mod bytes;
```

`geario/Cargo.toml` 的 `[dependencies]` 追加:

```toml
bytes = { workspace = true }
serde = { workspace = true }
log = { workspace = true, optional = true }
simdutf8 = { workspace = true, optional = true }
backtrace = { workspace = true, optional = true }
```

`[features]` 里改:

```toml
simd = ["simdutf8"]
overuse = ["backtrace", "log"]
```

- [ ] **Step 5: 验证**

```bash
cd /Users/zoujiaqing/projects/Neton/geario && cargo check 2>&1 | tail -30
```

预期:通过。若报 `unresolved import crate::bytes::bytes::`,回到 Step 3 补漏。
若报 `inner attribute not permitted`,把 `mod.rs` 顶部的 `#![deny(...)]` 移到 `geario/src/lib.rs`。

- [ ] **Step 6: 单元测试跑通(此模块不含 `#[ntex::test]`)**

```bash
cargo test --lib bytes 2>&1 | tail -20
```

预期:通过。`ntex-bytes` 的单元测试用标准 `#[test]`,不依赖运行时宏。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Port ntex-bytes as the geario bytes module"
```

---

## Task 4: `src/service/`(拓扑序第 2 位,无内部依赖)

**Files:**
- Create: `geario/src/service/`(来自 `ntex/ntex-service/src/`)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::service::{Service, ServiceFactory, Pipeline, PipelineCall, Ctx, Middleware}`,以及 `crate::service::cfg::{Cfg, SharedCfg}`。`io` 与 `net` 的公开 API 依赖 `SharedCfg`。

- [ ] **Step 1: 复制源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/service
cp -R ../ntex/ntex-service/src/. geario/src/service/
mv geario/src/service/lib.rs geario/src/service/mod.rs
```

- [ ] **Step 2: 规则 A(95 处,本次移植中最多的一个)**

```bash
find geario/src/service -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::service::/g' {} +
```

- [ ] **Step 3: 检查是否产生自嵌套**

```bash
grep -rn 'crate::service::service::' geario/src/service/
```

若有命中(`ntex-service` 内若存在名为 `service` 的子模块就会发生),改成 `self::` 形式。预期无命中——`ntex-service/src/` 下没有 `service.rs`。

- [ ] **Step 4: 声明模块与依赖**

`geario/src/lib.rs` 追加 `pub mod service;`

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
log = { workspace = true }
slab = { workspace = true }
foldhash = { workspace = true }
```

注意 `log` 在 Task 3 里被标了 `optional = true`(给 `overuse` feature 用)。此处必须改成非 optional:

```toml
log = { workspace = true }
```

并把 `[features]` 的 `overuse` 改成 `overuse = ["backtrace"]`。

- [ ] **Step 5: 验证**

```bash
cargo check 2>&1 | tail -30 && cargo test --lib service 2>&1 | tail -20
```

预期:两条都通过。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Port ntex-service as the geario service module"
```

---

## Task 5: `src/codec/` 与 `src/error/`(拓扑序第 3、4 位,均只依赖 bytes)

两者都小且互不相关,合为一个 Task。

**Files:**
- Create: `geario/src/codec/`(来自 `ntex/ntex-codec/src/`,101 行)
- Create: `geario/src/error/`(来自 `ntex/ntex-error/src/`,2,029 行)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::codec::{Decoder, Encoder}` —— **稳定 API,阶段二起不得移动或改名**,`geario-http` 将直接实现它们。
- Produces: `crate::error::{Error, ErrorMessage, ErrorMessageChained, fmt_err, fmt_err_string}`。

- [ ] **Step 1: 复制两份源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/codec geario/src/error
cp -R ../ntex/ntex-codec/src/. geario/src/codec/
cp -R ../ntex/ntex-error/src/. geario/src/error/
mv geario/src/codec/lib.rs geario/src/codec/mod.rs
mv geario/src/error/lib.rs geario/src/error/mod.rs
```

- [ ] **Step 2: 规则 A(codec 0 处,error 17 处)**

```bash
find geario/src/error -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::error::/g' {} +
```

`codec` 内部零处 `crate::`,跳过。

- [ ] **Step 3: 规则 B(两者都引用 `ntex_bytes::`)**

```bash
find geario/src/codec geario/src/error -name '*.rs' -exec sed -i '' 's/\bntex_bytes::/crate::bytes::/g' {} +
```

- [ ] **Step 4: 修 error 模块的测试断言字符串**

`ntex-error/src/lib.rs:229,233` 有两处硬编码模块路径:

```bash
grep -n 'ntex_error::tests::test_error' geario/src/error/mod.rs
```

改成 `geario::error::tests::test_error`。这两处是断言字符串,不改会让测试失败——而这正是我们要用来验证移植正确性的测试。

- [ ] **Step 5: 声明模块与依赖**

`geario/src/lib.rs` 追加:

```rust
pub mod codec;
pub mod error;
```

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
thiserror = { workspace = true }
```

(`backtrace`、`foldhash` 已在前面 Task 加过;`backtrace` 需从 optional 改为非 optional,因为 `ntex-error` 无条件依赖它。相应地 `[features]` 的 `overuse` 简化为 `overuse = []`。)

- [ ] **Step 6: 验证**

```bash
cargo check 2>&1 | tail -30 && cargo test --lib codec error 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Port ntex-codec and ntex-error as geario modules"
```

---

## Task 6: `src/rt/`(拓扑序第 5 位,依赖 error)

**Files:**
- Create: `geario/src/rt/`(来自 `ntex/ntex-rt/src/`)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::rt::{System, Builder, SystemRunner, Arbiter, Runtime, RuntimeBuilder, Driver, Runner, BlockFuture, spawn, spawn_blocking, ThreadPool}`。
- `geario-macros` 展开出的 `geario::rt::System` 在此 Task 后可解析。`geario::rt::DefaultRuntime` 要到 Task 9(`net`)才有。

- [ ] **Step 1: 复制源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/rt
cp -R ../ntex/ntex-rt/src/. geario/src/rt/
mv geario/src/rt/lib.rs geario/src/rt/mod.rs
```

**不要**复制 `ntex-rt/build.rs` —— Task 1 已经把它放在 `geario/build.rs` 了。

- [ ] **Step 2: 规则 A(40 处)**

```bash
find geario/src/rt -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::rt::/g' {} +
```

- [ ] **Step 3: 检查自嵌套**

`ntex-rt/src/` 下**有 `rt.rs`**,所以规则 A 一定会产生 `crate::rt::rt::` 这种三层嵌套:

```bash
grep -rn 'crate::rt::rt::' geario/src/rt/
```

把命中处改成 `self::rt::` 或 `crate::rt::rt::`(后者其实是正确的完整路径,但可读性差)。**推荐统一改成 `self::rt::`。**

同理检查 `crate::rt::handle::`、`crate::rt::system::` 等是否指向真实存在的子模块——它们是对的,不用改。

- [ ] **Step 4: 规则 B**

```bash
find geario/src/rt -name '*.rs' -exec sed -i '' 's/\bntex_error::/crate::error::/g' {} +
```

- [ ] **Step 5: 修字符串字面量**

| 文件:行 | 原 | 新 |
|---|---|---|
| `rt/driver.rs:118` | `"ntex/ntex-rt/src/driver.rs"` | `"geario/src/rt/driver.rs"` |
| `rt/builder.rs:12` | doc 注释 `Defaults to "ntex" if unset.` | `Defaults to "geario" if unset.` |
| `rt/builder.rs:34` | `name: "ntex".into()` | `name: "geario".into()` |

行号是移植前的,移植后可能有偏移,用内容搜:

```bash
grep -rn '"ntex' geario/src/rt/
```

- [ ] **Step 6: 声明模块与依赖**

`geario/src/lib.rs` 追加 `pub mod rt;`

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
atomic-waker = { workspace = true }
async-channel = { workspace = true }
async-task = { workspace = true }
crossbeam-queue = { workspace = true }
crossbeam-channel = { workspace = true }
futures-timer = { workspace = true }
libc = { workspace = true }
nix = { workspace = true, features = ["signal"] }
oneshot = { workspace = true }
parking_lot = { workspace = true }
scoped-tls = { workspace = true }
swap-buffer-queue = { workspace = true }
compio-driver = { workspace = true, optional = true }
compio-runtime = { workspace = true, optional = true }
tok-io = { workspace = true, features = ["rt", "net"], optional = true }

[target.'cfg(target_family = "unix")'.dependencies]
signal-hook = { workspace = true }

[target.'cfg(target_family = "windows")'.dependencies]
ctrlc = { workspace = true }
```

`[features]` 更新:

```toml
tokio = ["tok-io"]
compio = ["compio-driver", "compio-runtime"]
```

- [ ] **Step 7: 验证**

```bash
cargo check 2>&1 | tail -30
```

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "Port ntex-rt as the geario rt module"
```

---

## Task 7: `src/util/`(拓扑序第 6 位,依赖 bytes/error/service/rt)

**Files:**
- Create: `geario/src/util/`(来自 `ntex/ntex-util/src/`)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::util::{time::*, channel::*, future::*, services::*, task::LocalWaker, HashMap, HashSet}`,以及本 Task 新增的 `crate::util::enable_test_logging()`。
- `geario-macros` 展开出的 `geario::util::enable_test_logging()` 在此 Task 后可解析。

**本 Task 有一个 spec 未覆盖的新增项**,见 Step 5。

- [ ] **Step 1: 复制源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/util
cp -R ../ntex/ntex-util/src/. geario/src/util/
mv geario/src/util/lib.rs geario/src/util/mod.rs
```

- [ ] **Step 2: 规则 A(65 处)**

```bash
find geario/src/util -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::util::/g' {} +
```

- [ ] **Step 3: 规则 B**

```bash
cd geario/src/util
find . -name '*.rs' -exec sed -i '' \
  -e 's/\bntex_bytes::/crate::bytes::/g' \
  -e 's/\bntex_error::/crate::error::/g' \
  -e 's/\bntex_service::/crate::service::/g' \
  -e 's/\bntex_rt::/crate::rt::/g' {} +
cd /Users/zoujiaqing/projects/Neton/geario
```

- [ ] **Step 4: 处理测试里的 `ntex::` 引用**

`ntex-util` 的测试大量使用 `ntex::rt::spawn`(集中在 `services/buffer.rs`、`services/inflight.rs`、`services/onerequest.rs`、`time/mod.rs`)。这些在 geario 内应指向自身:

```bash
find geario/src/util -name '*.rs' -exec sed -i '' 's/\bntex::rt::/crate::rt::/g' {} +
```

同时把 `#[ntex::test]` 换成 `#[geario::test]`:

```bash
find geario/src/util -name '*.rs' -exec sed -i '' 's/#\[ntex::test\]/#[geario::test]/g' {} +
```

doc 注释里的 `use ntex::time::...` / `#[ntex::main]` 示例逐个改成 `geario`:

```bash
grep -rn 'ntex::' geario/src/util/
```

- [ ] **Step 5: 新增 `enable_test_logging`(spec 漏项)**

`geario-macros` 展开出 `geario::util::enable_test_logging()`,但**该函数在上游定义于 `ntex/src/lib.rs:132` —— 属于我们不移植的上层 crate**。geario 必须自己提供。

在 `geario/src/util/mod.rs` 末尾追加(逐字对应上游实现,只把环境变量名换成 geario 的):

```rust
#[doc(hidden)]
pub fn enable_test_logging() {
    #[cfg(not(feature = "no-test-logging"))]
    if std::env::var("GEARIO_NO_TEST_LOG").is_err() {
        if std::env::var("RUST_LOG").is_err() {
            unsafe {
                std::env::set_var("RUST_LOG", "trace");
            }
        }
        let _ = env_logger::builder().is_test(true).try_init();
    }
}
```

`geario/Cargo.toml` 的 `[features]` 加 `no-test-logging = []`,`[dependencies]` 加 `env_logger = { workspace = true }`。

- [ ] **Step 6: 声明模块与依赖**

`geario/src/lib.rs` 追加 `pub mod util;`

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
bitflags = { workspace = true }
futures-core = { workspace = true }
hashbrown = { workspace = true, features = ["serde"] }
pin-project-lite = { workspace = true }
```

- [ ] **Step 7: 验证 —— 这是宏第一次被真正使用**

```bash
cargo check 2>&1 | tail -30
cargo test --lib util 2>&1 | tail -40
```

预期:`cargo test` 能跑起来并通过。**这一步是整个移植的关键闸门** —— 它同时验证了 `extern crate self as geario`、`geario-macros` 的路径改写、`enable_test_logging`、`geario::rt::System` 四件事。

若报 `cannot find DefaultRuntime in geario::rt`,这是**预期内的**:`DefaultRuntime` 定义在 `ntex-net/src/lib.rs:115`,要到 Task 9 才移植。此时可临时在 `geario/src/rt/mod.rs` 加一个 `pub use crate::net::DefaultRuntime;` 占位是**错误做法**;正确做法是**跳过本 Step 的 `cargo test`,只做 `cargo check`**,把测试验证推迟到 Task 9 之后,并在 commit message 里注明。

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "Port ntex-util as the geario util module

Adds enable_test_logging, which upstream defines in the top-level ntex
crate rather than ntex-util."
```

---

## Task 8: `src/io/` 与 `src/dispatcher/`(拓扑序第 7、8 位)

`dispatcher` 只依赖 `io/codec/util/service`,紧随 `io`,合为一个 Task。

**Files:**
- Create: `geario/src/io/`(来自 `ntex/ntex-io/src/`)
- Create: `geario/src/dispatcher/`(来自 `ntex/ntex-dispatcher/src/`)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::io::{Io, IoRef, IoBoxed, Sealed, Filter, Layer, Base, Framed, IoContext, IoConfig, Decoded, RecvError, IoStatusUpdate, Readiness, IoStream, FilterLayer, Handle, testing::IoTest, types, cfg}`。
- Produces: `crate::dispatcher::{Dispatcher, DispatchItem, Control, Reason, DispatcherError}`。

- [ ] **Step 1: 复制两份源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/io geario/src/dispatcher
cp -R ../ntex/ntex-io/src/. geario/src/io/
cp -R ../ntex/ntex-dispatcher/src/. geario/src/dispatcher/
mv geario/src/io/lib.rs geario/src/io/mod.rs
mv geario/src/dispatcher/lib.rs geario/src/dispatcher/mod.rs
```

- [ ] **Step 2: 规则 A(io 42 处,dispatcher 0 处)**

```bash
find geario/src/io -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::io::/g' {} +
```

- [ ] **Step 3: 检查 io 的自嵌套**

`ntex-io/src/` 下**有 `io.rs`**,规则 A 会产生 `crate::io::io::`:

```bash
grep -rn 'crate::io::io::' geario/src/io/
```

改成 `self::io::`。

- [ ] **Step 4: 规则 B(两个目录都做)**

```bash
cd geario/src
find io dispatcher -name '*.rs' -exec sed -i '' \
  -e 's/\bntex_bytes::/crate::bytes::/g' \
  -e 's/\bntex_codec::/crate::codec::/g' \
  -e 's/\bntex_service::/crate::service::/g' \
  -e 's/\bntex_util::/crate::util::/g' \
  -e 's/\bntex_rt::/crate::rt::/g' \
  -e 's/\bntex_io::/crate::io::/g' {} +
cd /Users/zoujiaqing/projects/Neton/geario
```

- [ ] **Step 5: 处理测试宏与 ntex:: 残留**

```bash
cd geario/src
find io dispatcher -name '*.rs' -exec sed -i '' \
  -e 's/#\[ntex::test\]/#[geario::test]/g' \
  -e 's/\bntex::rt::/crate::rt::/g' {} +
grep -rn 'ntex::' io dispatcher
cd /Users/zoujiaqing/projects/Neton/geario
```

剩余命中(多为 doc 注释)逐个改成 `geario`。

- [ ] **Step 6: 声明模块 + dispatcher 的 io 别名**

`geario/src/lib.rs` 追加:

```rust
pub mod dispatcher;
pub mod io;
```

在 `geario/src/io/mod.rs` 末尾追加别名,还原上游 `ntex::io` 的对外形态(上游 `ntex/src/lib.rs:102-105` 把两者合并在 `io` 命名空间下):

```rust
#[doc(hidden)]
pub use crate::dispatcher::*;
```

这样 `geario::io::Dispatcher` 与 `geario::dispatcher::Dispatcher` 都可用,移植过来的代码与示例一行不改。

- [ ] **Step 7: 依赖**

`geario/Cargo.toml` `[dependencies]` 追加(多数已在前面加过,只补缺的):

```toml
# io 需要，前面未加过的：
# （bitflags / log / pin-project-lite / slab / backtrace 均已存在）
```

核对一遍 `ntex-io/Cargo.toml` 与 `ntex-dispatcher/Cargo.toml` 的依赖是否都已在 `geario/Cargo.toml` 中。

- [ ] **Step 8: 验证**

```bash
cargo check 2>&1 | tail -40
```

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Port ntex-io and ntex-dispatcher as geario modules"
```

---

## Task 9: `src/net/`(拓扑序第 9 位)—— 含切断 ntex-http

**Files:**
- Create: `geario/src/net/`(来自 `ntex/ntex-net/src/`,含 `polling/`、`uring/`、`iocp/`、`compio/`、`tokio/` 五个驱动子目录)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::net::{Reactor, DefaultRuntime, tcp_connect, unix_connect, from_tcp_stream, from_unix_stream, connect::*, channel::*}`。
- **`DefaultRuntime` 在此 Task 后才存在**,`geario-macros` 展开的完整路径到此才闭合。

- [ ] **Step 1: 复制源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/net
cp -R ../ntex/ntex-net/src/. geario/src/net/
mv geario/src/net/lib.rs geario/src/net/mod.rs
```

- [ ] **Step 2: 规则 A(40 处)**

```bash
find geario/src/net -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::net::/g' {} +
```

- [ ] **Step 3: 规则 B**

```bash
find geario/src/net -name '*.rs' -exec sed -i '' \
  -e 's/\bntex_bytes::/crate::bytes::/g' \
  -e 's/\bntex_error::/crate::error::/g' \
  -e 's/\bntex_service::/crate::service::/g' \
  -e 's/\bntex_util::/crate::util::/g' \
  -e 's/\bntex_rt::/crate::rt::/g' \
  -e 's/\bntex_io::/crate::io::/g' \
  -e 's/\bntex_net::/crate::net::/g' {} +
```

**`ntex_io_uring::` 与 `ntex_polling::` 不动** —— 它们是外部 crate。确认:

```bash
grep -rn 'ntex_io_uring\|ntex_polling' geario/src/net/
```

预期:9 处,全部保持原样。

- [ ] **Step 4: 切断 ntex-http(spec 第 4 节)**

`net/connect/uri.rs:1` 是全部 10 个 crate 内唯一使用 `ntex_http` 的地方,而 `ntex_http::Uri` 就是 `pub use http::uri::Uri`:

```bash
sed -i '' 's/^use ntex_http::Uri;/use http::uri::Uri;/' geario/src/net/connect/uri.rs
grep -rn 'ntex_http' geario/src/net/
```

预期:第二条命令无输出。文件其余 57 行不动。

- [ ] **Step 5: 修字符串字面量**

| 位置 | 原 | 新 |
|---|---|---|
| `net/mod.rs`(原 lib.rs:84) | `panic!("not in a ntex driver")` | `panic!("not in a geario driver")` |
| `net/connect/error.rs:45-48,71-80` | `"ntex-connect-InvalidInput"` 等 8 处 | `"geario-connect-*"` |

```bash
grep -rn '"ntex' geario/src/net/
```

逐个改。

- [ ] **Step 6: 处理测试宏**

```bash
find geario/src/net -name '*.rs' -exec sed -i '' \
  -e 's/#\[ntex::test\]/#[geario::test]/g' \
  -e 's/\bntex::rt::/crate::rt::/g' {} +
grep -rn 'ntex::' geario/src/net/
```

- [ ] **Step 7: 声明模块与依赖**

`geario/src/lib.rs` 追加 `pub mod net;`

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
cfg-if = { workspace = true }
http = { workspace = true }
ntex-polling = { workspace = true }
socket2 = { workspace = true, features = ["all"] }
compio-buf = { workspace = true, optional = true }
compio-io = { workspace = true, optional = true }
compio-net = { workspace = true, optional = true }

[target.'cfg(target_os = "linux")'.dependencies]
ntex-io-uring = { workspace = true, features = ["direct-syscall"] }

[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true, features = [
    "Win32_Foundation",
    "Win32_Networking_WinSock",
    "Win32_Security",
    "Win32_Storage_FileSystem",
    "Win32_System_IO",
    "Win32_System_WindowsProgramming",
] }
```

`[features]` 更新:

```toml
tokio = ["tok-io"]
compio = ["compio-net", "compio-driver", "compio-runtime", "compio-buf", "compio-io"]
```

- [ ] **Step 8: 验证 —— 此时 macOS/polling 全链路应打通**

```bash
cargo check 2>&1 | tail -40
cargo test --lib 2>&1 | tail -40
```

**这是第二个关键闸门。** `DefaultRuntime` 到位后,Task 7 推迟的测试验证在此补上。预期 `util`、`io`、`net` 三个模块的 `#[geario::test]` 全部跑通。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Port ntex-net as the geario net module

Drops the ntex-http dependency: connect/uri.rs now uses http::uri::Uri
directly, which is what ntex_http::Uri re-exported."
```

---

## Task 10: `src/server/`(拓扑序第 10 位,最后一个模块)

**Files:**
- Create: `geario/src/server/`(来自 `ntex/ntex-server/src/`)
- Modify: `geario/src/lib.rs`、`geario/Cargo.toml`

**Interfaces:**
- Produces: `crate::server::{net::*, WorkerPool, ServerBuilder}` 等。echo 压测(Task 13)依赖它。

- [ ] **Step 1: 复制源码**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/src/server
cp -R ../ntex/ntex-server/src/. geario/src/server/
mv geario/src/server/lib.rs geario/src/server/mod.rs
```

- [ ] **Step 2: 规则 A(11 处)**

```bash
find geario/src/server -name '*.rs' -exec sed -i '' 's/\bcrate::/crate::server::/g' {} +
```

- [ ] **Step 3: 规则 B**

```bash
find geario/src/server -name '*.rs' -exec sed -i '' \
  -e 's/\bntex_service::/crate::service::/g' \
  -e 's/\bntex_util::/crate::util::/g' \
  -e 's/\bntex_rt::/crate::rt::/g' \
  -e 's/\bntex_io::/crate::io::/g' \
  -e 's/\bntex_net::/crate::net::/g' {} +
```

- [ ] **Step 4: 修字符串字面量**

| 位置 | 原 | 新 |
|---|---|---|
| `server/signals.rs:76,116` | `"ntex-server signals"` | `"geario-server signals"` |
| `server/manager.rs:310` | `"Stopping ntex system, {:?} server"` | `"Stopping geario system, {:?} server"` |
| `server/pool.rs:38` | `name: "ntex".to_string()` | `name: "geario".to_string()` |
| `server/net/accept.rs:79` | `"ntex:accept"` | `"geario:accept"` |

```bash
grep -rn '"ntex' geario/src/server/
```

- [ ] **Step 5: 处理测试宏**

```bash
find geario/src/server -name '*.rs' -exec sed -i '' \
  -e 's/#\[ntex::test\]/#[geario::test]/g' \
  -e 's/\bntex::rt::/crate::rt::/g' {} +
grep -rn 'ntex::' geario/src/server/
```

- [ ] **Step 6: 声明模块与依赖**

`geario/src/lib.rs` 追加 `pub mod server;`

`geario/Cargo.toml` `[dependencies]` 追加:

```toml
core_affinity = { workspace = true }
uuid = { workspace = true }
```

- [ ] **Step 7: 全量验证**

```bash
cargo check 2>&1 | tail -40
cargo test --lib 2>&1 | tail -40
```

**这是第三个关键闸门:全部 10 个模块到齐。** 预期 129 个 `#[geario::test]` 全部通过。

统计实际跑了多少测试:

```bash
cargo test --lib 2>&1 | grep 'test result'
```

- [ ] **Step 8: 验证 ntex 残留清零**

```bash
grep -rn 'ntex' geario/src/ | grep -v 'ntex-polling\|ntex_polling\|ntex-io-uring\|ntex_io_uring\|derived from ntex\|ntex-rs/ntex'
```

预期:无输出。若有命中,逐个处理。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Port ntex-server as the geario server module

All ten upstream crates are now merged; the library builds and the unit
test suite runs on macOS with the polling driver."
```

---

## Task 11: 集成测试

**Files:**
- Create: `geario/tests/test_bytes.rs`(738 行)
- Create: `geario/tests/test_buf.rs`(160 行)
- Create: `geario/tests/test_buf_mut.rs`(64 行)
- Create: `geario/tests/test_bytes_stress.rs`(39 行)
- Create: `geario/tests/test_debug.rs`(35 行)
- Create: `geario/tests/test_iter.rs`(21 行)
- Create: `geario/tests/test_serde.rs`(15 行)

**Interfaces:**
- Consumes: `geario::bytes::*` 的公开 API(从 crate 外部视角)。

保留集成测试身份而非并入 `src/bytes/` 的 `#[cfg(test)] mod`:它们的价值就在于"只能看见公开 API",合并进 `src/` 会让它们看见私有项,削弱测试强度。

- [ ] **Step 1: 复制**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/tests
cp ../ntex/ntex-bytes/tests/*.rs geario/tests/
```

- [ ] **Step 2: 改路径(10 处)**

```bash
sed -i '' 's/\bntex_bytes::/geario::bytes::/g' geario/tests/*.rs
grep -c 'geario::bytes::' geario/tests/*.rs
```

- [ ] **Step 3: 补 dev-dependencies**

`geario/Cargo.toml` 的 `[dev-dependencies]`:

```toml
rand = { workspace = true }
serde_test = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 4: 验证**

```bash
cargo test --tests 2>&1 | tail -30
```

预期:7 个测试二进制全部通过。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Port ntex-bytes integration tests"
```

---

## Task 12: 微基准

**Files:**
- Create: `geario/benches/buf.rs`
- Create: `geario/benches/bytes.rs`
- Modify: `geario/Cargo.toml`

**Interfaces:**
- Produces: 可 `cargo bench` 的两个基准,用于与 ntex 同 commit 对比(验收标准 ±3%)。

- [ ] **Step 1: 复制**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
mkdir -p geario/benches
cp ../ntex/ntex-bytes/benches/*.rs geario/benches/
sed -i '' 's/\bntex_bytes::/geario::bytes::/g' geario/benches/*.rs
```

- [ ] **Step 2: 检查基准框架**

```bash
head -20 geario/benches/bytes.rs
```

上游 `ntex-bytes/Cargo.toml` 没有 `[[bench]]` 段,说明用的是 libtest 的 `#[bench]`(需 nightly)。若确认如此,在 `geario/Cargo.toml` 加:

```toml
[[bench]]
name = "buf"
harness = false

[[bench]]
name = "bytes"
harness = false
```

并把基准改写为 criterion 形式;`[dev-dependencies]` 加 `criterion = { workspace = true }`。

若上游用的是 criterion,直接照搬。**先看清楚再动手,不要假设。**

- [ ] **Step 3: 验证能跑**

```bash
cargo bench --bench bytes 2>&1 | tail -20
```

- [ ] **Step 4: 记录基线**

在 ntex 仓库跑同一组基准,把两边数字写进 `docs/benchmarks/2026-09-XX-micro.md`:

```bash
cd /Users/zoujiaqing/projects/Neton/ntex && cargo bench -p ntex-bytes 2>&1 | tee /tmp/ntex-micro.txt
```

对比,确认差异在 **±3%** 以内。超出则回头查移植错误。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Port ntex-bytes microbenchmarks and record the baseline"
```

---

## Task 13: echo 示例与对标压测

**Files:**
- Create: `geario/examples/echo.rs`
- Create: `bench-echo/server-ntex/{Cargo.toml,src/main.rs}`
- Create: `bench-echo/server-geario/{Cargo.toml,src/main.rs}`
- Create: `bench-echo/client/{Cargo.toml,src/main.rs}`
- Create: `bench-echo/README.md`

**Interfaces:**
- Consumes: `geario::server::*`、`geario::io::*`、`geario::codec::*`。
- Produces: 验收标准第 4 条所需的端到端数字。

`bench-echo/` **不属于 geario workspace** —— 否则 `server-ntex` 会把上游 ntex 拖进主依赖树,两套 `Io` 类型同时存在。

- [ ] **Step 1: 移植 echo 示例**

以 `ntex/examples/echo.rs` 为蓝本(它依赖上层 `ntex` crate 的 re-export,需改成 geario 的模块路径):

```bash
cat /Users/zoujiaqing/projects/Neton/ntex/ntex/examples/echo.rs
```

按实际内容改写 import,放到 `geario/examples/echo.rs`。验证:

```bash
cargo build --example echo && cargo run --example echo
```

另开终端 `nc 127.0.0.1 <port>` 验证回显。

- [ ] **Step 2: 建 bench-echo 三个 crate**

根 `Cargo.toml` 的 `[workspace]` 加:

```toml
exclude = ["bench-echo"]
```

三个 crate 各自独立 `Cargo.toml`。`server-ntex` 依赖:

```toml
ntex = { git = "https://github.com/ntex-rs/ntex", rev = "48eef5bd" }
```

`server-geario` 依赖:

```toml
geario = { path = "../../geario" }
```

两份 `main.rs` 除 import 路径外**逐字相同**。

- [ ] **Step 3: 写压测客户端**

固定连接数、固定 payload、固定时长,输出 QPS 与 p50/p99 延迟。用 `std::net::TcpStream` + 线程池即可,不引入额外运行时以免影响测量。

- [ ] **Step 4: 跑对比**

```bash
cd bench-echo
# 终端 A
cargo run --release -p server-ntex
# 终端 B
cargo run --release -p client -- --target 127.0.0.1:8080 --conns 64 --duration 30
# 换 server-geario 重跑
```

- [ ] **Step 5: 记录结果**

写进 `docs/benchmarks/2026-09-XX-echo-macos.md`,含:机器型号、rustc 版本、驱动(polling)、连接数、payload、三轮取中位数。

**验收:差异 ±3% 以内。** 超出则说明移植有误,回头排查。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Add echo example and the ntex-vs-geario echo benchmark harness"
```

---

## Task 14: 批次一验收收尾

**Files:**
- Create: `docs/benchmarks/` 下的结果文件(Task 12、13 已写)
- Modify: `README.md`

- [ ] **Step 1: 逐条核对 spec 第 6 节批次一的五条**

```bash
cd /Users/zoujiaqing/projects/Neton/geario
# 1. build
cargo build 2>&1 | tail -5
# 2. test
cargo test 2>&1 | grep 'test result'
# 5. ntex 残留
grep -rn 'ntex' geario/src/ | grep -v 'ntex-polling\|ntex_polling\|ntex-io-uring\|ntex_io_uring'
```

第 3、4 条(微基准、echo 压测 ±3%)已在 Task 12、13 完成。

- [ ] **Step 2: 确认 129 个测试都在**

```bash
grep -rc '#\[geario::test\]' geario/src/ | awk -F: '{s+=$2} END {print s}'
```

预期:129。少于 129 说明有测试在移植中丢失。

- [ ] **Step 3: 补 README 的当前状态**

写明:批次一(macOS/polling)已验收,批次二(Linux/polling+uring)待服务器到位。

- [ ] **Step 4: Commit 并打 tag**

```bash
git add -A
git commit -m "Complete phase-1 batch-one acceptance on macOS"
git tag phase1-batch1
```

---

## Task 15: 批次二验收(Linux 服务器到位后)

**前置条件:** 一台 Linux 机器。此 Task 在服务器可用前**不执行**。

- [ ] **Step 1: polling 驱动**

```bash
cargo build --features neon-polling
cargo test --features neon-polling 2>&1 | grep 'test result'
```

- [ ] **Step 2: uring 驱动**

```bash
cargo build --features neon-uring
cargo test --features neon-uring 2>&1 | grep 'test result'
```

- [ ] **Step 3: 两个驱动各跑一遍微基准与 echo 压测**

对照 ntex@`48eef5bd`,同机同参数,±3%。

- [ ] **Step 4: 记录并 tag**

```bash
git add -A
git commit -m "Complete phase-1 batch-two acceptance on Linux"
git tag phase1-complete
```

---

## 自查记录

**Spec 覆盖核对:**

| spec 节 | 对应 Task |
|---|---|
| 2 fork 点 / License 归属 | Task 1 |
| 3 仓库结构 / 工具链 | Task 0、1 |
| 3 build.rs | Task 1 Step 5 |
| 3 geario-macros | Task 2 |
| 4 模块映射(10 个) | Task 3-10 |
| 4 dispatcher 顶层 + io 别名 | Task 8 Step 6 |
| 4 src/bytes 嵌套 | Task 3 Step 3 |
| 4 切断 ntex-http | Task 9 Step 4 |
| 4 Feature 表 | Task 1、6、9 累积 |
| 5 层 1a(348 处) | 各 Task 的"规则 A" |
| 5 层 1b(378 处) | 各 Task 的"规则 B" |
| 5 层二(字符串) | Task 5、6、9、10 |
| 5 层三(测试) | Task 7-11 |
| 5 层四(lint) | Task 1 Step 3、6 |
| 6 批次一验收 | Task 14 |
| 6 批次二验收 | Task 15 |
| 7 双轨 benchmark | Task 12、13 |

**spec 未覆盖、本计划新增的两项:**

1. **Task 0 工具链** —— 本机 rustc 1.92.0 低于 ntex 要求的 1.95,是硬阻塞。spec 写作时未检查。
2. **Task 7 Step 5 `enable_test_logging`** —— 该函数定义在 `ntex/src/lib.rs:132`,属于不移植的上层 crate,但 `geario-macros` 展开时会引用它。geario 必须自己提供。

**风险提示:**

- Task 7 的 `cargo test` 可能因 `DefaultRuntime` 未到位而无法运行,已在该 Step 写明处理方式(推迟到 Task 9)。
- 规则 A 在含同名子模块的三个 crate(`bytes`/`rt`/`io`)会产生三层嵌套,已在 Task 3、6、8 各自的检查 Step 中列出。
