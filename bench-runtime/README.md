# Linux TCP echo comparison

Run **Linux runtime comparison** from GitHub Actions. It compares geario's
forced polling and io_uring builds against Tokio current-thread, with one
server worker and four persistent clients. The server and load generator are
pinned to separate logical CPUs. All variants use the same client and receive
a fresh process and warmup per sample.

The artifact contains the six paired rounds at 128 bytes and 16 KiB, process
CPU per completed echo, closed-loop p99, binary hashes and runner information.
Geario uses its normal accept loop and worker dispatch. Tokio accepts on its
runtime thread. This measures the complete TCP echo stacks, including those
different accept paths. A shared runner can surface regressions, but its
results are exploratory and have no percentage threshold for passing.
