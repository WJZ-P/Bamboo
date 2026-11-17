#!/bin/bash
# 开关关闭时对 main 不慢于 3%：九台机器基准 + 第 1 步的三种计划刻形状基准，
# 每个二进制各跑 5 轮取最小值，严格按 A-B-A-B 交替，报告逐对比值中位数。
#
# 用法：compare-partition-micro.sh <base 工作树> <changed 工作树>
#   ROUNDS（默认 5）、TICKS（默认 4000）、OUT（默认 /tmp/bamboo-partition-micro）
set -u
BASE=${1:?base worktree}
CHANGED=${2:?changed worktree}
ROUNDS=${ROUNDS:-5}
TICKS=${TICKS:-4000}
OUT=${OUT:-/tmp/bamboo-partition-micro}
mkdir -p "$OUT"

run_cache() {
    local tree=$1
    (cd "$tree" && CARGO_PROFILE_RELEASE_DEBUG=0 \
        ./target/release/cache_bench "$TICKS" 1 2>/dev/null)
}

run_shapes() {
    local tree=$1
    (cd "$tree" && CARGO_PROFILE_RELEASE_DEBUG=0 \
        cargo test --release -q -p mc-bamboo-blocks ticks_bench -- --ignored --nocapture 2>/dev/null \
        | grep -E 'dense 16|medium 1024|sparse 4096|single point')
}

# 每轮一份原始输出，A 和 B 交替；最后用 python 逐对算比值。
for round in $(seq 1 "$ROUNDS"); do
    for side in a b; do
        if [ "$side" = "a" ]; then tree="$BASE"; else tree="$CHANGED"; fi
        run_cache "$tree" > "$OUT/cache-$round-$side.txt" &
        pid_cache=$!
        run_shapes "$tree" > "$OUT/shapes-$round-$side.txt" &
        pid_shapes=$!
        wait "$pid_cache" "$pid_shapes"
    done
done

python3 - "$OUT" "$ROUNDS" <<'PY'
import re, statistics, sys
from pathlib import Path
root = Path(sys.argv[1])
rounds = int(sys.argv[2])


def cache_values(path):
    out = {}
    for line in path.read_text().splitlines():
        m = re.match(r"(\S+): 关 ([\d.]+) ms", line)
        if m:
            out[m.group(1)] = float(m.group(2))
    return out


def shape_values(path):
    out = {}
    for line in path.read_text().splitlines():
        m = re.search(r"(.+?): batch (\d+) ns/tick, rolling (\d+) ns/tick", line)
        if m:
            out[m.group(1) + " batch"] = int(m.group(2))
            out[m.group(1) + " rolling"] = int(m.group(3))
    return out


print("== 九台机器（cache_bench 关缓存，逐对比值中位数）==")
names = sorted(cache_values(root / "cache-1-a.txt").keys())
ratios = []
for name in names:
    pairs = []
    for r in range(1, rounds + 1):
        a = cache_values(root / f"cache-{r}-a.txt").get(name)
        b = cache_values(root / f"cache-{r}-b.txt").get(name)
        if a and b:
            pairs.append(b / a)
    if pairs:
        ratios.extend(pairs)
        print(f"{name}: {statistics.median(pairs):.3f}  (逐对 {', '.join(f'{x:.3f}' for x in pairs)})")
print(f"合计逐对比值中位数: {statistics.median(ratios):.3f}")

print()
print("== 三种计划刻形状（ticks_bench，逐对比值中位数）==")
names = sorted(shape_values(root / "shapes-1-a.txt").keys())
ratios = []
for name in names:
    pairs = []
    for r in range(1, rounds + 1):
        a = shape_values(root / f"shapes-{r}-a.txt").get(name)
        b = shape_values(root / f"shapes-{r}-b.txt").get(name)
        if a and b:
            pairs.append(b / a)
    if pairs:
        ratios.extend(pairs)
        print(f"{name}: {statistics.median(pairs):.3f}  (逐对 {', '.join(f'{x:.3f}' for x in pairs)})")
print(f"合计逐对比值中位数: {statistics.median(ratios):.3f}")
PY
