# Where the echo benchmark saturates

## Why this exists

The first geario-vs-ntex comparisons ran at 64 connections and produced
deltas that changed sign between rounds: -2.4%, then +3.4%, -4.8%, -11.8%,
+5.6%. Before adding rounds it was worth asking whether 64 was a sensible
place to measure at all.

## Sweep

`server-geario`, 128-byte payload, 4 seconds per point, Apple M1 Pro
(8 performance + 2 efficiency cores).

| conns | qps | p50 | p99 |
| --- | --- | --- | --- |
| 1 | 55,698 | 14.4 us | 58.7 us |
| 2 | 75,018 | 21.8 us | 77.2 us |
| 4 | 89,393 | 40.7 us | 107.0 us |
| 8 | 104,864 | 71.5 us | 147.2 us |
| **12** | **109,801** | 103.6 us | 202.6 us |
| 16 | 109,056 | 134.7 us | 383.2 us |
| 24 | 107,767 | 198.0 us | 811.9 us |
| 32 | 109,819 | 245.2 us | 1508.3 us |
| 48 | 115,798 | 326.5 us | 2700.9 us |
| 64 | 113,892 | 375.0 us | 4494.2 us |

## Reading

Throughput stops climbing at about twelve connections, near 110k requests
per second. Everything past that buys latency and nothing else: p99 grows
from 203 us to 4.5 ms, a factor of twenty-two, while throughput moves by
less than 6%.

So 64 connections sat deep in the saturated plateau. That is the worst place
to compare two implementations of the same thing. Throughput there is set by
queueing rather than by per-request cost, and queueing is exactly what varies
between runs. The wandering deltas were measuring the queue.

The client is one blocking thread per connection, so 64 of them on 10 cores
was also competing with the servers' own per-core workers for the machine.

## What the comparison measures now

Two points instead of one:

- **latency**, at a single connection. p50 is the round-trip cost with no
  queue in front of it. Per-request work shows up here directly.
- **throughput**, at the knee. The servers are busy but not thrashing.

## The machine

This is a working desktop. Zed, WindowServer, an editor assistant and
postgres hold a steady share of a core between them, so the load average
never approaches zero. It can resolve large differences and cannot resolve
small ones. `run.sh` refuses to start above half the core count, which keeps
the worst runs from being recorded, but it cannot manufacture a quiet
machine.

A few percent needs a dedicated host.
