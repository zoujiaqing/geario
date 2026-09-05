# echo benchmark: geario vs ntex, macOS

## Setup

| | |
| --- | --- |
| Date | 2026-09-06 |
| Machine | aarch64-apple-darwin |
| rustc | 1.98.1 (48a229cea 2026-09-01) |
| Driver | polling (kqueue) |
| ntex | fork point `48eef5bd`, sub-crates via path + `[patch.crates-io]` |
| Profile | `opt-level = 3`, `lto = true`, `codegen-units = 1` for both |
| Client | 64 connections, 128-byte payload, 10 s, blocking std sockets on threads |

Both servers echo through the same `BytesCodec`; geario's is the port of
ntex's, so the two paths are the same code modulo crate boundaries.

## Results

| Round | geario QPS | ntex QPS | Delta |
| --- | --- | --- | --- |
| 1 | 118,981 | 121,885 | -2.4% |
| 2 | 116,296 | 119,102 | -2.4% |
| 3 | 116,219 | 116,731 | -0.4% |
| **Median** | **116,296** | **119,102** | **-2.4%** |

Latency, median round:

| | geario | ntex |
| --- | --- | --- |
| p50 | 409.0 us | 444.4 us |
| p99 | 3618.1 us | 3061.8 us |

## Reading

The acceptance bar for the port was +/-3%, and -2.4% clears it. But the
number is not noise-shaped: two of three rounds land on -2.4% exactly, so
something small and real is more likely than measurement scatter.

Nothing in the port should cost throughput. Merging ten crates into one
removes cross-crate call boundaries, which if anything should help. Worth
a profile before the slimming phase starts, so the baseline is understood
rather than assumed.

p50 is better on geario and p99 is worse, in every round. That shape
usually means a scheduling or buffer-growth difference rather than a hot
path difference.

## Reproducing

    cd bench-echo
    cargo build --release
    ./target/release/server-geario &
    ./target/release/client 127.0.0.1:8080 64 10 128
