# geario 阶段二设计:架构精简与逻辑瘦身

- 日期:2026-09-09
- 状态:待实现
- 范围:geario + geario-http(+ geario-http-ffi 作为消费面校验对象)
- 前序:2026-09-04(阶段一移植)、2026-09-05(geario-http)、2026-09-06(ffi)

## 1. 目标与等式

阶段一把 ntex 10 个 crate、约 42k 行整体移植成单包 geario,逐字保留逻辑。
阶段二在**不牺牲 geario-http 能力**的前提下,删掉移植带来的死代码、拍平为
ntex-web 服务的过度抽象、让运行逻辑更直、架构更贴近"thread-per-core + !Send/Rc"
的真实形态。

**等式要写准:精简是手段;更快、更稳定是必须独立验证的结果。代码变少不自动等于性能更强。**

- **纯清理**:允许性能"无可测退步",但必须证明维护性 / 编译时间 / 二进制体积的收益。
- **性能优化(重构)**:必须用基准证明性能收益,否则回滚。

## 2. 硬约束(护栏)

不可协商的红线,每个任务隐含遵守:

- **必须保住 geario-http 的 HTTP/1.1、HTTP/2、未来 HTTP/3。** h1/h2 的行为验收项见 §2.1。
  - TLS filter 分层:**必须保住行为与性能,但不冻结现有实现结构**——允许独立实验、
    验证通过后替换。rustls 的 ALPN(`geario/src/tls/rustls/stream.rs`)是 h1/h2
    协商入口(`ALPN_PROTOS = ["h2","http/1.1"]`),明文 TCP 可加快路径。
  - `Pipeline` / `Dispatcher`:h2 多路复用载体,行为不得回退,结构可在验证后替换。
- **h3 的正确姿态**:留出**合适的传输扩展边界**,而不是提前搭空架构。QUIC 用数据报,
  还涉及定时器、流控、取消、以及与 TCP+TLS 不同的集成方式。当前精简**只需避免把所有
  传输硬编码成 TCP 字节流**;具体抽象由后续 QUIC 原型验证——留口是基础条件,不等于
  已保证 h3 能力。
- **三平台都要绿。** Linux + macOS + Windows CI 全绿(含 Windows 运行期测试)为准。
- **热路径**:见 §6。要求是**保住行为与性能**,不是冻结实现——允许实验后替换。
- **每步独立验收。** 见 §1 的两类等式 + §8 的分层验收。
- **删除资格**:见 §5.1 的多重门,零引用只是候选筛选的第一步。
- **提交信息英文,源码内不留任何 AI/衍生痕迹**(归属只在 NOTICE/README)。

### 2.1 h1/h2 行为验收项(重构不得回退)

任何触及 io/service/dispatcher/tls 的重构,合入前须证明以下行为不变:

- **h1**:keep-alive 连接复用、请求流水线(pipelining)、响应顺序
- **h2**:单连接多流(multiplexing)、流控(window)、单流取消(RST_STREAM)
- **背压**:读写限速 / inflight 上限
- **断连**:对端 RST/EOF、半关闭
- **TLS / ALPN**:h1↔h2 协商正确
- **优雅关闭**:drain、in-flight 请求收尾

对应测试:每项在触及相关代码的任务里**逐步关联到具体测试名**(而非笼统写
"现有测试覆盖");缺失的场景先补用例再改代码。

## 3. 基线现状

geario 单包约 43,328 行。按目录:

| 目录 | 行数 |
|---|---|
| bytes | 8,065 |
| util | 6,531 |
| net | 6,501 |
| service | 6,134 |
| io | 5,721 |
| server | 3,294 |
| rt | 2,835 |
| error | 2,035 |
| dispatcher | 1,385 |
| tls | 702 |
| codec | 101 |

## 4. 消费面分析(精简依据)

真实消费者是 `server/`、`dispatcher/`、以及 geario-http 的 raw h1 `HttpService`。
它们只用了 service 层一薄片:`Service`、`Pipeline`/`PipelineCall`、`Ctx`、
`chain::{service,map,map_err}`、`SharedCfg`;server 甚至不走 `ServiceFactory`,
而是 `AsyncFn + IntoService + Pipeline` 自己装箱。

**阶段一子代理的关键误判(已交叉核对修正):**

| 曾被标"死" | 真相 | 依据 |
|---|---|---|
| iocp(1,444) | **真驱动,但已腐化** | 见 §5.0,已修复 |
| `service::{Middleware,Stack,apply_fn}` | geario-http 客户端连接器在用 | `geario-http/src/client/builder.rs:6,40` |
| `util::services::Extensions` | geario-http 在用 | `geario-http` 4 处 `use` |
| `Timeout`/`KeepAlive` | 待定:需分清是 geario 的还是 geario-http 自有类型 | geario-http 27/62 处引用 |
| `DriverType` | iocp 用了 `DriverType::Iocp` | `geario/src/net/iocp/reactor.rs:81-82` |

教训:**内部无引用 ≠ 可删**。删除资格见 §5.1 的多重门。

## 5. 分层方案

### 5.0 第 0 层:三平台 CI 护栏 —— 已完成

状态:**真实 CI(含 Windows runner,MSVC 工具链)三平台全绿,已核实。**

- geario run:https://github.com/zoujiaqing/geario/actions/runs/34360748184
- geario-http run:https://github.com/zoujiaqing/geario-http/actions/runs/34358990902

注意:CI 绿只证明"iocp 后端在当前用例上能编能跑",**不等于"iocp 已无问题"**。
它此前从未在 CI 跑过,首跑就逼出了三个潜伏问题(见下),说明覆盖仍浅。

没有 Windows CI 是 iocp 腐化的直接原因。本层补护栏,后续所有删改才有三平台守门。

- **iocp 编译修复**:`geario/src/net/iocp/reactor.rs` 的 `syscall` 宏
  从 `crate::rt::{...,syscall}` 改为 `use crate::syscall;`(与 uring/polling 一致)。
  第二个"未用导入"错误是该宏未解析的连锁,修好即消。
- **Windows CI**:
  - geario `test` 矩阵加 `windows-latest`(**实跑 `cargo test`,非仅编译**);
    新增 `features-windows` job 跑 `rustls / neon-iocp / rustls,neon-iocp` 的
    all-targets check。
  - geario-http 新增 `test-windows` job,单测 `geario-http` 本 crate
    (`full,rustls,hyper-full`),避开 unix-only 的 bench 工作区成员。
- **首跑逼出并修复的 3 个问题(均非 iocp 驱动缺陷)**:
  1. `iocp/reactor.rs` 编译腐化(上条)——真 bug,无 CI 才没被发现。
  2. `util::time::wheel` 的 `test_timer`:下界(不提前触发)**改为所有平台都查**;
     只有上界在粗时钟平台放宽 slack。**遗留**:确定性时间轮逻辑测试(可注入时钟
     验证到期/顺序/取消/重置)需要 wheel 暴露可控时钟,属单独重构,记为后续项。
  3. `tests/direct_write.rs`:就绪式 + io_uring 驱动**保留严格断言**(能抓直写回归);
     iocp 尚未实现可选的 `write_bufs` 能力,测试验证其"干净拒绝(恰好 0)+ 缓冲路径
     正确送达"。修正了此前"完成式驱动做不到直写"的错误结论(io_uring 同为完成式却已实现)。
- **遗留提升项**:上面 2 的确定性时间轮测试;必要时给 h1/h2 行为项(§2.1)补齐场景用例。

**本层验收:达成**(三平台 CI 全绿)。

### 5.1 删除资格(所有删除类任务的统一门)

**决策(2026-09-10,用户确认):geario 是为 geario-http 而 fork 的底座、非对外发布的库,
公开 API 面收缩到 geario-http + ffi 实际消费范围。** 未被消费者用到的 pub API 可删,
但删除仍属"缩小公开面",逐项立项、逐项验证、单独提交。

零引用只是**候选筛选**;grep 计数会因排除模式不同而误导(见下)。**终极门是编译器 +
测试套件**:删除后 `geario`(all-targets,各 feature 组合)+ `geario-http`(full,rustls,
hyper-full)+ `geario-http-ffi` 必须全部编过 + 测试通过 + 基准无回退。候选还须逐条排除:

1. **跨 crate 引用**:`geario` + `geario-http` + `geario-http-ffi` 三处无非测试引用。
2. **消费者未用**:geario-http + ffi 均不消费(现按决策可删;若消费则载重,保留)。
3. **trait 实现**:不是某个被用到的 trait 的 impl(impl 可能通过 trait 对象间接可达)。
4. **宏生成**:不是宏展开产物、也不被宏引用。
5. **平台 / feature 分支**:平台代码(`cfg(windows)` 等)与 pub feature(tokio/compio)
   单独立项;不与内部删除混批。
6. **示例 / 外部 ABI 承诺**:不被 examples、ffi C 头、或对外 ABI 依赖。

**阶段一子代理的死代码清单不可靠,必须逐项用上述门 + 编译器复核。** 已证伪的误判:
STEXT(`From<Arc<str>>` 活用 + pub trait impl)、`pl_factory::PipelineFactory`
(geario-http `HttpPipeline` 核心类型)、`channel::{bstream,inplace,mpsc}`(geario-http 用)、
`map_init_err`(geario-http `service.rs` 用)、`state.rs::RequestState`(16 处引用)——
全部载重,不可删。

### 5.2 第 1 层:纯死代码删除(低风险)

收益是体积/编译/可维护性(§1 纯清理等式)。每项须先过 §5.1 全部门。

| 候选 | 约 LOC | 位置/依据 | 备注 |
|---|---|---|---|
| tokio 运行时后端 | ~745 | `net/tokio/*`;`Reactor::run` panic,opt-in feature 无消费者 | 删 feature,单独立项(公开 feature 面) |
| compio 运行时后端 | ~610 | `net/compio/*`;同上 | 同上 |
| bytes KIND_STEXT 外部存储 vtable | ~250 | `bytes/stext.rs`、`stext_arc.rs` + `storage.rs` 对应臂;`from_ext` 零调用 | 内部,优先 |
| service Tier A | ~640 | `map_config.rs`(157)+`unit_config`、`then.rs`(200)、`map_state.rs`(49)+`state.rs`(21)、`pl_factory.rs`(102)、`map_init_err.rs`(107) | 逐个核对是否 pub 导出 |
| util 孤儿/死代码 | ~2,980 | `future/ready.rs`(未被编译的孤儿)、channel 的 mpsc/bstream/inplace、future/on_drop 等 | 逐项核对 |
| bytes hex + 宽数值 Buf/BufMut | ~200+ | `{:x}` 零使用;i128/f64/uint 等仅 bench 用 | 移到 nightly/bench 门,不直接删 |
| io/bytes 死 helper | 小 | `BufConfig::buf_with_capacity`、`TimerHandle::instant/remains`、`with_read_src_buf` 等 | 内部 |

**验收(纯清理)**:三平台 CI 全绿 + 基准无可测退步;分批提交。

### 5.3 第 2 层:条件删除(需跨 crate 核对 + 产品决定)

经核对**部分被 geario-http 客户端使用**,不能直接删。逐项定性:

- `service/middleware.rs`(418)、`apply.rs`(346):geario-http 客户端在用
  `apply_fn`/`Middleware`/`Stack`(`client/builder.rs`)→ **保留**。
- `util::services::Extensions`:geario-http 在用 → **保留**。
- `Timeout`/`KeepAlive`:先判定 geario-http 用的是 geario 的还是自有类型;若自有,
  geario 侧 `util/services/{timeout,keepalive}` 可删,否则保留。
- `pl_state.rs`(429):仅 `util/services/buffer.rs` 用;若 buffer 确认无外部消费者,连带删。

### 5.4 第 3 层:过度抽象重构(高价值 / 高风险 / 必须过基准且独立立项)

唯一可能带来提速的一层。**每项单独立项:先测成本,再改结构,基准证明收益方合入。**

1. **cfg.rs 的 Configuration/SharedCfg(528)——先画所有权与跨线程边界,再决定改法。**
   - 现状:`server.bind` 的工厂是 `F: AsyncFn(&Cfg::State) -> I + Send + Clone + 'static`
     (`geario/src/server/net/builder.rs:217`),配置经 accept 进入 **worker 线程**——
     **存在真实的 Send 边界,不能整体换成 Rc。**
   - 成本点在**每次 accept 连接**:`io.convert(cfg.clone())`(Arc 克隆)→ `Io::new` 里
     `get::<IoConfig>()` 做 `mem::forget(clone)`(原子增)+ HashMap 查 TypeId + downcast +
     指针打标;`Cfg::drop` 再 `Arc::decrement_strong_count`。
   - **候选改法(待验证)**:"跨线程共享配置(Arc,交给每个 worker)+ worker 内本地解析
     配置(Rc/按值)"的两层结构,去掉 per-accept 的 TypeId HashMap 与原子 churn。
     **前提**:先确认 accept→worker→connection 的实际传递路径,再定哪部分能降为 Rc。
   - 必须保住 TlsConfig 通道(h2 依赖)。风险:穿透 net/io/tls/server 约 10 个文件签名。
2. **Pipeline 类型擦除的 per-request 成本——独立立项,先测再改。**
   - 每帧:1 个 `Box::pin`(`pl_inner.rs:121`)+ `Rc<dyn PipelineInternalApi>` 虚表
     + slab reg/unreg(`pipeline.rs:130,199`);流水线响应再多一个 boxed spawn。
   - 候选:让 `Dispatcher` 对具体 `Service` 单态化,消除 `Rc<dyn>` 擦除。
   - 保住 h2 多路复用语义(§2.1)。风险高(`BoxFuture` transmute 的生命周期擦除是核心)。
3. **io 永远动态的 filter 链**。零 filter 明文连接也付 boxed `Base` + 每次读写虚表 +
   多分配一个用不到的第二层写缓冲。候选:为透明连接加编译期快路径;**TLS/h2 分层路径
   行为不变**(结构可验证后替换)。
4. **bytes Storage 4 后端 → 2 后端**。生产只用 KIND_VEC + KIND_STATIC;删 KIND_STEXT
   后评估再去 KIND_INLINE。风险:KIND_INLINE 是小缓冲微优化,须基准确认不劣化。
5. **WaitersRef 多属主就绪仲裁(ctx.rs 122)**。dispatcher 单属主、server 每连接一个
   `Pipeline`,多属主 waker 仲裁 + per-call slab churn 基本浪费。候选:单属主快路径。
   风险高(核心正确性)。

**验收(性能优化)**:逐项旋转配对基准(bootstrap 95% CI、每轮数据、二进制 sha256、
错误计数)+ §2.1 行为项全过;明确提升方合入,否则回滚并记录证据。

## 6. 热路径(保住行为与性能,不冻结实现)

以下已验证为收益来源或热点。要求是**行为与性能不回退**;允许独立实验、验证后替换实现,
但不得以"精简"名义顺手改坏:

- 分叉的 poller(`geario-polling` 的 epoll 后端)
- io_uring 向量化发送:`net/uring/stream.rs` 的 `Operation::Writev`、`send()`
- `consolidate_write_state`、`try_write_vectored`
- 分页写缓冲:`bytes/pages.rs`、`bytes/stvec.rs`
- `dispatcher/mod.rs` 的 keepalive/读速率/背压/优雅关闭状态机(h1/h2 协议运行时)

## 7. 保留(非死代码 / 产品决定)

- **iocp**(1,444):三平台标配,已修复 + 补 CI,保留。
- **dispatcher**(1,385):可复用协议运行时,geario-http h1 建其上,保留。
- **time 时间轮**(约 1,680):io 超时后端,载重;"改用 reactor 原生定时器"属重设计,不在本阶段。

## 8. 执行顺序

1. **第 0 层**:iocp 修复 + 三平台 CI 已配置 → **等 Windows runner(MSVC)运行验收**。
2. **第 1 层**:先删满足 §5.1 全部门的**非公开、确实不可达**内部代码;公开 API 与平台
   分支单独立项。分批提交,每批过三平台 CI + 基准无退步。
3. **第 2 层**:逐项附跨 crate 证据后定性删/留。
4. **第 3 层**:每项单独立项——cfg per-accept、Pipeline per-request 先测成本再改结构,
   优先做;再按风险做 filter 快路径、Storage 后端、WaitersRef。

## 9. 风险与回滚

- 每层/每项独立提交,可单独 revert。
- 第 3 层每项合入前须有基准证据文档(`geario/docs/benchmarks/` 或
  `geario-http/docs/benchmarks/`)+ §2.1 行为项通过,达不到即回滚。
- Windows 运行期风险:iocp 从未在 CI 跑过,首推可能暴露运行期问题,fix-forward。
- 压测服务器 106.55.63.153 的 `kernel.io_uring_disabled=2` 在 io_uring 实验收尾后需还原。
