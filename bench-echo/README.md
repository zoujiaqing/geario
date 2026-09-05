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

    cargo run --release --manifest-path server-geario/Cargo.toml
    cargo run --release --manifest-path client/Cargo.toml -- 127.0.0.1:8080

Then the same two commands with server-ntex, which binds 8081.
