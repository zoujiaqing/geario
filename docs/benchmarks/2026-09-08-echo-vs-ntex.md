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

## Why: measured, not inferred

strace -f -c on both servers under the same load (Rocky, 128 B, 4
connections). Counts normalized by the client's request total:

| syscall | geario /req | ntex /req |
| --- | --- | --- |
| read | 2.000 | 2.001 |
| write | 2.000 | 2.001 |
| epoll_pwait | 1.007 | 1.001 |
| **epoll_ctl** | **0.000** | **3.003** |
| **timerfd_settime** | **0** | **1.001** |
| **total** | **5.009** | **9.009** |

The dispatch path is identical: both read twice, write twice, and wait once
per request. The entire difference is the poller. Upstream `ntex-polling`
registers one-shot, so every interest delivered has to be re-armed with
`epoll_ctl` -- the connection fd and the notifier eventfd -- and it resets
the timerfd on every wait, together three `epoll_ctl` and one
`timerfd_settime` per request. `geario-polling` registers level-triggered
once, reads the eventfd only when it fired, and skips the timerfd when the
deadline has not changed: `epoll_ctl` drops to zero, the timerfd call
disappears. Nine syscalls per request against five.

This is the attribution, counted directly rather than argued from a forced
driver choice: the read/write/wait syscalls are equal, so the dispatch and
buffer paths are doing the same work, and the throughput gap is the four
syscalls per request the fork removes. It is largest at small payloads and
high request rates, where wakeups per byte are highest, and shrinks as the
payload grows.

## What this is not

Polling only; io_uring is not measured here. One echo service, two small
payloads, at the knee on two KVM guests. It says geario's IO and dispatch
path is faster than ntex's on this workload because it spends fewer syscalls
per wakeup. It does not measure many workers, real protocols, or the
io_uring driver.
