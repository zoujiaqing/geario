# echo benchmark

Compares geario against the ntex crates it was ported from, on the same
machine with the same client.

`server-ntex` depends on ntex-io, ntex-net, ntex-server and ntex-codec
directly rather than on the `ntex` crate: `ntex` does not build at the
fork point, because ntex-h2 on the `service-updates` branch is out of
sync with ntex's Pipeline API.

This directory is excluded from the geario workspace so that the ntex
copies of Io, Codec and friends never enter geario's dependency tree.

## Running

    cargo build --release
    ./run.sh

`run.sh` alternates the two servers and prints one `geario ntex` pair per
round. It tracks each server by pid and waits for it to exit; an earlier
version used pkill between rounds and left processes alive often enough to
contaminate the numbers.

Nothing else should be building or running while it does. The measurement
does not resolve small differences: see docs/benchmarks for what the spread
looks like in practice.
