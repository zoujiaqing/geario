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

Taken with the old harness. See "Reading" below before using these.

Latency, median round:

| | geario | ntex |
| --- | --- | --- |
| p50 | 409.0 us | 444.4 us |
| p99 | 3618.1 us | 3061.8 us |

## Reading

**Withdrawn.** The first version of this file read -2.4% as a real effect
because two of three rounds landed on it exactly. Later rounds on the same
harness produced -3.9%, +3.4%, -4.8%, then -11.8% and +5.6%: a spread wide
enough to contain zero several times over, with the sign changing.

Three samples were not enough to tell a repeated number from a coincidence,
and reading a trend into them was the mistake.

Part of the spread had a cause. `run.sh` used to separate rounds with
`pkill`, which left a server alive often enough to matter; a stray holds its
port and competes for CPU with the round that follows. That is fixed, but
the numbers above were taken with the old harness and are not worth
re-interpreting.

## Where this leaves the question

The port should not cost throughput. Merging ten crates into one removes
cross-crate call boundaries, which if anything should help. Whether it does
is still unmeasured: this harness, on this machine, does not resolve a
difference of a few percent.

Re-measuring needs a quiet machine, more rounds, and a look at the spread
before any single number is quoted. Until then there is nothing here to
profile, because there is nothing established to explain.

## Reproducing

    cd bench-echo
    cargo build --release
    ./target/release/server-geario &
    ./target/release/client 127.0.0.1:8080 64 10 128
