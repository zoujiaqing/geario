# Did the fork make geario slower than ntex? No.

- Date: 2026-09-09. Answering directly: geario is not behind the ntex 4.0 it
  was forked from. The earlier io_uring note compared geario's io_uring
  driver to geario's polling driver -- an internal comparison -- and reading
  it as a fork regression was a framing mistake in the report, corrected here.

## Same run, geario vs ntex 4.0 only, 16 KB, conns=4, 4 workers

| driver | geario | ntex 4.0 | geario vs ntex |
| --- | --- | --- | --- |
| io_uring | 114,680 | 114,084 | +0.52% [-1.00%, +2.06%] (tie) |
| polling | 183,796 | 173,530 | **+5.93%** [+4.69%, +7.14%] (geario faster) |

ntex was pinned to SHA `8af6d0271...` (crate 4.0.0-beta.9). Both built the
same way, same host, same config. On io_uring the two are equal; on polling
geario is ahead.

## The io_uring "slowdown" is ntex's own behaviour

The 16 KB io_uring number is low for *both*: ntex io_uring 114k vs ntex
polling 173k is the same cliff geario has. It is inherited, not introduced.

The code proves it. geario's io_uring `send()` is byte-identical to
`ntex-net/src/uring/stream.rs`: the 1536-byte zero-copy threshold, the
single outstanding `wr_op`, and the `SendZc` path are all upstream ntex, not
geario additions. The only geario change in that function is wrapping the
threshold constant in an env lookup that defaults to the same 1536. The two
other geario commits that touched the io_uring driver were correctness fixes
on the completion path (recovering data that a short send or an unfinished
read dropped); neither changed the send path.

## What is and is not claimed

- geario is faster than ntex on polling, equal on io_uring. No regression.
- The io_uring-vs-polling gap is a shared, inherited property of the ntex
  io_uring driver; improving it (vectored send, SendZc threshold) is a new
  optimization that would help geario pull ahead of ntex on io_uring too. It
  is not a fix for something the fork broke.
