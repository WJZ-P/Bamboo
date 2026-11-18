#!/bin/bash
# 岛的实施计划第 10 步 d 段：多岛开 / 关、tick 线程数不同的几种配置，同一时间段内交替比较。
#
# 用法：compare-islands.sh <工作树> [场景...]
#   缺省场景 islands-1 islands-4 islands-8 islands-16 islands-heavy-4。
#   CONFIGS（默认 "off:0:1 on:1:1 on:1:2 on:1:4 on:1:8"）：每项「名字:BAMBOO_ISLANDS:BAMBOO_ISLAND_THREADS」，
#     名字只用来起目录名（目录是 <名字>-t<线程数>）。
#   ROUNDS（默认 2）、DURATION（默认 30 秒）、PROFILE_SECS（默认 5）、PORT（默认 25996）、
#   OUT（默认 /tmp/bamboo-islands）、WORLD_COMMAND_EVERY（默认 0，>0 时每隔这么多刻从控制台发一条
#   要整个世界的命令）、HEAVY_ITEMS（重场景第 0 组的额外掉落物个数，缺省用场景自己的）。
#
# 同一个二进制，只换环境变量。每个场景、每一轮里把全部配置各跑一个全新进程；单数轮按 CONFIGS 的
# 顺序、双数轮倒过来（A-B-C…C-B-A），抵消时间段内负载的漂移。每次运行前后记 `uptime` 的负载，
# 假玩家跑的那段前后各取一次服务端进程的 CPU 时间。剖析报告丢掉前两份（搭场景、进人），
# 汇总由 summarize-islands-compare.py 做。
#
# Same binary, only the environment differs. For each scene and round every
# configuration runs in a fresh process; odd rounds in CONFIGS order, even
# rounds reversed, so drift in machine load cancels. The load average is
# logged around every run and the server's CPU time is sampled before and
# after the bots' run. The summary is summarize-islands-compare.py's.
set -u
TREE=$(cd "${1:?worktree}" && pwd)
shift
SCENES=("$@")
[ "${#SCENES[@]}" -eq 0 ] && SCENES=(islands-1 islands-4 islands-8 islands-16 islands-heavy-4)
read -r -a CONFIG_LIST <<< "${CONFIGS:-off:0:1 on:1:1 on:1:2 on:1:4 on:1:8}"
ROUNDS=${ROUNDS:-2}
DURATION=${DURATION:-30}
PROFILE_SECS=${PROFILE_SECS:-5}
PORT=${PORT:-25996}
OUT=${OUT:-/tmp/bamboo-islands}
WORLD_COMMAND_EVERY=${WORLD_COMMAND_EVERY:-0}
BIN="$TREE/target/release/bamboo-loadtest"
mkdir -p "$OUT"

SERVER=""
cleanup() {
    [ -n "$SERVER" ] && kill "$SERVER" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

# 进程的 CPU 时间（秒），`ps` 的 `cputime` 是 [[dd-]hh:]mm:ss 的格式。
cpu_seconds() {
    ps -o cputime= -p "$1" 2>/dev/null | python3 -c '
import sys
text = sys.stdin.read().strip()
if not text:
    print("nan")
    raise SystemExit
days = 0
if "-" in text:
    d, text = text.split("-", 1)
    days = int(d)
parts = [float(p) for p in text.split(":")]
total = 0.0
for p in parts:
    total = total * 60 + p
print(total + days * 86400)
'
}

run_one() {
    local scene=$1 name=$2 islands=$3 threads=$4 round=$5
    local dir="$OUT/$scene/$name-t$threads/round-$round"
    rm -rf "$dir"
    mkdir -p "$dir"
    local spec bots pattern layout seconds
    spec=$("$BIN" describe "$scene") || return 1
    read -r bots pattern layout seconds <<< "$spec"
    echo "=== $scene $name-t$threads round $round ($(uptime | sed 's/.*load averages*: //')) ==="
    uptime > "$dir/load-before.txt"
    local heavy=()
    [ -n "${HEAVY_ITEMS:-}" ] && heavy=(BAMBOO_LOADTEST_HEAVY_ITEMS="$HEAVY_ITEMS")
    (cd "$dir" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_ISLANDS="$islands" \
        BAMBOO_ISLAND_THREADS="$threads" \
        BAMBOO_LOADTEST_WORLD_COMMAND_EVERY="$WORLD_COMMAND_EVERY" ${heavy[@]+"${heavy[@]}"} \
        "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 \
        --view-distance 8 --scenario "$scene" > server.log 2>&1) &
    SERVER=$!
    local ready=0
    for _ in $(seq 1 1500); do
        grep -q "bench scenario ready" "$dir/server.log" 2>/dev/null && { ready=1; break; }
        kill -0 "$SERVER" 2>/dev/null || break
        sleep 0.2
    done
    [ "$ready" = 1 ] || { echo "$scene/$name: scenario not ready" >&2; kill "$SERVER"; wait "$SERVER" 2>/dev/null; SERVER=""; return 1; }
    local cpu_start wall_start
    cpu_start=$(cpu_seconds "$SERVER")
    wall_start=$(python3 -c 'import time; print(time.time())')
    "$BIN" --addr "127.0.0.1:$PORT" --bots "$bots" --pattern "$pattern" \
        --layout "$layout" --duration "$DURATION" --ramp 2 \
        --json "$dir/client.json" > "$dir/client.log" 2>&1 || true
    local cpu_end wall_end
    cpu_end=$(cpu_seconds "$SERVER")
    wall_end=$(python3 -c 'import time; print(time.time())')
    echo "$cpu_start $cpu_end $wall_start $wall_end" > "$dir/cpu.txt"
    uptime > "$dir/load-after.txt"
    sleep 1
    kill "$SERVER" 2>/dev/null
    wait "$SERVER" 2>/dev/null
    SERVER=""
}

for round in $(seq 1 "$ROUNDS"); do
    order=("${CONFIG_LIST[@]}")
    if [ $((round % 2)) -eq 0 ]; then
        order=()
        for ((i = ${#CONFIG_LIST[@]} - 1; i >= 0; i--)); do
            order+=("${CONFIG_LIST[$i]}")
        done
    fi
    for scene in "${SCENES[@]}"; do
        for config in "${order[@]}"; do
            IFS=: read -r name islands threads <<< "$config"
            run_one "$scene" "$name" "$islands" "$threads" "$round" || true
        done
    done
done

python3 "$TREE/tools/mc-bamboo-loadtest/summarize-islands-compare.py" "$OUT" "${SCENES[@]}"
