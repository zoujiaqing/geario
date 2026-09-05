#!/bin/sh
# Alternating geario/ntex rounds.
#
# Servers are tracked by pid and waited on. An earlier version used pkill
# between rounds, which left processes alive often enough to contaminate the
# numbers: a stray server holds its port and competes for CPU with the round
# that follows.
set -eu

ROUNDS="${ROUNDS:-6}"
CONNS="${CONNS:-64}"
SECS="${SECS:-10}"
PAYLOAD="${PAYLOAD:-128}"
BIN="$(dirname "$0")/target/release"

run_one() {
    server="$1"
    port="$2"
    "$BIN/$server" >/dev/null 2>&1 &
    pid=$!
    # Wait for the port rather than sleeping a guess.
    for _ in $(seq 1 50); do
        if nc -z 127.0.0.1 "$port" 2>/dev/null; then break; fi
        sleep 0.1
    done
    qps=$("$BIN/client" "127.0.0.1:$port" "$CONNS" "$SECS" "$PAYLOAD" | awk '/^qps/ {print $2}')
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "$qps"
}

for r in $(seq 1 "$ROUNDS"); do
    g=$(run_one server-geario 8080)
    n=$(run_one server-ntex 8081)
    echo "$g $n"
done
