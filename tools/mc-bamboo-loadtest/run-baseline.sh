#!/bin/bash
# 一键跑基线：每个场景重启一次服务端，收集剖析报告和客户端统计，最后打印汇总表。
#
# 用法：run-baseline.sh [结果目录] [场景名...]
#   结果目录缺省是临时目录，脚本结束会删掉；传了目录就保留。
#   KEEP=1 时临时目录也保留。PORT 可以换端口，RAMP 可以换每个场景的连入时长。
#
# 服务端和压测工具是两个进程，不共用一个 tokio 运行时。五个场景都是离线模式、
# 压缩阈值 256、视距 8、剖析间隔 10 秒；只有人数上限从默认 20 提到 500。
set -u

TREE=$(cd "$(dirname "$0")/../.." && pwd)
BIN="$TREE/target/release/bamboo-loadtest"
PORT=${PORT:-25977}
RAMP=${RAMP:-5}
PROFILE_SECS=${PROFILE_SECS:-10}
KEEP=${KEEP:-}

if [ "$#" -ge 1 ] && [ -n "$1" ]; then
    OUT=$(mkdir -p "$1" && cd "$1" && pwd)
    KEEP=1
else
    OUT=$(mktemp -d /tmp/bamboo-baseline.XXXXXX)
fi
shift $(( $# >= 1 ? 1 : 0 ))
SCENES=("$@")
if [ "${#SCENES[@]}" -eq 0 ]; then
    SCENES=(idle crowd-50 crowd-200 walk-50 fly-20)
fi

echo "building bamboo-loadtest --release in $TREE"
(cd "$TREE" && cargo build --release -p mc-bamboo-loadtest) || exit 1

SERVER=""

stop_server() {
    if [ -n "$SERVER" ]; then
        kill "$SERVER" 2>/dev/null || true
        wait "$SERVER" 2>/dev/null || true
        SERVER=""
    fi
}
trap 'stop_server; if [ -z "$KEEP" ]; then rm -rf "$OUT"; fi' EXIT INT TERM

scene_spec() {
    case "$1" in
        idle)      echo "1 stand cluster 30" ;;
        crowd-50)  echo "50 wander cluster 60" ;;
        crowd-200) echo "200 wander cluster 60" ;;
        walk-50)   echo "50 walk spread 60" ;;
        fly-20)    echo "20 fly spread 60" ;;
        *) return 1 ;;
    esac
}

run_scene() {
    local name=$1 spec bots pattern layout seconds
    spec=$(scene_spec "$name") || { echo "unknown scenario: $name" >&2; return 1; }
    read -r bots pattern layout seconds <<< "$spec"
    local dir="$OUT/$name"
    mkdir -p "$dir"

    echo "=== $name: $bots bots, $pattern/$layout, ${seconds}s ==="
    # 服务端在临时目录里跑：四份管理名单和日志都落在那里，不在仓库里留文件。
    # 不用子 shell 包一层，否则 $! 是子 shell 的 pid，kill 不到真正的服务端。
    (cd "$dir" && exec env BAMBOO_PROFILE="$PROFILE_SECS" \
        "$BIN" serve --addr "127.0.0.1:$PORT" --max-players 500 --view-distance 8 \
        > server.log 2>&1) &
    SERVER=$!

    local ready=0
    for _ in $(seq 1 60); do
        if grep -q "loadtest server is listening" "$dir/server.log" 2>/dev/null; then
            ready=1
            break
        fi
        if ! kill -0 "$SERVER" 2>/dev/null; then
            echo "server exited before it was ready:" >&2
            tail -20 "$dir/server.log" >&2
            return 1
        fi
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        echo "server did not become ready" >&2
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

    # 留一点时间让最后一份剖析报告落到日志里，再关服。
    sleep 2
    stop_server
    cat "$dir/client.log"
    echo
    return "$client_status"
}

failed=0
for scene in "${SCENES[@]}"; do
    run_scene "$scene" || failed=1
done

echo "=== summary ==="
python3 "$TREE/tools/mc-bamboo-loadtest/summarize.py" "$OUT" "${SCENES[@]}"
echo "raw results: $OUT"
exit "$failed"
