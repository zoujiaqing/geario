# geario vs ntex, two operating points

Supersedes `2026-09-06-echo-macos.md`, whose conclusion was withdrawn.

## Status of this run

**Harness smoke test, not a result.** Taken with `IGNORE_LOAD=1` on a machine
at load ~5 of 10 cores. It shows the harness works end to end and that the
spread is now small enough to be worth quoting; it does not establish
anything about the two servers.

## Setup

| | |
| --- | --- |
| Machine | Apple M1 Pro, 8 performance + 2 efficiency cores |
| rustc | 1.98.1 |
| Driver | polling (kqueue) |
| ntex | fork point `48eef5bd`, sub-crates by path with `[patch.crates-io]` |
| Profile | `opt-level = 3`, `lto = true`, `codegen-units = 1`, both sides |
| Rounds | 6 per mode, alternating, one warmup discarded |

Two operating points, chosen from the connection sweep:

- **latency**, 1 connection. p50 is the round trip with no queue in front.
- **throughput**, 12 connections, the knee. Busy but not thrashing.

## Results

| Mode | Metric | geario | ntex | per-round delta |
| --- | --- | --- | --- | --- |
| 1 conn | p50 | 13.9 us | 14.1 us | -0.3% (-4.3 .. +1.5) |
| 1 conn | qps | 62,227 | 59,976 | +2.5% (-1.1 .. +4.9) |
| 12 conn | p50 | 101.4 us | 102.2 us | -0.0% (-2.5 .. +1.6) |
| 12 conn | qps | 112,730 | 111,294 | -0.1% (-3.3 .. +4.5) |

All four sets change sign between rounds, so none of them establishes a
difference. The medians are printed because hiding them would be worse, not
because they mean anything.

## What improved

The old harness, at 64 connections with `pkill` between rounds, produced
deltas of -2.4%, +3.4%, -4.8%, -11.8%, +5.6%: a spread of seventeen points.
This one, on a machine that is no quieter, holds every delta inside about
four points.

Three changes account for it. Rounds no longer leave stray servers behind.
The measurement moved off the saturated plateau, where throughput is set by
queueing rather than by per-request cost. And a warmup round is discarded.

## What is still needed

A quiet machine. This is a working desktop: an editor, a window server, an
editor assistant and postgres hold a steady share of a core between them, so
the load average never approaches zero. `run.sh` refuses above half the core
count, which is why this run had to override it.

Four points of spread is enough to see a large regression and not enough to
resolve a few percent. Both numbers being this close is consistent with the
port having cost nothing, which is what one would expect from a port; it is
not evidence of it.

## Reproducing

    cd bench-echo
    cargo build --release
    ./run.sh | tee raw.txt
    ./report.py < raw.txt
