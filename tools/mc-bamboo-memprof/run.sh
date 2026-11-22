#!/usr/bin/env bash
# 用法 / usage:
#   tools/mc-bamboo-memprof/run.sh <结果目录> [端口] [profile|plain]
#
# 三个时刻各取一份读数：起服后空载、一名假玩家在线（视距取 server.properties）、
# 玩家离开一段时间之后。
#   profile（默认）：用 bamboo-memprof 起服，写 <名字>.os.txt / <名字>.raw / symbols.txt，
#                    再用 `bamboo-memprof report` 汇总成 <名字>.report.txt。
#   plain：          用不带分析器的 bamboo 起服，只从外面读工作集和私有提交量，
#                    写进 plain.txt。这才是真实服务端的进程级读数。
# 先编好二进制：
#   CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release \
#       -p mc-bamboo-memprof -p mc-bamboo-loadtest -p mc-bamboo
#
# Takes three readings: idle after startup, one synthetic player online, and a
# while after the player left. `profile` runs the server under bamboo-memprof
# and writes per-site snapshots plus owner reports; `plain` runs the ordinary
# bamboo binary and only samples working set and private commit from outside,
# which is the real process-level reading.
#
# 环境变量 / environment:
#   VIEW       server.properties 的 view-distance（默认 8）
#   SEED       level-seed（默认 20261121；种子固定，几次读数才可比）
#   SETTLE     起服后等多少秒再取空载读数（默认 20）
#   ONLINE     玩家在线多少秒（默认 90；在线读数在离开前 10 秒取）
#   AFTER      玩家离开后等多少秒（默认 120）
#   BAMBOO_TOML  写进 bamboo.toml 的内容（可选）
#   TRIM       plain 模式下设为 1：离开后再量一次强制换出工作集（EmptyWorkingSet）后的读数
#   BIN        二进制所在目录（默认 target/release）
set -u

TREE=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
PORT=${2:-25981}
MODE=${3:-profile}
VIEW=${VIEW:-8}
SETTLE=${SETTLE:-20}
ONLINE=${ONLINE:-90}
AFTER=${AFTER:-120}
BIN=${BIN:-"$TREE/target/release"}
SERVER_DIR="$OUT/server-$MODE"
CONTROL="$OUT/control"

rm -rf "$SERVER_DIR"
mkdir -p "$SERVER_DIR" "$CONTROL"
printf 'view-distance=%s\nonline-mode=false\nserver-port=%s\nlevel-seed=%s\n' \
    "$VIEW" "$PORT" "${SEED:-20261121}" \
    > "$SERVER_DIR/server.properties"
if [ -n "${BAMBOO_TOML:-}" ]; then
    printf '%s\n' "$BAMBOO_TOML" > "$SERVER_DIR/bamboo.toml"
fi

if [ "$MODE" = plain ]; then
    "$BIN/bamboo.exe" --server-dir "$SERVER_DIR" "127.0.0.1:$PORT" \
        > "$OUT/server-$MODE.log" 2>&1 &
else
    "$BIN/bamboo-memprof.exe" serve --server-dir "$SERVER_DIR" --control "$CONTROL" \
        "127.0.0.1:$PORT" > "$OUT/server-$MODE.log" 2>&1 &
fi
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null' EXIT
sleep 1
WINPID=$(cat "/proc/$SERVER/winpid")

# 取一份读数：profile 模式发 snap 命令并等文件写完，plain 模式从外面读进程计数器。
# One reading: `profile` sends a snap command and waits for the files; `plain`
# reads the process counters from outside.
reading() {
    if [ "$MODE" = plain ]; then
        powershell -NoProfile -Command "\$p = Get-Process -Id $WINPID; '{0}: working_set {1:N1} MB, private_commit {2:N1} MB, threads {3}' -f '$1', (\$p.WorkingSet64 / 1MB), (\$p.PrivateMemorySize64 / 1MB), \$p.Threads.Count" \
            | tee -a "$OUT/plain.txt"
        return
    fi
    echo "snap $1" > "$CONTROL/cmd"
    while [ -e "$CONTROL/cmd" ] || [ ! -s "$CONTROL/$1.raw" ]; do sleep 0.5; done
    sleep 2
    cat "$CONTROL/$1.os.txt"
}

[ "$MODE" = plain ] && : > "$OUT/plain.txt"
sleep "$SETTLE"
reading startup
"$BIN/bamboo-loadtest.exe" --addr "127.0.0.1:$PORT" --bots 1 --pattern stand \
    --duration "$ONLINE" > "$OUT/bot-$MODE.log" 2>&1 &
BOT=$!
sleep $((ONLINE - 10))
reading online
wait "$BOT"
sleep "$AFTER"
reading left

if [ "$MODE" = plain ] && [ "${TRIM:-0}" != 0 ]; then
    # 实验：玩家离开后把工作集强制换出，看用户看到的那部分内存能回收多少。
    # 只动本脚本自己起的那个进程。这次实验不改产品代码，只量收益。
    # 先隔 3 秒再读一次 `settled`，把「空闲缓存自己释放」和「强制换出」分开。
    # Experiment: force the working set out after the player leaves, to measure how
    # much of the memory the user sees can be reclaimed. Only touches the process this
    # script started; no product code changes, this is just a measurement. Read
    # `settled` three seconds later first, to separate the idle caches' own release
    # from the forced trim.
    sleep 3
    reading settled
    powershell -NoProfile -Command "Add-Type -Name W -Namespace P -MemberDefinition '[DllImport(\"psapi.dll\")] public static extern bool EmptyWorkingSet(IntPtr h);'; \$p = Get-Process -Id $WINPID; [P.W]::EmptyWorkingSet(\$p.Handle) | Out-Null" \
        | tee -a "$OUT/plain.txt"
    sleep 3
    reading trimmed
fi

if [ "$MODE" != plain ]; then
    # 实验：压实堆之后再取一份，看空闲块里有多少能退还给系统。
    # Experiment: one more reading after compacting the heaps.
    echo compact > "$CONTROL/cmd"
    sleep 5
    reading compacted
    echo quit > "$CONTROL/cmd"
    wait "$SERVER"
    for name in startup online left; do
        "$BIN/bamboo-memprof.exe" report "$CONTROL/$name.raw" "$CONTROL/symbols.txt" \
            --top 60 --stacks 60 > "$OUT/$name.report.txt"
    done
fi
