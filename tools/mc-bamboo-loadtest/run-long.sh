#!/bin/bash
# 岛的实施计划第 11 步：长时间压力。缺省配置（多岛开、`island_threads` 缺省）下跑 `islands-roam`，
# 全程开每刻全量对照和不变量断言（`BAMBOO_PROFILE_ISLANDS=2`，release 构建也查），每隔几秒记一次
# 服务端进程的内存、CPU 时间和机器负载，结束后由 summarize-long.py 汇总。
#
# 用法：run-long.sh <工作树> [名字]
#   DURATION（默认 3600 秒）、PROFILE_SECS（默认 30）、SAMPLE_SECS（默认 10）、PORT（默认 25997）、
#   OUT（默认 /tmp/bamboo-long）、BOTS（缺省用场景自己的人数）、RAMP（默认 20 秒）、
#   WORLD_COMMAND_EVERY（默认 1200 刻：每分钟一条要整个世界的命令）、VERIFY（默认 2，
#   给 BAMBOO_PROFILE_ISLANDS；1 = 只统计不全量对照）。其余 BAMBOO_* 环境变量原样传给服务端
#   （调参时用来换分区器参数或线程数）。
#
# Island plan step 11: the long run. `islands-roam` under the default
# configuration with the per-tick full check and invariant assertions on
# (`BAMBOO_PROFILE_ISLANDS=2`, release builds included); the server's memory,
# CPU time and the machine load are sampled every few seconds, and
# summarize-long.py writes the summary.
set -u
TREE=$(cd "${1:?worktree}" && pwd)
NAME=${2:-run}
DURATION=${DURATION:-3600}
PROFILE_SECS=${PROFILE_SECS:-30}
SAMPLE_SECS=${SAMPLE_SECS:-10}
PORT=${PORT:-25997}
OUT=${OUT:-/tmp/bamboo-long}
RAMP=${RAMP:-20}
WORLD_COMMAND_EVERY=${WORLD_COMMAND_EVERY:-1200}
VERIFY=${VERIFY:-2}
SCENE=islands-roam
BIN="$TREE/target/release/bamboo-loadtest"
DIR="$OUT/$NAME"
rm -rf "$DIR"
mkdir -p "$DIR"

SERVER=""
SAMPLER=""
cleanup() {
    [ -n "$SAMPLER" ] && kill "$SAMPLER" 2>/dev/null
    [ -n "$SERVER" ] && kill "$SERVER" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

read -r bots pattern layout _ <<< "$("$BIN" describe "$SCENE")"
bots=${BOTS:-$bots}
env | grep '^BAMBOO_' > "$DIR/env.txt"
echo "=== $SCENE $NAME: $bots bots for $DURATION s ($(uptime | sed 's/.*load averages*: //')) ==="
(cd "$DIR" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_PROFILE_ISLANDS="$VERIFY" \
    BAMBOO_LOADTEST_WORLD_COMMAND_EVERY="$WORLD_COMMAND_EVERY" \
    "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 \
    --view-distance 8 --scenario "$SCENE" > server.log 2>&1) &
SERVER=$!
ready=0
for _ in $(seq 1 1500); do
    grep -q "bench scenario ready" "$DIR/server.log" 2>/dev/null && { ready=1; break; }
    kill -0 "$SERVER" 2>/dev/null || break
    sleep 0.2
done
[ "$ready" = 1 ] || { echo "$SCENE: scenario not ready" >&2; exit 1; }

# 内存（RSS，KiB）、进程 CPU 时间（秒）、1 分钟负载，每 SAMPLE_SECS 秒一行。
# Memory (RSS, KiB), process CPU time (s) and the 1-minute load, one line every SAMPLE_SECS.
(
    echo "time rss_kib cpu_seconds load1"
    while kill -0 "$SERVER" 2>/dev/null; do
        sample=$(ps -o rss=,cputime= -p "$SERVER" 2>/dev/null) || break
        load=$(uptime | sed 's/.*load averages*: //' | awk '{print $1}' | tr -d ',')
        python3 - "$sample" "$load" <<'PY'
import sys, time
rss, cpu = sys.argv[1].split()
days = 0
if "-" in cpu:
    d, cpu = cpu.split("-", 1)
    days = int(d)
total = 0.0
for part in cpu.split(":"):
    total = total * 60 + float(part)
print(f"{time.time():.1f} {rss} {total + days * 86400:.1f} {sys.argv[2]}")
PY
        sleep "$SAMPLE_SECS"
    done
) > "$DIR/samples.txt" &
SAMPLER=$!

uptime > "$DIR/load-before.txt"
"$BIN" --addr "127.0.0.1:$PORT" --bots "$bots" --pattern "$pattern" \
    --layout "$layout" --duration "$DURATION" --ramp "$RAMP" \
    --json "$DIR/client.json" > "$DIR/client.log" 2>&1
uptime > "$DIR/load-after.txt"
sleep 2
kill -0 "$SERVER" 2>/dev/null && echo "alive" > "$DIR/server-state.txt" || echo "exited" > "$DIR/server-state.txt"
kill -INT "$SERVER" 2>/dev/null
wait "$SERVER" 2>/dev/null
SERVER=""
kill "$SAMPLER" 2>/dev/null
SAMPLER=""

python3 "$TREE/tools/mc-bamboo-loadtest/summarize-long.py" "$DIR" | tee "$DIR/summary.txt"
