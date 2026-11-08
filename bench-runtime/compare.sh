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

mode=${BENCH_MODE:-compare}
drivers=(polling uring)
if [[ $mode == diagnose || $mode == diagnose64 || $mode == verify ]]; then
    drivers=(uring)
fi
if [[ $mode == diagnose || $mode == diagnose64 ]]; then
    export RUSTFLAGS="${RUSTFLAGS:-} -C force-frame-pointers=yes -C debuginfo=1"
fi

# A forced driver fails on init rather than silently falling back to polling.
for driver in "${drivers[@]}"; do
    cargo build --locked --release --features "$driver"
    cp target/release/echo-geario "$result_dir/geario-$driver"
done
if [[ $mode == verify ]]; then
    base=$(git rev-parse --verify "${BENCH_BASE_REF:?missing baseline commit}^{commit}")
    echo "baseline=$base" >> "$result_dir/environment.txt"
    baseline=$(mktemp -d)
    trap 'git worktree remove --force "$baseline"' EXIT
    git worktree add --detach "$baseline" "$base"
    # Keep both harnesses identical, including the dependency lock.
    cp -R Cargo.toml Cargo.lock src "$baseline/bench-runtime/"
    cp ../bench-echo/server-geario/src/main.rs "$baseline/bench-echo/server-geario/src/"
    cp ../bench-echo/client/src/main.rs "$baseline/bench-echo/client/src/"
    CARGO_TARGET_DIR="$PWD/target" cargo build --manifest-path "$baseline/bench-runtime/Cargo.toml" \
        --locked --release --features uring --bin echo-geario
    cp target/release/echo-geario "$result_dir/geario-base"
    git worktree remove --force "$baseline"
    trap - EXIT
fi
sha256sum "$result_dir"/geario-* target/release/echo-{tokio,client} \
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

start() {
    local key=$1
    shift
    taskset -c "${cpus[0]}" "$@" > "$key.server.log" 2>&1 &
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
}

server_command() {
    command=("$result_dir/geario-uring")
    case "$1" in
        tokio) command=(target/release/echo-tokio) ;;
        polling) command=("$result_dir/geario-polling") ;;
        uring) ;;
        base) command=("$result_dir/geario-base") ;;
        plain) command=(env GEARIO_URING_ZC_SIZE=4294967295 "${command[@]}") ;;
        read32k) command=(env BENCH_READ_CAPACITY=32744 "${command[@]}") ;;
        direct) command=(env BENCH_DIRECT_WRITE=1 "${command[@]}") ;;
        read64k) command=(env BENCH_READ_CAPACITY=65536 "${command[@]}") ;;
        read64plain) command=(env BENCH_READ_CAPACITY=65536 GEARIO_URING_ZC_SIZE=4294967295 "${command[@]}") ;;
        vectored|serial) command=(env BENCH_READ_CAPACITY=65536 GEARIO_URING_ZC_SIZE=4294967295 BENCH_SEGMENTS="$1" "${command[@]}") ;;
        *) echo "unknown variant: $1" >&2; return 1 ;;
    esac
}

measure() {
    local name=$1 size=$2 round=$3 key
    key="$result_dir/${size}-${round}-${name}"
    local -a command
    server_command "$name"
    start "$key" "${command[@]}"
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

if [[ $mode == diagnose ]]; then
    names=(uring tokio plain read32k direct)
    sizes=(16384)
    rounds=5
elif [[ $mode == diagnose64 ]]; then
    names=(uring tokio plain read64k read64plain vectored serial)
    sizes=(65536)
    rounds=7
elif [[ $mode == verify ]]; then
    names=(base uring tokio)
    sizes=(128 16384 65536)
    rounds=9
else
    names=(polling uring tokio)
    sizes=(128 16384)
    rounds=6
fi
for size in "${sizes[@]}"; do
    for ((round=0; round<rounds; round++)); do
        for ((offset=0; offset<${#names[@]}; offset++)); do
            measure "${names[(round + offset) % ${#names[@]}]}" "$size" "$round"
        done
    done
done

if [[ $mode == diagnose || $mode == diagnose64 ]]; then
    profile_size=${sizes[0]}
    perf_bin=${BENCH_PERF:-$(command -v perf || true)}
    for name in "${names[@]}"; do
        server_command "$name"
        key="$result_dir/$profile_size-profile-$name"
        start "$key" "${command[@]}"
        taskset -c "${cpus[1]}" target/release/echo-client "$BENCH_ADDR" 4 1 "$profile_size" > "$key.warmup.txt"
        # Profiling is separate from the untraced latency/throughput samples.
        if [[ -n $perf_bin ]] && sudo "$perf_bin" stat -e task-clock -- true >/dev/null 2>&1; then
            sudo "$perf_bin" record -F 997 --call-graph fp -e cpu-clock -p "$pid" -o "$key.data" -- sleep 6 > "$key.perf.log" 2>&1 &
            profiler=$!
            sleep 1
            taskset -c "${cpus[1]}" target/release/echo-client "$BENCH_ADDR" 4 4 "$profile_size" > "$key.client.txt"
            if wait "$profiler"; then
                sudo "$perf_bin" report --stdio --no-children -i "$key.data" > "$key.perf.txt"
            fi
        else
            echo "perf unavailable on this runner" > "$key.perf.log"
        fi
        if command -v strace >/dev/null; then
            sudo timeout -s INT 6 strace -f -c -p "$pid" -o "$key.syscalls.txt" > "$key.strace.log" 2>&1 &
            tracer=$!
            for ((attempt=0; attempt<100; attempt++)); do
                if awk '/^TracerPid:/ {exit $2 == 0}' "/proc/$pid/status"; then break; fi
                sleep 0.02
            done
            awk '/^TracerPid:/ {exit $2 == 0}' "/proc/$pid/status"
            taskset -c "${cpus[1]}" target/release/echo-client "$BENCH_ADDR" 4 3 "$profile_size" > "$key.strace-client.txt"
            rc=0
            wait "$tracer" || rc=$?
            [[ $rc == 124 ]]
            test -s "$key.syscalls.txt"
        fi
        cleanup
    done
fi
