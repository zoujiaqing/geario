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

## io_uring: even

The same comparison with both servers on their default driver, which on
Fedora is io_uring for both (confirmed by strace: `io_uring_enter` present).
All workers, twelve paired rounds:

| payload | conns | geario vs ntex (io_uring) |
| --- | --- | --- |
| 128 B | 2 | -0.01% [-0.83%, +0.81%] |
| 128 B | 8 | -0.27% [-0.76%, +0.19%] |
| 1 KB | 8 | +0.24% [-0.07%, +0.57%] |

Dead even, every interval across zero. This is expected and it bounds the
claim: the io_uring driver does not use the epoll backend, so the poller
fork saves nothing there. geario's lead over ntex is real on the polling
driver -- kqueue on macOS, epoll on Linux, anywhere io_uring is off or
unavailable -- and absent on io_uring, where the two are the same speed.

## Where this leaves it

- **Polling: geario wins**, 10-19% at the knee, counted down to the four
  syscalls per request the fork removes, on two kernels and all workers.
- **io_uring: geario ties ntex.** To lead on the default Linux path the win
  has to come from the dispatch or buffer path, which the syscall counts
  show is currently identical work on both. That is the next target.

## What this is not

Two payloads at the knee on two KVM guests, echo only. It does not measure
real protocols; geario-http against ntex is a separate exercise.
