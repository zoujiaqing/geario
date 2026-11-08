# Linux TCP echo comparison

Run **Linux runtime comparison** from GitHub Actions. It compares geario's
forced polling and io_uring builds against Tokio current-thread, with one
server worker and four persistent clients. The server and load generator are
pinned to separate logical CPUs. All variants use the same client and receive
a fresh process and warmup per sample.

The artifact contains the six paired rounds at 128 bytes and 16 KiB, process
CPU per completed echo, closed-loop p99, binary hashes and runner information.
Geario uses its normal accept loop and worker dispatch. Tokio accepts on its
runtime thread. This measures the complete TCP echo stacks, including those
different accept paths. A shared runner can surface regressions, but its
results are exploratory and have no percentage threshold for passing.

The workflow also supports these manual modes:

- `diagnose` holds the 16 KiB workload fixed while changing one geario
  setting at a time. It compares ordinary ring sends, plain sends with
  SendZc disabled, a larger receive buffer, and a handler that uses geario's
  existing direct socket write API. It takes perf samples and counts syscalls
  in separate instrumented runs.
- `diagnose64` compares default io_uring, Tokio, direct socket writes,
  and an explicitly configured 64 KiB receive buffer. It takes separate perf
  and syscall samples after measuring four rotated rounds at 64 KiB.
- `verify` builds the selected ref and a pinned baseline on the same runner
  with the same benchmark sources and lockfile. It compares both builds and
  Tokio in nine rotated rounds at 128 B, 16 KiB, and 64 KiB. Supply an optional
  `intermediate` ref for a fourth build and eight rotated rounds. The CI
  summary reports medians and the median per-round ratio against the baseline.

The [16 KiB diagnosis](https://github.com/zoujiaqing/geario/actions/runs/42456488002)
found a +17.9% throughput change when disabling SendZc, +9.1% when using a
larger receive buffer, and +52.8% when using the handler's direct write API.
Those are separate comparisons to the same baseline, with distinct costs
and possible interactions. The direct write experiment lowered the traced
`io_uring_enter` count from 0.67 to 0.25 per completed echo. The perf samples
also included page pinning and buffer-copy work in the baseline. Exact traced
counts can differ from those of untraced measurements.

The [bounded send A/B](https://github.com/zoujiaqing/geario/actions/runs/42456985449)
kept the benchmark handler unchanged. Trying one nonblocking send for an
idle medium single page improved paired 16 KiB throughput by 14.9% and
reduced CPU per echo by 13.3%. Closed-loop p99 rose 1.1% in that A/B.
The [receive capacity A/B](https://github.com/zoujiaqing/geario/actions/runs/42457017628)
used the bounded send as its baseline. Giving the default read buffer room
for a full 16 KiB of payload improved throughput by 26.4%, reduced CPU per
echo by 20.6%, and reduced closed-loop p99 by 3.5%. Both runs saw effectively
unchanged 128 B throughput. Absolute rates can differ across runners.

The [worker wake A/B](https://github.com/zoujiaqing/geario/actions/runs/42457124631)
also used the bounded send as its baseline. The io_uring reactor polls the
executor immediately after dispatching completions, so a local task queued
during dispatch no longer has to signal the eventfd. At 16 KiB, the change
improved paired throughput by 12.3%, reduced CPU per echo by 11.3%, and
reduced p99 by 7.9%. At 128 B it improved throughput by 6.4% and reduced CPU
per echo by 6.1%, while closed-loop p99 increased by 12.5%. The latter
comparison was at each variant's own saturation rate.

The [combined A/B](https://github.com/zoujiaqing/geario/actions/runs/42457469871)
compared all three changes with `4a06cd0` on one runner. At 16 KiB, the
paired result was +45.6% throughput, −31.5% CPU per echo, and −2.0%
closed-loop p99. Geario reached 130,033 RPS, compared with 89,429 for the
baseline and 146,834 for Tokio. The paired throughput gap to Tokio narrowed
from 39.1% to 11.4%. At 128 B the combined candidate changed throughput by
+5.5%, CPU per echo by −5.0%, and p99 by −13.6% relative to the baseline.
The [functional CI run](https://github.com/zoujiaqing/geario/actions/runs/42457474064)
passed for the measured code.

The [follow-up profile](https://github.com/zoujiaqing/geario/actions/runs/42465945456)
at `c96174d` found one send syscall and 0.25 `io_uring_enter` calls per
16 KiB echo under tracing. It found almost no notifier reads/writes. The
application's direct write experiment still improved untraced throughput by
7.2%. The profile showed TCP wakeups and data copies on both stacks, with
geario-specific samples in the buffered send and receive submission paths.

The [encoder-turn send A/B](https://github.com/zoujiaqing/geario/actions/runs/42466378064)
used identical handlers. At 16 KiB the paired median was +1.1% throughput,
−1.1% CPU/request, and −0.7% p99. Throughput improved in eight of nine
rounds. At 128 B the paired medians were −0.5%, +0.4%, and +0.6%, respectively.
At 64 KiB they were +1.9%, −1.9%, and −2.7%.

The [receive poll-first A/B](https://github.com/zoujiaqing/geario/actions/runs/42466465882)
reduced 16 KiB closed-loop p99 by 21.9% on its runner, while throughput
fell 2.2% and CPU/request increased 2.3%. It was excluded from the default
path. Absolute gaps to Tokio changed across runner CPUs. The paired effects
in this report are all within a run.

The [default read page recycling A/B](https://github.com/zoujiaqing/geario/actions/runs/42467143281)
used the encoder-turn send as its baseline. It found a +0.2% paired
throughput change at 16 KiB on its AMD runner. Default receive allocations
reuse the existing thread-local 16 KiB page pool after the last reference
is released. Explicit read buffer configuration continues to use its own
cache policy.

The [combined follow-up](https://github.com/zoujiaqing/geario/actions/runs/42467153879)
compared both changes with `c96174d` on one Intel Xeon 6973P-C runner. At
16 KiB, throughput increased in all nine rounds. The paired medians were
+6.4% throughput, −6.2% CPU/request, and −3.2% p99. The paired throughput
gap to Tokio narrowed from 11.8% to 6.3%. At 128 B the paired medians were
−0.1%, +0.1%, and +0.9%, respectively. At 64 KiB they were −0.3%, −0.3%,
and −0.1%. The [functional CI](https://github.com/zoujiaqing/geario/actions/runs/42467148169)
passed for the measured code.

The [64 KiB diagnostic run](https://github.com/zoujiaqing/geario/actions/runs/42469401135)
kept the request size, client, worker count, and runner fixed. Five paired
rounds separated the following changes:

| Change at 64 KiB | Paired throughput | Paired CPU/request | Paired p99 |
|---|---:|---:|---:|
| Grow from 16 KiB directly to 64 KiB when the socket has more data | +31.6% | −24.4% | −23.5% |
| Disable SendZc with the original read policy | +17.9% | −15.5% | −3.2% |
| Grow directly and disable SendZc | +56.7% | −35.8% | −18.9% |
| Batch 16 KiB slices with Writev instead of flushing each slice | +44.7% | −30.7% | +0.6% |

The SQE trace counted about four receive submissions and one SendZc per
baseline echo. Growing directly reduced that to two receive submissions.
The original path also copied the accumulated prefix at each growth step.
With full reads, the 16→32→48→64 KiB sequence copies 96 KiB of prefix,
while 16→64 KiB copies 16 KiB. Those byte totals describe the code path
under full reads. The receive and send submissions were counted directly
in a separate instrumented run. Starting with a 64 KiB read allocation
also changes the configuration and pool policy. It was slower than the
burst-growth variant on this runner.

The segmented comparison held read capacity and the plain send policy
fixed. The serial version made roughly four send syscalls per request.
The vectored version used approximately one Writev submission and
performed similarly to one contiguous plain Send. There was no evidence
for splitting an already-contiguous write.

The production receive path starts at 16 KiB and uses a pooled 64 KiB
page only when a default-sized page is nearly full and the receive path
needs more space. Custom receive sizing retains its growth policy.
The send path selects plain Send on known loopback TCP peers once at
registration. For loopback, Linux can still copy zero-copy payloads and
deliver a completion notification. See the [Linux kernel documentation](https://docs.kernel.org/next/networking/msg_zerocopy.html#loopback).

The [production A/B](https://github.com/zoujiaqing/geario/actions/runs/42470174425)
compared both changes with `515e853` on one AMD EPYC 7763 runner. There were
nine paired rounds per size, with one worker and four persistent connections.

| Size | Paired throughput | Paired CPU/request | Paired p99 |
|---|---:|---:|---:|
| 128 B | −0.6% | +0.8% | +0.3% |
| 16 KiB | −0.2% | +0.2% | −2.6% |
| 64 KiB | +67.4% | −40.3% | −27.5% |

At 64 KiB, geario went from 13,903 to 23,275 RPS and from 71.91 to
42.80 µs of server CPU per echo. Tokio reached 26,646 RPS at 37.39 µs
CPU per echo. The paired throughput gap went from 47.7% to 13.3%.
Geario's closed-loop p99 went from 346.2 to 247.8 µs, and Tokio's was
252.2 µs. The 128 B throughput change was small but consistently negative
in eight of nine rounds. Its paired bootstrap 95% interval was
[−2.28%, −0.16%], within the previous 3% throughput tolerance.
The [functional CI](https://github.com/zoujiaqing/geario/actions/runs/42470169264)
passed, including the burst-prefix, write cancellation, and short-write tests.

The [64 KiB follow-up profile](https://github.com/zoujiaqing/geario/actions/runs/42474315946)
used the runtime at `f635fdc`. TCP wakeups and kernel copies dominated
both profiles. Geario also spent time submitting and completing receive
work in the ring. Its traced baseline made roughly 0.67 ring entries
per completed echo. Bypassing the send queue with the existing direct
write API reduced that to 0.50 and added roughly one write syscall per
echo. Untraced throughput was unchanged and closed-loop p99 increased
24.5%. Always starting with an explicitly configured 64 KiB receive
allocation reduced throughput by 14.2%. Those two controls were kept
separate from the production changes.

Two receive candidates were then compared. One attempts a bounded
nonblocking continuation only after a burst buffer grows and the
kernel reports readable data. The other lets a connection that recently
received a large burst borrow a 64 KiB page from the existing pool.
The latter uses the normal receive lifecycle and leaves custom
buffer sizing in control.

The [nine-round continuation comparison](https://github.com/zoujiaqing/geario/actions/runs/42474673673)
at 64 KiB gave +1.4% throughput, −1.7% CPU/request, and +29.4% p99
against `f635fdc`. The [eight-round four-way comparison](https://github.com/zoujiaqing/geario/actions/runs/42474783478)
measured the continuation and cached-page candidates alongside the same
baseline and Tokio on one runner:

| 64 KiB variant | RPS | CPU µs/request | p99 µs | Paired RPS | Paired CPU/request | Paired p99 |
|---|---:|---:|---:|---:|---:|---:|
| Baseline | 39,874 | 24.96 | 129.15 | — | — | — |
| Nonblocking continuation | 39,078 | 25.58 | 192.35 | −1.9% | +2.1% | +42.3% |
| Cached page with full initial window | 40,905 | 24.42 | 152.40 | +4.0% | −4.0% | +16.1% |
| Tokio | 42,059 | 23.17 | 157.90 | — | — | — |

For the full-window candidate, the paired bootstrap 95% throughput
interval was [−1.37%, +5.52%], and p99 increased in seven of eight rounds.
Neither candidate cleared the tail latency check.

The [preserved-window comparison](https://github.com/zoujiaqing/geario/actions/runs/42475509137)
also reused a cached 64 KiB page, but limited the first receive to 16 KiB.
This retained the receive submission sequence while avoiding the prefix copy
on a continuation. Nine paired rounds on one AMD EPYC 7763 runner gave these
changes versus `f635fdc`:

| Size | Paired throughput | Paired CPU/request | Paired p99 |
|---|---:|---:|---:|
| 128 B | −0.3% | +0.4% | +0.4% |
| 16 KiB | −2.6% | +2.6% | +2.8% |
| 64 KiB | +0.6% | −0.3% | −1.7% |

The paired bootstrap 95% interval for throughput was [−4.03%, +2.59%]
at 64 KiB and [−2.98%, −1.49%] at 16 KiB. The 64 KiB baseline was
13.2% behind Tokio on throughput in this run. The candidate was 12.3%
behind, with 14.9% more CPU per request and 9.7% lower closed-loop p99.
The [functional CI](https://github.com/zoujiaqing/geario/actions/runs/42475505777)
passed. The 16 KiB throughput loss and uncertain 64 KiB gain excluded
this candidate from the default runtime.
