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

The workflow also supports two manual modes:

- `diagnose` holds the 16 KiB workload fixed while changing one geario
  setting at a time. It compares ordinary ring sends, plain sends with
  SendZc disabled, a larger receive buffer, and a handler that uses geario's
  existing direct socket write API. It takes perf samples and counts syscalls
  in separate instrumented runs.
- `verify` builds the selected ref and a pinned baseline on the same runner,
  using identical benchmark sources and dependency locks. It compares both
  builds and Tokio in nine rotated rounds at 128 B and 16 KiB. The CI summary
  reports medians and the median per-round ratio against the baseline.

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
