#!/bin/bash
# 直线高速飞行时前方的区块多快到达：一键起服、跑一名假玩家、汇总成表。
#
# 用法：run-chunk-trace.sh <结果目录> [服务端二进制] [端口]
#   服务端二进制缺省是本工作树 target/release/bamboo-loadtest。
#   GEN_DELAY_US 是模拟慢生成器的每区块额外耗时（缺省 25000 µs，约 25 ms）。
#   DURATION 是跑多久（缺省 40 秒），PROFILE_SECS 是服务端剖析报告间隔（缺省 10 秒）。
#   CHUNK_THREADS 是计算池线程数（缺省 4），BOTS 是朝不同方向飞的玩家数（缺省 1）。
set -u

TREE=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:?usage: run-chunk-trace.sh <result-dir> [server-binary] [port]}
BIN=${2:-$TREE/target/release/bamboo-loadtest}
PORT=${3:-26041}
GEN_DELAY_US=${GEN_DELAY_US:-25000}
DURATION=${DURATION:-40}
PROFILE_SECS=${PROFILE_SECS:-10}
CHUNK_THREADS=${CHUNK_THREADS:-4}
BOTS=${BOTS:-1}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

(cd "$OUT" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_FAKE_GEN_DELAY_US="$GEN_DELAY_US" \
    "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 --view-distance 8 \
    --chunk-worker-threads "$CHUNK_THREADS" > server.log 2>&1) &
SERVER=$!
cleanup() {
    kill "$SERVER" 2>/dev/null || true
    wait "$SERVER" 2>/dev/null || true
}
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

ready=0
for _ in $(seq 1 100); do
    if grep -q "loadtest server is listening" "$OUT/server.log" 2>/dev/null; then
        ready=1
        break
    fi
    if ! kill -0 "$SERVER" 2>/dev/null; then
        echo "server exited before it was ready:" >&2
        tail -20 "$OUT/server.log" >&2
        exit 1
    fi
    sleep 0.2
done
if [ "$ready" -ne 1 ]; then
    echo "server did not become ready" >&2
    cleanup
    exit 1
fi

"$BIN" --addr "127.0.0.1:$PORT" --bots "$BOTS" --pattern fly --layout spread \
    --duration "$DURATION" --ramp 0 --chunk-trace \
    --json "$OUT/client.json" --trace-json "$OUT/trace.json" > "$OUT/client.log" 2>&1
CLIENT_STATUS=$?
sleep 2
cleanup
trap - EXIT
cat "$OUT/client.log"
echo
if [ "$CLIENT_STATUS" -ne 0 ]; then
    echo "loadtest client failed ($CLIENT_STATUS)" >&2
    exit "$CLIENT_STATUS"
fi
python3 "$TREE/tools/mc-bamboo-loadtest/summarize-chunk-trace.py" "$OUT"
