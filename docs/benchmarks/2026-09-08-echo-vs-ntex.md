# Echo: geario against ntex

- Date: 2026-09-08
- Goal: geario has to be faster than ntex, the framework it was ported from.
- Hosts: Rocky 9 (kernel 5.14, 4 cores) and Fedora 44 (kernel 7.0.12, 2 cores).
- Servers: `server-geario` and `server-ntex`, the same echo loop
  (`io.recv` / `io.send` with `BytesCodec`) written against each. ntex is a
  local checkout at 4.0.0-beta.4, later than geario's fork point.
- **Both forced to the polling driver** (`neon-polling`), so io_uring is out
  of the picture and the comparison isolates geario's forked poller
  (`geario-polling`) against upstream `ntex-polling`, plus whatever else has
  diverged since the fork.
- Method: `echo_ab.sh`, twelve paired rounds at the knee, alternating arms
  inside each round, bootstrap 95% CI over the per-round deltas. Positive
  means geario is faster.

## Results

| host | payload | conns | geario vs ntex |
| --- | --- | --- | --- |
| Rocky 9 | 128 B | 4 (knee) | **+14.09%** [+10.95%, +17.24%] |
| Rocky 9 | 1 KB | 4 (knee) | **+10.07%** [+7.95%, +12.23%] |
| Rocky 9 | 128 B | 1 (latency) | +3.82% [+0.65%, +7.45%] |
| Fedora 44 | 128 B | 2 (knee) | **+18.83%** [+18.27%, +19.46%] |

Every interval is clear of zero, on both kernels, with no bad rounds. At the
knee geario serves 278k echo requests a second where ntex serves 244k on
Rocky, and 98k against 82k on Fedora.

## Why

The win is the poller fork. Upstream `ntex-polling` does four avoidable
syscalls on every `wait`: it re-arms the eventfd and the timerfd one-shot,
and sets the timerfd even when the timeout has not changed. `geario-polling`
registers both level-triggered once, reads the eventfd only when it fired,
and skips the timerfd when the deadline is unchanged. An echo server wakes
up once per request, so those saved syscalls are saved per request, which is
why the lead is largest at small payloads and high request rates and shrinks
as the payload grows and each wakeup carries more work.

## What this is not

Polling only; io_uring is not measured here. One echo service, two small
payloads, at the knee on two KVM guests. It says geario's IO and dispatch
path is faster than ntex's on this workload because it spends fewer syscalls
per wakeup. It does not measure many workers, real protocols, or the
io_uring driver.
