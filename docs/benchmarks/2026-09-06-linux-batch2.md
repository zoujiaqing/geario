# Linux acceptance and the geario vs ntex comparison there

Completes batch two of the phase-1 port spec, which was deferred until a
Linux host was available.

## Host

| | |
| --- | --- |
| Machine | 4 vCPU AMD EPYC 7K62, KVM guest |
| OS | Rocky Linux 9.8, kernel 5.14 |
| glibc | 2.34 |
| Steal time | 0% during the runs |
| Load before starting | 0.18 |

Binaries are cross-compiled from macOS against glibc 2.34 with the
`x86_64-unknown-linux-gnu` toolchain already on the build machine, so the
server needs no Rust installed and keeps its own allocator.

## Build

| Driver | Result |
| --- | --- |
| default (polling) | OK |
| `neon-polling` | OK |
| `neon-uring` | OK |

The uring driver did not build before this. `net/uring/reactor.rs` imported
`crate::rt::syscall`, but `syscall` is `macro_export`ed and therefore lives
at the crate root. The same mistake was fixed elsewhere during the port;
this copy survived because `net/uring/` is `cfg(target_os = "linux")` and
had never been compiled. Cross-compiling found it on the first attempt.

## Tests

242 passed, no failures, across nine test binaries.

macOS runs 241 unit and integration tests. The extra one here is the DNS
resolver test, which is `#[ignore]`d on the development machine: a fake-IP
proxy there resolves every name, so its assertion that an unknown host fails
to resolve cannot hold. On a host with ordinary DNS it passes.

## Comparison

Ten rounds per mode, alternating, one warmup discarded. Connection counts
come from a sweep on this host: throughput knees at four connections, which
is also the core count.

| Mode | Metric | geario | ntex | per-round delta |
| --- | --- | --- | --- | --- |
| 4 conn | qps | 206,999 | 213,184 | -2.8% (-9.1 .. +4.8) |
| 4 conn | p50 | 14.8 us | 14.6 us | +2.1% (-6.5 .. +26.1) |
| 16 conn | qps | 254,870 | 260,607 | -1.3% (-17.7 .. +15.3) |
| 16 conn | p50 | 58.8 us | 57.2 us | -1.1% (-10.3 .. +19.2) |

All four sets change sign between rounds. Nothing is established.

## Two things worth knowing about this host

Latency at one and two connections is *worse* than at four: 29.3 us and
31.4 us against 13.9 us. Latency should not fall as load rises. The guest
has no cpufreq control, so the host decides clocks and idle states, and at
one connection the machine is idle enough to be slowed down. Measurements
below the knee are not usable here.

Spread is also wider than on the development laptop, up to 33 points against
4. A shared 4-vCPU cloud instance is a worse measurement environment than a
busy laptop with ten cores, even at zero steal.

## Against macOS

| | M1 Pro, 10 cores | EPYC 7K62, 4 vCPU |
| --- | --- | --- |
| Knee | 12 connections | 4 connections |
| Peak throughput | ~110k | ~270k |
| p50 at the knee | 103 us | 14 us |
| Delta spread | 4 points | up to 33 points |

Linux does about 2.4 times the throughput at a fraction of the latency,
which is the usual gap between the two network stacks rather than anything
about geario.

## Where the comparison stands

Two machines, four operating points, eight metric sets, none of which
establishes a difference. That is consistent with the port having cost
nothing, and it is what one would expect from a port. It is not proof.

Narrowing further needs longer rounds, more of them, and pinned cores.
