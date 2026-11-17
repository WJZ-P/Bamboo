#!/bin/bash
# 重负载基线：造场景、按区块记三个阶段、每个场景跑 ROUNDS 轮。
#
# 用法：run-heavy.sh [结果目录] [场景名...]
#   结果目录缺省是临时目录，脚本结束删掉；传了目录就保留（KEEP=1 也保留）。
#   PORT（默认 25990）、RAMP（默认 5）、PROFILE_SECS（默认 10）、ROUNDS（默认 3）。
#   PARTITION（默认 1）：划分 / 越界统计开关，0 = 关闭（性能基线），2 = 再开全量对照。
#   结果目录按开关分开：<目录>/on 或 <目录>/off，同轮交替比较时分别跑两遍。
#   INTERLEAVE=1：按「轮」在外、「场景」在内的顺序跑（第 1 轮把每个场景各跑一遍，
#   再第 2 轮……），机器上有别的任务时同一轮里的场景之间才好比较。缺省按场景跑完再换。
#
# 场景由 bamboo-loadtest serve --scenario <名字> 在服务端进程里搭：固定的九台机器
# （tools/mc-bamboo-hit-rate）按固定网格摆好、整片区域走 /forceload 票正式交付；
# 不写存档、不启动真实的 Minecraft 服务端或客户端。每个场景一个全新进程，
# 剖析报告清零后再统计。机器忙时绝对耗时噪声大，所以用 ROUNDS 轮取中位数，
# 报告以占比和同轮内的相对量为主证据。
set -u

TREE=$(cd "$(dirname "$0")/../.." && pwd)
BIN="$TREE/target/release/bamboo-loadtest"
PORT=${PORT:-25990}
RAMP=${RAMP:-5}
PROFILE_SECS=${PROFILE_SECS:-10}
ROUNDS=${ROUNDS:-3}
KEEP=${KEEP:-}
# DURATION 只给冒烟测试用；正式测量用 describe 给出的场景时长。
DURATION=${DURATION:-}
PARTITION=${PARTITION:-1}

if [ "$#" -ge 1 ] && [ -n "$1" ]; then
    OUT=$(mkdir -p "$1" && cd "$1" && pwd)
    KEEP=1
else
    OUT=$(mktemp -d /tmp/bamboo-heavy.XXXXXX)
fi
shift $(( $# >= 1 ? 1 : 0 ))
SCENES=("$@")
if [ "${#SCENES[@]}" -eq 0 ]; then
    SCENES=(spread-4 spread-16 spread-64 big-16 big-64 spread-16-players \
            normal-20-walk normal-20-fly normal-50-walk normal-50-fly)
fi

# 开关不同不要混进同一份汇总；目录后缀让两遍测量的原始报告都留着。
if [ "$PARTITION" = "0" ]; then
    OUT="$OUT/off"
else
    OUT="$OUT/on"
fi
mkdir -p "$OUT"
KEEP=1

if [ "${SKIP_BUILD:-0}" != "1" ]; then
    echo "building bamboo-loadtest --release in $TREE"
    (cd "$TREE" && CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release -p mc-bamboo-loadtest) || exit 1
fi

SERVER=""
stop_server() {
    if [ -n "$SERVER" ]; then
        kill "$SERVER" 2>/dev/null || true
        wait "$SERVER" 2>/dev/null || true
        SERVER=""
    fi
}
cleanup() {
    stop_server
    if [ -z "$KEEP" ]; then
        rm -rf "$OUT"
    fi
}
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM
trap cleanup EXIT

run_one() {
    local name=$1
    local round=$2
    local dir="$OUT/$name/round-$round"
    mkdir -p "$dir"
    local spec bots pattern layout seconds
    spec=$("$BIN" describe "$name") || { echo "unknown scenario: $name" >&2; return 1; }
    read -r bots pattern layout seconds <<< "$spec"
    if [ -n "$DURATION" ]; then seconds=$DURATION; fi
    echo "=== $name round $round: $bots bots, $pattern/$layout, ${seconds}s ==="
    # 服务端在临时目录里跑：管理名单和日志都落在那里，不在仓库里留文件。
    # 第 0 步报告提醒：红石缓存和按区块统计不要一起用；这里同样只跑默认
    # 缓存关闭状态。PARTITION=0 时把这个变量设成 0，剖析器读环境变量时
    # 只认 1 / 2，其余值（含 0）都当成关闭。
    (cd "$dir" && exec env BAMBOO_PROFILE="$PROFILE_SECS" BAMBOO_PROFILE_CHUNKS=1 \
        BAMBOO_PROFILE_PARTITION="$PARTITION" \
        "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 --view-distance 8 \
        --scenario "$name" > server.log 2>&1) &
    SERVER=$!
    local ready=0
    for _ in $(seq 1 120); do
        if grep -q "loadtest server is listening" "$dir/server.log" 2>/dev/null; then
            ready=1
            break
        fi
        if ! kill -0 "$SERVER" 2>/dev/null; then
            echo "server exited before it was ready:" >&2
            tail -20 "$dir/server.log" >&2
            stop_server
            return 1
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        echo "server did not become ready" >&2
        stop_server
        return 1
    fi
    # 等场景搭好（强制加载的区块全部交付）再放人，否则前几秒量的是生成排队。
    local staged=0
    for _ in $(seq 1 600); do
        if grep -q "bench scenario ready" "$dir/server.log" 2>/dev/null; then
            staged=1
            break
        fi
        if ! kill -0 "$SERVER" 2>/dev/null; then
            echo "server exited before the scenario was built:" >&2
            tail -20 "$dir/server.log" >&2
            stop_server
            return 1
        fi
        sleep 0.5
    done
    if [ "$staged" -ne 1 ]; then
        echo "scenario did not become ready in 300s" >&2
        stop_server
        return 1
    fi
    "$BIN" --addr "127.0.0.1:$PORT" --bots "$bots" --pattern "$pattern" \
        --layout "$layout" --duration "$seconds" --ramp "$RAMP" \
        --json "$dir/client.json" > "$dir/client.log" 2>&1
    local client_status=$?
    if [ "$client_status" -ne 0 ]; then
        echo "loadtest failed ($client_status):" >&2
        cat "$dir/client.log" >&2
    fi
    sleep 2
    stop_server
    return "$client_status"
}

failed=0
if [ "${INTERLEAVE:-0}" = "1" ]; then
    for round in $(seq 1 "$ROUNDS"); do
        for scene in "${SCENES[@]}"; do
            if ! run_one "$scene" "$round"; then
                failed=1
                stop_server
                break 2
            fi
        done
    done
else
    for scene in "${SCENES[@]}"; do
        for round in $(seq 1 "$ROUNDS"); do
            if ! run_one "$scene" "$round"; then
                failed=1
                stop_server
                break 2
            fi
        done
    done
fi

echo "=== summary ==="
python3 "$TREE/tools/mc-bamboo-loadtest/summarize-heavy.py" "$OUT" "${SCENES[@]}"
echo "raw results: $OUT"
exit "$failed"
