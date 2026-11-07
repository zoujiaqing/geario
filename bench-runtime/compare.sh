#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

result_dir=${1:-results}
mkdir -p "$result_dir"
result_dir=$(cd "$result_dir" && pwd)
mapfile -t cpus < <(python3 -c 'import os; print(*sorted(os.sched_getaffinity(0)), sep="\n")')
if (( ${#cpus[@]} < 2 )); then
    echo "At least two CPUs are required to separate server and client" >&2
    exit 1
fi

{
    git rev-parse HEAD
    uname -a
    rustc -Vv
    lscpu
    echo "server_cpu=${cpus[0]} client_cpu=${cpus[1]}"
} > "$result_dir/environment.txt"

# A forced driver fails on init rather than silently falling back to polling.
for driver in polling uring; do
    cargo build --locked --release --features "$driver"
    cp target/release/echo-geario "$result_dir/geario-$driver"
done
sha256sum "$result_dir"/geario-{polling,uring} target/release/echo-{tokio,client} \
    > "$result_dir/binaries.sha256"

export BENCH_ADDR=127.0.0.1:18080 BENCH_WORKERS=1
pid=
cleanup() {
    if [[ -n $pid ]]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        pid=
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

measure() {
    local name=$1 exe=$2 size=$3 round=$4 key
    key="$result_dir/${size}-${round}-${name}"
    taskset -c "${cpus[0]}" "$exe" > "$key.server.log" 2>&1 &
    pid=$!
    python3 - "$pid" "$BENCH_ADDR" <<'PY'
import os, socket, sys, time
host, port = sys.argv[2].split(":")
for _ in range(100):
    os.kill(int(sys.argv[1]), 0)
    try:
        with socket.create_connection((host, int(port)), 0.1):
            break
    except OSError:
        time.sleep(0.05)
else:
    sys.exit("server did not listen")
PY
    taskset -c "${cpus[1]}" target/release/echo-client "$BENCH_ADDR" 4 1 "$size" > "$key.warmup.txt"
    local before after
    before=$(awk '{print $14 + $15}' "/proc/$pid/stat")
    taskset -c "${cpus[1]}" target/release/echo-client "$BENCH_ADDR" 4 3 "$size" > "$key.txt"
    after=$(awk '{print $14 + $15}' "/proc/$pid/stat")
    awk -v ticks="$((after - before))" -v hz="$(getconf CLK_TCK)" \
        '/^requests / {printf "server_cpu_us_per_request %.3f\n", 1000000 * ticks / hz / $2}' \
        "$key.txt" >> "$key.cpu.txt"
    printf '%s round=%s body=%s\n' "$name" "$round" "$size"
    cat "$key.txt" "$key.cpu.txt"
    cleanup
}

for size in 128 16384; do
    for round in {0..5}; do
        names=(polling uring tokio)
        for offset in {0..2}; do
            name=${names[(round + offset) % 3]}
            exe="$result_dir/geario-$name"
            if [[ $name == tokio ]]; then exe=target/release/echo-tokio; fi
            measure "$name" "$exe" "$size" "$round"
        done
    done
done
