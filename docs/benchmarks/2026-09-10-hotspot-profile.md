# 热点采样(2026-09-10,探索性)

- 主机:106.55.63.153(Tencent 云 VM,kernel 5.14)。**注意**:此 VM 无硬件 PMU
  (`perf` 默认 cycles 事件采到 0 样本),改用软件事件 `perf record -e task-clock`。
- 负载:`geario-after`(清理后),WORKERS=1,CONNS=64,io_uring,闭环饱和。
- 二进制未 strip,符号可解析(Rust v0 mangling,c++filt 不解,手读路径)。

## DSO 层面(CPU 时间归属)

| 负载 | 内核 | geario-after 用户态 | nf_tables(防火墙) | libc |
|---|---|---|---|---|
| 16KB io_uring | 85.85% | 12.93% | (计入 kernel,nft_do_chain 3.63%) | 1.22% |
| 128B io_uring | 80.08% | 14.16% | 4.62% | 0.90% |

内核占大头:`_raw_spin_unlock_irqrestore` ~20%、`rep_movs_alternative`(memcpy)~14%、
`nft_do_chain`(nftables,**benchmark 环境的 loopback 防火墙,与 geario 无关**)3.6–4.6%、
tcp_clean_rtx_queue / net_rx_action。

## geario 用户态 top 函数(占比小、摊薄)

128B io_uring(用户态共 ~14%):
- 1.44% `geario_http::h1::codec` Decoder::decode(请求解析)
- 0.89% `geario_http::h1::dispatcher` poll_read_request
- 0.88% `geario::io::ioref::IoRef::encode`(响应编码)
- 0.86% `geario::service::chain::ServiceChain<...>::call`(服务链调用)
- 0.63% `geario::io::buf::Stack::write_buf_size`

16KB io_uring 类似,codec decode 1.14% 居首,其余 < 1%。

## 结论(直接影响下一步优化)

1. **CPU 由内核 loopback 网络 + 数据拷贝主导(80–86%)**;geario 用户态仅 13–14%,
   且无单一热点(最大 ~1.4%)。geario 用户态已相当精简。
2. **预设的 Tier-3 候选不是热点**:`cfg`/`SharedCfg`(per-accept,keep-alive 下摊薄,榜上无名)、
   `Pipeline`/service-chain per-request(仅 0.86%)、`WaitersRef`(< 0.5%)。按"先测成本再改",
   **这些重构的潜在收益 < 1% 用户态 ≈ < 0.15% 整体,不值高风险改动**(与 GPT"不预定重写"一致)。
3. **若要在用户态找收益**,最大的片是 h1 **codec 编解码**(decode+encode ≈ 2.3% @128B);
   但同样很小,需先确认可优化空间与安全性。
4. **测量环境噪声**:`nft_do_chain` 3.6–4.6% 是 loopback 防火墙,纯环境开销,建议基准时
   为回环放行以降噪(不改 geario);此 VM 无 vPMU、共享负载,是之前 CI 宽的根源之一。

**建议**:不基于当前数据启动 cfg/Pipeline 重构(收益太小)。若继续冲性能,应先在**真实网卡 +
多核**环境用 task-clock profiling 复核热点(loopback 掩盖了真实 NIC 路径),再据实定目标。
