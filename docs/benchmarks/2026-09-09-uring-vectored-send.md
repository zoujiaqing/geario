# Vectored send: geario leads ntex on io_uring for medium/large responses

- Date: 2026-09-09. Host: Rocky 9 (kernel 5.14, 4 cores), io_uring on, 4 workers.
- The fix for the io_uring medium-response slowdown diagnosed earlier.

## Change

geario's io_uring `send()` gathered one page per SQE with one send
outstanding per connection, so a response spanning several 16 KB pages
serialized into that many send/completion round-trips. It now gathers up to
16 pages (64 KB) into one `Writev`, mirroring the polling driver's `writev`;
the single-page case keeps the existing Send/SendZc path. Partial writes are
handled like polling: advance the pages by the bytes taken, put the rest back
at the front of the write buffer. This is a plain vectored write, not
zero-copy.

## io_uring vs polling, same geario commit (the cliff is gone)

| body | polling | io_uring before | io_uring vectored | before | after |
| --- | --- | --- | --- | --- | --- |
| 16 KB | 184,528 | 111,516 | 170,184 | 0.61 | **0.92** |
| 32 KB | 145,031 | 95,935 | 133,909 | 0.67 | **0.92** |
| 64 KB | 97,592 | 69,064 | 88,869 | 0.75 | **0.91** |

## io_uring, geario (vectored) vs ntex 4.0 (unchanged), paired A/B

| body | geario vs ntex 4.0 |
| --- | --- |
| 8 KB | +0.58% [-1.97%, +3.15%] (tie) |
| 16 KB | **+47.83%** [+45.68%, +50.31%] |
| 32 KB | **+40.02%** [+37.48%, +43.09%] |
| 64 KB | **+33.81%** [+31.95%, +35.69%] |

ntex 4.0 still sends one page per SQE, so geario now leads it by a third to a
half on 16-64 KB responses over io_uring, and ties below one page. This is
the lead on the default Linux path that io_uring parity previously lacked.

## Attribution: batching vs dropping SendZc

The vectored change does two things at once -- merges several submissions
into one, and stops using SendZc for the multi-page case. Three configs of
the same binary, rotated per round, twelve rounds at 16 KB io_uring, via
`GEARIO_URING_MAX_WRITE_ITEMS` and `GEARIO_URING_ZC_SIZE`; bootstrap 95% CI,
zero errors, binary and client hashes in the raw log:

| step | delta | 95% CI |
| --- | --- | --- |
| dropping SendZc (per-page) | +10.64% | [+8.31%, +12.86%] |
| batching on top (vectored) | +32.94% | [+30.95%, +35.28%] |
| combined vs the original per-page SendZc | +47.03% | [+43.72%, +50.14%] |

Batching is the dominant factor; dropping SendZc is a real but smaller
separable part. The two compose multiplicatively (1.106 x 1.329 = 1.47).
The original per-page-SendZc path is what ntex 4.0 still runs, so the
combined figure is the like-for-like lead. (An earlier one-shot run put the
split at 19/30; the rotated sampling above supersedes those numbers -- the
single-run ratios were not stable.)

## Correctness## Correctness

Full suites pass on Rocky with the io_uring driver: geario-http 168 tests
(including the 128 KB full-duplex streaming/keep-alive test that caught the
earlier io_uring data-loss bugs) and geario core 249. macOS (kqueue) 373.
The partial-write path is the same shape the polling driver has used
throughout.

## Not yet

- 8 KB and below is one page, so it takes the single-page Send/SendZc path,
  not vectored; 1-8 KB io_uring still trails polling (SendZc penalty above
  1536 bytes, and io_uring's per-request cost at small sizes). Raising the
  SendZc threshold is the next lever.
- Not measured: p99, CPU per request, worker scaling, kqueue send behaviour.
