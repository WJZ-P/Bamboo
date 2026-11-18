#!/bin/bash
# 岛的实施计划第 7 步：分离驱动（岛在 tick 线程上）对齐步（内联）的同轮 A-B-A 比较。
#
# 用法：compare-island-threads.sh <工作树> [场景...]
#   缺省场景 islands-1 normal-20-walk spread-16。
#   ROUNDS（默认 3）、DURATION（默认 20 秒）、PROFILE_SECS（默认 5）、PORT（默认 25994）。
#   A_THREADS（默认 0 = 齐步）、B_THREADS（默认 1 = 分离）：两边的 BAMBOO_ISLAND_THREADS。
#   A/A 对照把两个都设成同一个值。
#   比的是一个岛时的两种驱动，所以两边都显式关掉多岛（BAMBOO_ISLANDS=0），不随缺省变。
#
# 同一个二进制，只换环境变量。每一对里按 A-B-A 顺序各跑一个全新进程，丢掉第一份剖析
# 报告（搭场景），其余报告取中位数；逐轮报 B 对两次 A 中位数的比值。另外报分离那一遍
# 每刻的主控线程耗时（master_busy）、岛线程耗时（island_busy）和交接耗时（island_handoff）。
#
# A-B-A comparison of the detached drive (island on a tick thread) against
# lockstep (inline), same binary, only the environment differs. Prints the
# per-round B / median(A1, A2) ratios of tick_total, and the detached run's
# per-tick master, island and hand-over times.
set -u
TREE=$(cd "${1:?worktree}" && pwd)
shift
SCENES=("$@")
[ "${#SCENES[@]}" -eq 0 ] && SCENES=(islands-1 normal-20-walk spread-16)
ROUNDS=${ROUNDS:-3}
DURATION=${DURATION:-20}
PROFILE_SECS=${PROFILE_SECS:-5}
PORT=${PORT:-25994}
A_THREADS=${A_THREADS:-0}
B_THREADS=${B_THREADS:-1}
OUT=${OUT:-/tmp/bamboo-island-threads}
BIN="$TREE/target/release/bamboo-loadtest"
mkdir -p "$OUT"

SERVER=""
cleanup() {
    [ -n "$SERVER" ] && kill "$SERVER" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

run_one() {
    local scene=$1 tag=$2 threads=$3
    local dir="$OUT/$scene/$tag"
    mkdir -p "$dir"
    local spec bots pattern layout seconds
    spec=$("$BIN" describe "$scene") || return 1
    read -r bots pattern layout seconds <<< "$spec"
    (cd "$dir" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_ISLANDS=0 BAMBOO_ISLAND_THREADS="$threads" \
        "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 \
        --view-distance 8 --scenario "$scene" > server.log 2>&1) &
    SERVER=$!
    local ready=0
    for _ in $(seq 1 600); do
        grep -q "bench scenario ready" "$dir/server.log" 2>/dev/null && { ready=1; break; }
        kill -0 "$SERVER" 2>/dev/null || break
        sleep 0.2
    done
    [ "$ready" = 1 ] || { echo "$scene/$tag: scenario not ready" >&2; kill "$SERVER"; SERVER=""; return 1; }
    "$BIN" --addr "127.0.0.1:$PORT" --bots "$bots" --pattern "$pattern" \
        --layout "$layout" --duration "$DURATION" --ramp 2 \
        --json "$dir/client.json" > "$dir/client.log" 2>&1 || true
    sleep 1
    kill "$SERVER" 2>/dev/null
    wait "$SERVER" 2>/dev/null
    SERVER=""
}

# 一份 server.log 里 `name` 指标的（mean, p50, p95），丢掉第一份报告后取各列中位数。
metric() {
    python3 - "$1" "$2" <<'PY'
import re, statistics, sys
text = open(sys.argv[1], errors="replace").read()
rows = re.findall(r"^\s+%s\s+samples=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+p95=\s*([\d.]+)" % re.escape(sys.argv[2]), text, re.M)
rows = [r for r in rows if int(r[0]) > 0]
rows = rows[1:] or rows
if not rows:
    print("- - -")
else:
    print(" ".join(f"{statistics.median(float(r[i]) for r in rows):.4f}" for i in (1, 2, 3)))
PY
}

echo "scene,round,A1_mean,B_mean,A2_mean,ratio_mean,ratio_p50,B_master_busy_mean,B_island_busy_mean,B_handoff_mean,B_handoff_p50,B_handoff_p95"
for scene in "${SCENES[@]}"; do
    for round in $(seq 1 "$ROUNDS"); do
        run_one "$scene" "r$round-a1" "$A_THREADS" || continue
        run_one "$scene" "r$round-b" "$B_THREADS" || continue
        run_one "$scene" "r$round-a2" "$A_THREADS" || continue
        read -r a1m a1p _ <<< "$(metric "$OUT/$scene/r$round-a1/server.log" tick_total)"
        read -r bm bp _ <<< "$(metric "$OUT/$scene/r$round-b/server.log" tick_total)"
        read -r a2m a2p _ <<< "$(metric "$OUT/$scene/r$round-a2/server.log" tick_total)"
        read -r mb _ _ <<< "$(metric "$OUT/$scene/r$round-b/server.log" master_busy)"
        read -r ib _ _ <<< "$(metric "$OUT/$scene/r$round-b/server.log" island_busy)"
        read -r hm hp h95 <<< "$(metric "$OUT/$scene/r$round-b/server.log" island_handoff)"
        python3 - "$scene" "$round" "$a1m" "$bm" "$a2m" "$a1p" "$bp" "$a2p" "$mb" "$ib" "$hm" "$hp" "$h95" <<'PY'
import statistics, sys
scene, rnd, a1m, bm, a2m, a1p, bp, a2p, mb, ib, hm, hp, h95 = sys.argv[1:]
f = float
am = statistics.median([f(a1m), f(a2m)])
ap = statistics.median([f(a1p), f(a2p)])
print(f"{scene},{rnd},{f(a1m):.3f},{f(bm):.3f},{f(a2m):.3f},{f(bm)/am:.3f},{f(bp)/ap:.3f},{mb},{ib},{hm},{hp},{h95}")
PY
    done
done
