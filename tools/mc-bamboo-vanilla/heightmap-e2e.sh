#!/usr/bin/env bash
# primed 高度图的端到端对拍：六张图各自故意写错，确认 Java 对拍能报出来。
# 覆盖报告见 docs/worldgen-e2e-coverage.md 的「primed 高度图覆盖」一节。
#
# End-to-end oracle for primed heightmaps: deliberately corrupt each of the six
# maps and require the Java oracle to report mismatches. See the coverage doc.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT" || exit 1
BIN=${BIN:-"$ROOT/target/release/mc-bamboo-vanilla"}
JAVA=${JAVA:-"${JAVA_HOME:+$JAVA_HOME/bin/java}"}
JAVA=${JAVA:-java}

if [ ! -x "$BIN" ]; then
    echo "缺少 $BIN，先跑 cargo build --release -p mc-bamboo-vanilla" >&2
    exit 1
fi

# 这批坐标同时触发读 WORLD_SURFACE(_WG)（bamboo、植被）、OCEAN_FLOOR(_WG)
# （ore、disk、surface_water_depth_filter）、MOTION_BLOCKING（freeze_top_layer、
# 高度图放置修饰器）和 MOTION_BLOCKING_NO_LEAVES（树叶藤蔓装饰器）的地物。
ARGS=(--through features --all-features --seeds 3 --chunks 8 --salt 7)
# 字段名|这一批坐标里确实被某个地物读到的那一列（单列改坏确认用）。
# 任意列不一定被读到：地物只在自己候选位置的列上问 getHeight，所以这里挑的是
# 实测会报不一致的列，见 docs/worldgen-e2e-coverage.md。
FIELDS=(
    "WORLD_SURFACE_WG|32"
    "WORLD_SURFACE|127"
    "OCEAN_FLOOR_WG|0"
    "OCEAN_FLOOR|0"
    "MOTION_BLOCKING|0"
    "MOTION_BLOCKING_NO_LEAVES|0"
)

failed=0
printf '%-30s %-8s %10s  %s\n' FIELD COLUMN MISMATCHES RESULT

clean=$("$BIN" gen-world "$JAVA" "${ARGS[@]}" 2>&1)
if ! printf '%s\n' "$clean" | grep -q '0 mismatches'; then
    echo "干净跑不是 0 不一致：" >&2
    printf '%s\n' "$clean" | tail -4 >&2
    exit 1
fi
echo "clean run: $(printf '%s\n' "$clean" | tail -1)"

for row in "${FIELDS[@]}"; do
    IFS='|' read -r field proven_column <<<"$row"
    # 两种改坏：整张图每列 +1，以及只把实测会被读到的那一列 +1。
    for column in all "$proven_column"; do
        out=$("$BIN" gen-world "$JAVA" "${ARGS[@]}" --break-heightmap "$field:$column" 2>&1)
        line=$(printf '%s\n' "$out" | tail -1)
        count=$(printf '%s\n' "$line" | sed -nE 's/.*detector works \(([0-9]+) mismatches\).*/\1/p')
        if [ -n "$count" ]; then
            printf '%-30s %-8s %10s  %s\n' "$field" "$column" "$count" PASS
        else
            printf '%-30s %-8s %10s  %s\n' "$field" "$column" 0 FAIL
            printf '%s\n' "$out" | tail -3 >&2
            failed=1
        fi
    done
done

if [ "$failed" -ne 0 ]; then
    echo "至少一张高度图没被对拍覆盖，见上面的 FAIL 行" >&2
    exit 1
fi
echo "所有六张 primed 高度图都被逐方块对拍覆盖（整体 +1 与单列写错各一组）"
