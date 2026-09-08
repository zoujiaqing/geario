# Why io_uring is slower than polling for medium responses

- Date: 2026-09-09
- Host: Rocky 9 (kernel 5.14, 4 cores), io_uring enabled. Same geario commit
  for both drivers, only the driver feature differs. Binary sha256 recorded
  in the raw logs. Single-run numbers to locate and attribute; each factor
  toggled one at a time.

## The slowdown, same commit, only the driver changed

geario echo/HTTP, conns=4, 4 workers, response body swept:

| body | polling qps | io_uring qps | io_uring / polling |
| --- | --- | --- | --- |
| 1 KB | 225,981 | 210,696 | 0.93 |
| 4 KB | 197,336 | 194,717 | 0.99 |
| 8 KB | 199,831 | 171,618 | 0.86 |
| 16 KB | 182,173 | 111,516 | **0.61** |
| 32 KB | 143,653 | 95,935 | 0.67 |
| 64 KB | 92,702 | 69,064 | 0.75 |

io_uring falls behind from ~8 KB, worst at 16 KB (39% slower). This is a
geario-internal comparison, not against ntex.

## Two causes, each confirmed by a one-factor toggle

At 16 KB, conns=4:

| configuration | qps | vs polling |
| --- | --- | --- |
| polling | 183,771 | — |
| io_uring, default (16 KB page, SendZc>1536) | 113,867 | 62% |
| io_uring, 64 KB page (one send), SendZc on | 153,593 | 84% |
| io_uring, 64 KB page + SendZc off | 171,724 | 93% |

1. **Per-page send serialization.** The write buffer is paged at 16 KB
   (`BytePageSize::Size16`), and the io_uring `send()` takes one page per SQE
   with only one send outstanding per connection (`wr_op` is single-slot). A
   16 KB body plus headers spans two pages, so it becomes two serialized
   send/completion round-trips, where the polling driver gathers all pages
   into one `writev`. Making the page hold the whole response (one send)
   recovers 114k -> 154k. Toggled via `BENCH_WRITE_PAGE`.

2. **SendZc.** Above 1536 bytes the driver uses zero-copy `SendZc`, which
   pins the buffer's pages and posts an extra completion. At these sizes the
   pinning plus the second CQE costs more than the copy it avoids. Disabling
   it (one page already) recovers 154k -> 172k, within 6.5% of polling.
   Toggled via `GEARIO_URING_ZC_SIZE`.

Together they take 16 KB io_uring from 62% of polling to 93%. The residual
is not yet attributed.

## The proper fixes (next)

- **Vectored send.** Gather the queued pages into one `Writev`/`SendMsg`
  instead of one `Send` per page, mirroring the polling driver's `writev`.
  This removes the serialization at the default page size, so it needs no
  memory-per-connection inflation. This is the main fix.
- **Raise the SendZc threshold.** 1536 is far too low; zero-copy did not pay
  at any size measured up to 64 KB. The break-even, if any, is higher and
  needs its own measurement before a new default is set.

Both must be landed one at a time, re-checked against ntex 4.0, and verified
for correctness on slow-reading clients, cancellation, disconnect and buffer
exhaustion before the gain is claimed. ntex 4.0's io_uring driver has the
same two behaviours, so fixing them is a route to leading on the default
Linux path, not just closing an internal gap.

## Corrections carried from the previous report

- "Both drivers drop together, so it is the shared ported driver" was stated
  before the cause was known. It is now attributed by toggle to geario's own
  send path; the claim no longer rests on the coincidence.
- These are Linux polling/io_uring numbers. They say nothing about kqueue.
- The fix is not assumed to be multishot: the problem is on the send side,
  not recv, and vectored send plus the SendZc threshold are the levers.
