#!/usr/bin/env bash
# 崩溃测试：后台静默起 bamboo.exe → 官方探针改方块 → 等 D6 匀速保存 →
# 强杀进程 → 用官方 codec 检查留下的世界目录 → 重启 → 再跑一次探针确认世界能读。
#
# Repeatable crash test: start bamboo.exe silently, let the official probe place
# blocks, wait for D6 paced saves, kill -9, verify the left-behind world dir with
# the official codecs, restart, and run the probe again to confirm the world loads.
#
# 用法 / usage:
#   BAMBOO=<bamboo.exe 路径> PROBE_RUN=<服务端目录> \
#   JAVA=<java 路径> tools/crash-test.sh  （工具走 cargo run）
#
# PROBE_RUN 里要有 server.properties / bamboo.toml / ops.json；world/ 会被删掉重建。
# 不提交 ops.json，也不把 world/ 留在仓库里。

set -u

WT="$(cd "$(dirname "$0")/.." && pwd)"
BAMBOO="${BAMBOO:-$WT/target/debug/bamboo.exe}"
PROBE_RUN="${PROBE_RUN:-$WT/../probe-run}"

# 工具用 cargo run 起（直接跑二进制时 workspace_root()/collect_jars 偶发异常）。
# Use cargo run for the tool; running the binary directly can misbehave.
vanilla() {
    (cd "$WT" && cargo run -q -p mc-bamboo-vanilla -- "$@")
}
JAVA="${JAVA:-/c/Users/Administrator/.jdks/openjdk-26.0.1/bin/java}"
PORT1="${PORT1:-25660}"
PORT2="${PORT2:-25661}"
LOG_DIR="${LOG_DIR:-$WT/target/crash-test-logs}"
mkdir -p "$LOG_DIR"

pids=()
cleanup() {
    for pid in "${pids[@]:-}"; do
        kill -9 "$pid" >/dev/null 2>&1 || true
    done
}
trap cleanup EXIT

wait_ready() {
    local port=$1 pid=$2
    for _ in $(seq 1 120); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            echo "server $pid exited before becoming ready" >&2
            return 1
        fi
        if (exec 3<>"/dev/tcp/127.0.0.1/$port") >/dev/null 2>&1; then
            exec 3>&- 3<&-
            return 0
        fi
        sleep 0.25
    done
    echo "server on $port never became ready" >&2
    return 1
}

start() {
    local port=$1 log=$2
    (cd "$PROBE_RUN" && exec "$BAMBOO" "127.0.0.1:$port") >"$log" 2>&1 &
    local pid=$!
    pids+=("$pid")
    wait_ready "$port" "$pid" || return 1
    echo "$pid"
}

echo "== crash test: fresh world =="
rm -rf "$PROBE_RUN/world"
PID1=$(start "$PORT1" "$LOG_DIR/first.log") || exit 1
echo "first server pid=$PID1"

echo "== first official probe (places blocks) =="
vanilla probe 127.0.0.1 "$PORT1" "$JAVA" >"$LOG_DIR/probe1.log" 2>&1 || true
grep -E "PROBE RESULT|PROBE FAIL" "$LOG_DIR/probe1.log" || true

# 让 D6 匀速保存把脏区块写下去（默认每刻至少 1 个、单区块 10 秒冷却）。
echo "== wait for paced saves =="
sleep 5

echo "== force kill (simulate crash) =="
kill -9 "$PID1" >/dev/null 2>&1 || true
sleep 1
if kill -0 "$PID1" >/dev/null 2>&1; then
    echo "first server did not die" >&2
    exit 1
fi

echo "== world dir left behind by the crash =="
find "$PROBE_RUN/world" -name '*.mca' | head -10
MCA=$(find "$PROBE_RUN/world" -name '*.mca' | wc -l)
echo "region/entity .mca files: $MCA"

echo "== official world-dir verification of the crashed world =="
vanilla world-dir "$PROBE_RUN/world" "$JAVA" 2>&1 | tail -5 || {
    echo "world-dir verification FAILED" >&2
    exit 1
}

echo "== restart and second official probe =="
PID2=$(start "$PORT2" "$LOG_DIR/second.log") || exit 1
echo "second server pid=$PID2"
vanilla probe 127.0.0.1 "$PORT2" "$JAVA" >"$LOG_DIR/probe2.log" 2>&1 || true
grep -E "PROBE RESULT|PROBE FAIL" "$LOG_DIR/probe2.log" || true

echo "== stopping and cleaning up =="
kill -9 "$PID2" >/dev/null 2>&1 || true
rm -rf "$PROBE_RUN/world"
pids=()
echo "crash test finished; see $LOG_DIR"
