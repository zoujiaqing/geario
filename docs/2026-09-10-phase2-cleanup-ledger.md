# 阶段二清理台账(第 1 层)

- 日期:2026-09-10
- 范围:geario 公开面收缩到 geario-http + ffi 实际消费范围(用户 2026-09-10 确认)
- 配套 spec:`docs/superpowers/specs/2026-09-09-geario-simplification-design.md`

## 状态口径(重要)

**当前口径:功能检查通过;性能验收未完成。** 三平台 CI 的既有检查通过,功能层面 OK。
性能侧已跑了一轮聚焦同机配对基准(见下),但只有 16KB 达到预先设定的非劣效门槛,
64KB 尚未确立;polling、小响应、CPU/请求、p99、RSS、ntex4 对照都还没做。因此**不能**
说"零性能回归",准确说法是"功能检查通过,性能验收未完成"。

## 已删除的公开 API 清单

| 提交 | 删除项(公开) |
|---|---|
| 7d88b0d | `service::map_config` / `unit_config` / `MapConfig` / `UnitConfig`;孤儿 `util/future/ready.rs`(未编译) |
| 02873b5 | `service::map_state` / `MapState`(free fn + 类型) |
| 15230bd | `ServiceChain::then` / `ServiceChainFactory::then` 方法;`service::{Then, ThenFactory}` |
| 4b18c15 | `util::services::{Buffer, Either(service), KeepAlive, OneRequest, Retry, Timeout, Variant}` 及各模块 |
| d115cca | `bytes` 的 `LowerHex`/`UpperHex`(Bytes/BytesMut)+ `BytesRef` |
| 3b18287 | `IoRef::with_read_src_buf`、`BufConfig::buf_with_capacity`、`TimerHandle::remains`、`TimerHandle::instant` |
| 26410ad | 公开 feature `tokio`、`compio`;模块 `net::tokio`、`net::compio`、`rt::rt_tokio`、`rt::rt_compio`;`SystemRunner::run_local`;6 个依赖(tok-io、compio-buf/io/net/driver/runtime) |

保留(agent 误判为死、实为载重):STEXT(`From<Arc<str>>`)、`pl_factory::PipelineFactory`
(geario-http HttpPipeline)、`pl_state::PipelineState`(geario-http client)、`channel::{bstream,
inplace,mpsc}`、`map_init_err`、`state.rs::RequestState`、`counter`+`inflight`、`Extensions`、
`middleware`/`apply`。

删除资格终极门:`geario`(all-targets,各 feature 组合)+ `geario-http`(full,rustls,
hyper-full)+ `geario-http-ffi` 全部编过 + geario 测试通过 + 三平台 CI 绿。

## 已知测试缺口

1. **`tests/tls_rustls.rs` 的 `a_client_with_no_common_alpn_protocol_is_told_so` 在 macOS 上
   `#[ignore]`。** 这是**暂时隔离,不是修好**。
   - 复现条件:加载重的 macOS runner,loopback 上握手 teardown 与告警投递竞态;客户端/服务端
     任一方可能先看到 reset/broken-pipe,失败表现为传输错误而非 rustls 告警。Linux/Windows/
     安静的 macOS 主机上稳定通过。
   - 恢复标准:能**稳定复现**该竞态后,做确定性修复(候选:确保告警在有序关闭前送达并被对端读取),
     然后去掉 macOS 的 `#[ignore]`,在 macOS CI 上连跑多次不 flake 方算恢复。
   - 曾尝试两次库级修复(服务端优雅关闭、客户端 flush 失败后 drain 读),本地过但 macOS CI 仍挂,
     已回退(见提交 2476691)。

2. **`tests/direct_write.rs` 已修**(提交见本次):echo 断言改为累计到预期长度 + 5s 超时,
   避免把 TCP 分段误判为驱动错误;两个用例一致处理。

## 基准验收计划(下一步,证明清理未损害既有优势)

固定"清理前 / 清理后"两个提交,**同机配对**测:

| 维度 | 要求 |
|---|---|
| 响应尺寸 | 小响应(128B)+ 16KB / 32KB / 64KB |
| 驱动 | polling 与 io_uring 分别跑 |
| 对照 | 固定 SHA 的 ntex 4 neon(见 server-ntex4) |
| 指标 | 吞吐、CPU/请求、p99、RSS |
| p99 比较 | **相同请求速率**下比 p99;最大吞吐单独测 |
| 方法 | 旋转配对 A/B、bootstrap 95% CI、每轮数据、二进制 sha256、错误计数 |

清理前提交(基线):第 1 层起点前一次;清理后提交:第 1 层末次。服务器:106.55.63.153 /
120.76.243.169。**重点是证明向量化发送的收益在清理后仍在。**

## h2 回归(与 h1 跑分并重)

补 HTTP/2 的:多流(multiplexing)、流控(window)、请求/流取消、慢消费者(slow consumer /
背压)回归检查,防止只照顾 h1。参见 spec §2.1。

## 之后的优化(每次一个,基于真实热点)

按 CPU / 尾延迟数据挑热点,一次一个实验、独立基准验收。**不预设** serde 删除或 SharedCfg /
Pipeline 必须重构 —— 先量它们在目标负载(尤其长 keep-alive)下每连接 vs 每请求的实际成本。

## 基准验收(2026-09-10,聚焦子集,性能验收未完成)

**验收标准(先于看结果设定):非劣效,门槛 3%。** 对每个负载,只有配对比值 B/A 的
95% bootstrap CI **下界 > 0.97** 才算"非劣效通过";CI 仅仅跨过 1.0 **不算通过**——
在这样的 CI 宽度下,跨 1 只说明"没检出变化",仍不能排除实质回退。

同机配对 A/B,清理前 `588c0d3`(A=geario-before)vs 清理后 `2378a15`(B=geario-after),
两个二进制均 macOS 交叉编译到 x86_64-unknown-linux-gnu,`--features uring`。
服务器 106.55.63.153(kernel 5.14,io_uring 开),CONNS=64,WORKERS=1,SECS=5,
旋转配对,bootstrap 10k 95%CI,errors+mismatches 全 0。

二进制 sha256(辅助记录,**不用来推断机器码相同**;大小接近不能证明布局/内联/依赖未变):
- before `f538e85e7603edf6a944c46fa3ab605b3aaef7d21c9dabe4b92fe31d8393fbb6`
- after  `09d0b622d2834e7bb3b78b70303dcf5dd0b7242d72a87daad5aa795220ee4d77`

| 尺寸 | 轮数 | B/A 均值 | 95%CI | 下界>0.97? | 判定 |
|---|---|---|---|---|---|
| 16KB | 12 | 1.056 | [0.979, 1.153] | 是 | **非劣效通过(3%)** |
| 64KB(批1) | 12 | 0.910 | [0.818, 1.007] | 否 | 未确立 |
| 64KB(批2) | 20 | 0.997 | [0.940, 1.060] | 否 | 未确立 |

**两批 64KB 都保留,不做取舍。** 批1 与批2 的点估计不同(0.910 vs 0.997)只说明两批结果
不一致,不能把批1 解释为"运气差";也不能一直重跑到得到"打平"。目前 64KB 两批 CI 下界
(0.818 / 0.940)都低于 0.97,**排除不了约 6% 的下降**,故 64KB 非劣效**未确立**。

原始逐轮数据与分析脚本随本提交存档:
- `docs/benchmarks/2026-09-10-cleanup-ab-16k-64k-r12-raw.txt`
- `docs/benchmarks/2026-09-10-cleanup-ab-64k-r20-raw.txt`
- `docs/benchmarks/2026-09-10-cleanup-ab-analyze.py`(复算:`python3 analyze.py <raw...>`)

**结论:16KB 达到 3% 非劣效;64KB 未确立;性能验收未完成。** 下一步收紧 64KB(更多轮、
CPU 绑定/降噪、检查负载与频率后再联合分析),并补齐 128B、polling、CPU/请求、
相同请求速率下的 p99、RSS,以及对固定 SHA ntex4 的对照(确认向量化发送的领先仍在)。
