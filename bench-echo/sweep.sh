#!/bin/sh
# Where does throughput stop climbing?
#
# The client is one blocking thread per connection. If QPS keeps rising with
# more connections, the run is client-bound and cannot tell the two servers
# apart no matter how many rounds it does. The useful connection count is the
# one just past the knee.
set -eu

SECS="${SECS:-5}"
PAYLOAD="${PAYLOAD:-128}"
BIN="$(cd "$(dirname "$0")" && pwd)/target/release"
SERVER="${SERVER:-server-geario}"
PORT="${PORT:-8080}"

"$BIN/$SERVER" >/dev/null 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null || true' EXIT
for _ in $(seq 1 100); do nc -z 127.0.0.1 "$PORT" 2>/dev/null && break; sleep 0.1; done

printf '%-6s %10s %10s %10s\n' conns qps p50_us p99_us
for c in 1 2 4 8 12 16 24 32 48 64; do
    out=$("$BIN/client" "127.0.0.1:$PORT" "$c" "$SECS" "$PAYLOAD")
    q=$(echo "$out" | awk '/^qps/ {print $2}')
    p50=$(echo "$out" | awk '/^p50/ {print $2}')
    p99=$(echo "$out" | awk '/^p99/ {print $2}')
    printf '%-6s %10s %10s %10s\n' "$c" "$q" "$p50" "$p99"
done
