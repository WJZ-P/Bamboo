#!/bin/bash
# 开关关闭时对 main（第 1 步结束时）的性能不变：同轮交替的 A-B-A 比较。
#
# 用法：compare-partition-overhead.sh <base 工作树> <changed 工作树> [轮数]
#   PORT（默认 25996）、PROFILE_SECS（默认 6）、DURATION（默认 12 秒）、ROUNDS（默认 5）。
#
# 默认 A = base 的二进制，B = changed 的二进制且 BAMBOO_PROFILE_PARTITION=0，
# 用来验证开关关闭时不比 main 慢。把 A_EXTRA 设成 BAMBOO_PROFILE_PARTITION=1、
# B_EXTRA 设成 0、两个路径都指向 changed，就变成「同轮交替量开关打开慢多少」。
# 每一对里按 A-B-A 顺序各跑一次，逐对比值中位数取 B/A；绝对值不作证据。
set -u
BASE=${1:?base worktree}
CHANGED=${2:?changed worktree}
ROUNDS=${ROUNDS:-5}
PORT=${PORT:-25996}
PROFILE_SECS=${PROFILE_SECS:-6}
DURATION=${DURATION:-12}
OUT=${OUT:-/tmp/bamboo-partition-ab}
mkdir -p "$OUT"
A="$BASE/target/release/bamboo-loadtest"
B="$CHANGED/target/release/bamboo-loadtest"

run_one() {
    local tag=$1
    local bin=$2
    local extra=$3
    local dir="$OUT/$tag"
    mkdir -p "$dir"
    local spec bots pattern layout seconds
    spec=$("$bin" describe spread-16) || return 1
    read -r bots pattern layout seconds <<< "$spec"
    (cd "$dir" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_PROFILE_CHUNKS=1 \
        $extra "$bin" serve --addr "127.0.0.1:$PORT" --max-players 500 \
        --view-distance 8 --scenario spread-16 > server.log 2>&1) &
    SERVER=$!
    local ready=0
    for _ in $(seq 1 120); do
        grep -q "loadtest server is listening" "$dir/server.log" 2>/dev/null && { ready=1; break; }
        kill -0 "$SERVER" 2>/dev/null || break
        sleep 0.2
    done
    [ "$ready" = 1 ] || { echo "$tag: server not ready" >&2; kill "$SERVER" 2>/dev/null; return 1; }
    local staged=0
    for _ in $(seq 1 300); do
        grep -q "bench scenario ready" "$dir/server.log" 2>/dev/null && { staged=1; break; }
        kill -0 "$SERVER" 2>/dev/null || break
        sleep 0.5
    done
    [ "$staged" = 1 ] || { echo "$tag: scenario not staged" >&2; kill "$SERVER" 2>/dev/null; return 1; }
    "$bin" --addr "127.0.0.1:$PORT" --bots "$bots" --pattern "$pattern" \
        --layout "$layout" --duration "$DURATION" --ramp 2 \
        --json "$dir/client.json" > "$dir/client.log" 2>&1 || true
    sleep 2
    kill "$SERVER" 2>/dev/null || true
    wait "$SERVER" 2>/dev/null || true
    SERVER=""
}

extract() {
    local log=$1
    python3 - "$log" <<'PY'
import re, sys, statistics
text = open(sys.argv[1], errors="replace").read()
vals = re.findall(r"^\s+tick_total\s+samples=\d+\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+p95=\s*([\d.]+)", text, re.M)
# 丢掉第一份（搭场景），余下取中位数。
vals = vals[1:] or vals
print(" ".join(str(statistics.median(float(v[i]) for v in vals)) for i in (1, 2)) if vals else "- -")
PY
}

EXTRA_A=${A_EXTRA:-}
echo "round,A_p50,A_p95,B_p50,B_p95,ratio_p50,ratio_p95"
for round in $(seq 1 "$ROUNDS"); do
    run_one "r${round}-a1" "$A" "$EXTRA_A" || continue
    run_one "r${round}-b" "$B" "${B_EXTRA:-BAMBOO_PROFILE_PARTITION=0}" || continue
    run_one "r${round}-a2" "$A" "$EXTRA_A" || continue
    read -r a1p50 a1p95 <<< "$(extract "$OUT/r${round}-a1/server.log")"
    read -r a2p50 a2p95 <<< "$(extract "$OUT/r${round}-a2/server.log")"
    read -r bp50 bp95 <<< "$(extract "$OUT/r${round}-b/server.log")"
    python3 - "$round" "$a1p50" "$a2p50" "$bp50" "$a1p95" "$a2p95" "$bp95" <<'PY'
import sys, statistics
r = sys.argv[1]
vals = [float(x) for x in sys.argv[2:]]
a1, a2, b = vals[0], vals[1], vals[2]
a = statistics.median([a1, a2])
i1, i2, ib = vals[3], vals[4], vals[5]
ai = statistics.median([i1, i2])
print(f"{r},{a1:.3f}/{a2:.3f},{i1:.3f}/{i2:.3f},{b:.3f},{ib:.3f},{b/a:.3f},{ib/ai:.3f}" if a and ai else "-")
PY
done
