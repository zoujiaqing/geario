#!/bin/sh
# Alternating geario/ntex rounds at two operating points.
#
# A connection sweep showed throughput saturating around twelve connections on
# this machine, at roughly 110k requests per second. Past that, added
# connections buy queueing latency and nothing else: p99 goes from 200us at
# twelve to 4.5ms at sixty-four while throughput stays flat.
#
# Earlier runs used sixty-four, which put every measurement deep in that
# plateau. That is the worst place to compare two implementations of the same
# thing: run-to-run variance is highest there and per-request cost is least
# visible.
#
# So two points are measured instead:
#
#   latency     one connection. p50 is the round-trip cost with no queue in
#               front of it, which is the cleanest signal there is here.
#   throughput  at the knee, where the servers are busy but not thrashing.
set -eu

ROUNDS="${ROUNDS:-8}"
SECS="${SECS:-8}"
PAYLOAD="${PAYLOAD:-128}"
LAT_CONNS="${LAT_CONNS:-1}"
TPUT_CONNS="${TPUT_CONNS:-12}"
# Built binaries usually sit under target/release, but a cross-compiled set
# gets copied next to this script instead.
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ -x "$HERE/target/release/client" ]; then
    BIN="$HERE/target/release"
else
    BIN="$HERE"
fi
GPORT="${GPORT:-18080}"
NPORT="${NPORT:-18081}"

CORES=$(sysctl -n hw.ncpu 2>/dev/null || nproc)
load=$(uptime | sed 's/.*averages*:[ ]*//' | awk '{print $1}' | tr -d ',')
busy=$(awk -v l="$load" -v c="$CORES" 'BEGIN { print (l > c/2) ? 1 : 0 }')
if [ "$busy" = "1" ] && [ "${IGNORE_LOAD:-0}" != "1" ]; then
    echo "refusing: load average is $load on $CORES cores." >&2
    echo "          Wait, or set IGNORE_LOAD=1 and do not quote the result." >&2
    exit 1
fi

wait_port() {
    for _ in $(seq 1 100); do
        nc -z 127.0.0.1 "$1" 2>/dev/null && return 0
        sleep 0.1
    done
    echo "server never bound port $1" >&2
    return 1
}

# Servers are tracked by pid and waited on. Separating rounds with pkill left
# strays alive often enough to matter, and a stray holds its port and competes
# for CPU with the round after it.
run_one() {
    BENCH_ADDR="127.0.0.1:$2" "$BIN/$1" >/dev/null 2>&1 &
    pid=$!
    wait_port "$2"
    "$BIN/client" "127.0.0.1:$2" "$3" "$SECS" "$PAYLOAD" \
        | awk '/^qps/ {q=$2} /^p50/ {p=$2} END {print q, p}'
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

# One discarded round: the first pays for page faults and for cores still
# ramping their clocks.
echo "# warmup" >&2
run_one server-geario "$GPORT" "$TPUT_CONNS" >/dev/null
run_one server-ntex "$NPORT" "$TPUT_CONNS" >/dev/null

for mode in latency throughput; do
    case "$mode" in
        latency) conns="$LAT_CONNS" ;;
        throughput) conns="$TPUT_CONNS" ;;
    esac
    echo "# mode=$mode conns=$conns rounds=$ROUNDS secs=$SECS payload=$PAYLOAD cores=$CORES"
    for _ in $(seq 1 "$ROUNDS"); do
        set -- $(run_one server-geario "$GPORT" "$conns")
        gq=$1 gp=$2
        set -- $(run_one server-ntex "$NPORT" "$conns")
        nq=$1 np=$2
        echo "$gq $nq $gp $np"
    done
done
