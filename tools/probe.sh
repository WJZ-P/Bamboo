#!/usr/bin/env bash
# 用法 / usage:
#   tools/probe.sh <工作树或仓库目录> <端口>
#   tools/probe.sh <工作树或仓库目录> <端口> --loaded
#
# 在临时目录里放 probe 需要的 ops.json 和离线的 server.properties，起这棵树编出来的
# bamboo，跑官方 probe，打印结果。路径全部从脚本位置和参数推出来；Java 从 JAVA_HOME
# 或 PATH 里找，不写死本机路径。
#
# --loaded 模式：先建档再重启，跑两遍完整 probe，共用同一个临时目录。
#   1) 用 `mc-bamboo-vanilla seed-world` 在 world/ 里写出官方格式的超平坦区块
#      （只有 region/，其它状态全是默认值），这就是「有存档的世界」。
#   2) 第一遍 probe 建档：正常关服（Ctrl+C）后玩家存档、level.dat、data/ 和
#      探针改过的区块都会落盘。
#   3) 第二遍之前把玩家存档、level.dat、data/ 等运行时状态清掉，并用第 1 步
#      那份干净的 region/ 覆盖回去，重启后再跑一遍 probe。
#      两遍都是「盘上已有区块、全部走异步读」，区块到达顺序的判据
#      （`nearest chunk comes first`、`first batch is the three by three ...`、
#      `chunk center is the spawn chunk`）会单独打印。
#
# 为什么第 3 步要恢复干净区块、只保留「盘上有 .mca」这个事实：官方 probe 的
# 全部断言是按全新世界校准的（出生点、正午、创造模式、每格方块、物品栏）。
# 第一遍会改时间、改 random_tick_speed、放方块、走动并留下玩家位置，直接拿它
# 重启会把这些无关差异算进来，看不出区块顺序有没有回归。把变量全部复位后，
# 只剩「区块在盘上」这一项差异，两遍都应该 718/718。崩溃测试
# tools/crash-test.sh 覆盖的是「带真实改动的世界重启后官方客户端能登录」。
#
# Run the official probe against a bamboo built from the given tree. --loaded
# seeds pristine on-disk chunks, runs a baseline, then restarts on the same
# directory with the runtime state reset and runs again. The chunk-arrival-order
# checks are printed separately. The probe is calibrated for a fresh world, so
# the loaded run keeps the world content pristine and only the "chunks already
# exist on disk" variable differs. Java comes from JAVA_HOME or PATH; no absolute
# local paths.
#
# 环境变量 / environment:
#   JAVA        java 可执行文件（默认 $JAVA_HOME/bin/java 或 PATH 里的 java）
#   KEEP        非空时保留临时目录（含各次的 server.log / probe.log）
#   PORT2       --loaded 第二遍的端口（默认 <端口>+1）
#   SEED_RADIUS seed-world 的半径（默认 8，覆盖视距 8 的跟踪范围）

set -u

usage() {
    sed -n '2,45p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

[ $# -ge 2 ] || usage
TREE=$(cd "$1" && pwd) || exit 1
PORT=$2
MODE=${3:-}
[ "$MODE" = "" ] || [ "$MODE" = "--loaded" ] || usage
PORT2=${PORT2:-$((PORT + 1))}
SEED_RADIUS=${SEED_RADIUS:-8}

ROOT=$(cd "$(dirname "$0")/.." && pwd)
if [ -n "${JAVA:-}" ]; then
    :
elif [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ]; then
    JAVA="$JAVA_HOME/bin/java"
else
    JAVA=java
fi
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only

TMP=$(mktemp -d "${TMPDIR:-/tmp}/bamboo-probe.XXXXXX")
WORLD="$TMP/world"
REGION="$WORLD/dimensions/minecraft/overworld/region"
PRISTINE="$TMP/pristine-region"
cat > "$TMP/ops.json" <<'JSON'
[{"uuid":"db80816f-ce8f-3340-b6e5-6defcb8c28ef","name":"Kitten","level":4,"bypassesPlayerLimit":false},{"uuid":"01bd0752-b5a4-34ce-9f9e-97b135904fcb","name":"Mochi","level":4,"bypassesPlayerLimit":false},{"uuid":"fc9b2232-d709-326b-b086-3b57b7ed6a68","name":"Latecomer","level":4,"bypassesPlayerLimit":false}]
JSON
# 读配置文件的 bamboo 会读这个目录里的 server.properties。这里写的值和
# ServerConfig::default() 一致（离线、视距 8、Bamboo 的 motd）；probe 的检查按这些值写。
cat > "$TMP/server.properties" <<'PROPS'
online-mode=false
motd=A Bamboo Server
view-distance=8
max-players=20
network-compression-threshold=256
enforce-secure-profile=true
prevent-proxy-connections=false
PROPS

(cd "$TREE" && cargo build -q --bin bamboo) || exit 1

start_server() {
    local port=$1 name=$2
    (cd "$TMP" && exec "$TREE/target/debug/bamboo" "127.0.0.1:$port") \
        > "$TMP/server-$name.log" 2>&1 &
    SERVER_PID=$!
    sleep 3
}

# 正常关服：SIGINT 才会让 world 走 stopServer 把 level.dat / data/ / 区块写完。
stop_server() {
    local pid=$1 name=$2
    kill -INT "$pid" 2>/dev/null
    local i
    for i in $(seq 1 300); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then break; fi
        sleep 0.1
    done
    if kill -0 "$pid" >/dev/null 2>&1; then
        echo "bamboo 没有在 30 秒内正常关服，见 $TMP/server-$name.log" >&2
        kill -9 "$pid" 2>/dev/null
        return 1
    fi
    wait "$pid" 2>/dev/null
    return 0
}

probe_once() {
    local name=$1 port=$2
    (cd "$TREE" && cargo run -q -p mc-bamboo-vanilla -- \
        probe 127.0.0.1 "$port" "$JAVA") > "$TMP/probe-$name.log" 2>&1
    local status=$?
    echo "== probe $name =="
    grep "PROBE FAIL" "$TMP/probe-$name.log" | head -20
    grep "PROBE RESULT" "$TMP/probe-$name.log" || {
        echo "probe 没有给出结果，日志尾部：";
        tail -20 "$TMP/probe-$name.log";
        return 1;
    }
    return "$status"
}

order_checks() {
    local name=$1
    echo "-- $name 的区块到达顺序检查 / chunk arrival order --"
    grep -E "PROBE (ok|FAIL)   play: (nearest chunk comes first|first batch is the three by three around the spawn|chunk center is the spawn chunk)" \
        "$TMP/probe-$name.log"
}

# 把 world/ 复位成「盘上有干净的模板区块、其它状态全新」。
reset_to_pristine_saved_world() {
    rm -rf "$WORLD/players" "$WORLD/data" \
        "$WORLD/dimensions/minecraft/overworld/data" \
        "$WORLD/dimensions/minecraft/overworld/entities" "$REGION"
    rm -f "$WORLD/level.dat" "$WORLD/level.dat_old"
    mkdir -p "$REGION"
    cp -R "$PRISTINE"/. "$REGION"/
}

status=0
if [ "$MODE" = "--loaded" ]; then
    rm -rf "$WORLD" "$PRISTINE"
    (cd "$TREE" && cargo run -q -p mc-bamboo-vanilla -- \
        seed-world "$WORLD" "$SEED_RADIUS") || exit 1
    mkdir -p "$PRISTINE"
    cp -R "$REGION"/. "$PRISTINE"/
    echo "== seed: $(find "$PRISTINE" -name '*.mca' | wc -l | tr -d ' ') 个区域文件已写入 =="

    # 第一遍：建档。关服会写出 level.dat / data/ / players/ 和改过的区块。
    start_server "$PORT" fresh
    probe_once fresh "$PORT" || status=1
    stop_server "$SERVER_PID" fresh || status=1
    order_checks fresh

    # 第二遍：同一个 world/ 重启，只保留「区块在盘上」这一项差异。
    reset_to_pristine_saved_world
    start_server "$PORT2" loaded
    probe_once loaded "$PORT2" || status=1
    stop_server "$SERVER_PID" loaded || status=1
    order_checks loaded
else
    start_server "$PORT" fresh
    probe_once fresh "$PORT" || status=1
    stop_server "$SERVER_PID" fresh || status=1
fi

if [ -n "${KEEP:-}" ]; then
    echo "日志和世界目录保留在 $TMP"
else
    rm -rf "$TMP"
fi
rm -rf "$TREE/crates/mc-bamboo-protocol/logs"
exit "$status"
