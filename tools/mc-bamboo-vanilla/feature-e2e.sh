#!/usr/bin/env bash
# 端到端逐类型对拍：官方在 JVM 里生成，Bamboo 经 Director 的同一条路径比较。
# 每个类型先跑干净对拍（必须 0 不一致），再跑改坏确认（必须报不一致）。
# 用法 / usage:
#   tools/mc-bamboo-vanilla/feature-e2e.sh [Feature 类型 ID ...]
# 不传参数跑全部类型；传参数只跑指定的几个。结果口径见 docs/worldgen-e2e-coverage.md。
#
# Per-type end-to-end oracle: vanilla generates in the JVM, Bamboo compares through
# the same code the Director runs. Each type gets a clean run (must be 0 mismatches)
# and a break confirmation (must report mismatches). See the coverage doc for the
# meaning of every column.
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

# 类型|step 掩码|能触发它的种子与区块。新地物按这个格式加一行。
# kind|step mask|seed/chunk arguments that actually trigger the type
CASES=(
    "minecraft:lake|1,2|--seeds 3 --chunks 8 --salt 7"
    "minecraft:geode|2|--seeds 3 --chunks 8 --salt 7"
    "minecraft:iceberg|2|--seeds 5 --chunk 146 -50 --scan 9"
    "minecraft:large_dripstone|2|--seeds 1 --chunk -53 -206 --scan 5"
    "minecraft:block_blob|2|--seeds 2 --chunk 83 -274 --scan 9"
    "minecraft:monster_room|3|--seeds 1 --chunk 3 -1 --scan 5"
    "minecraft:fossil|3|--seed 42 --chunk -267 -276 --scan 25"
    "minecraft:overlay|4|--seed 42 --chunk -267 -276 --scan 1"
    "minecraft:blue_ice|4|--seeds 1 --chunk 255 -109 --scan 5"
    "minecraft:spike|4|--seeds 1 --chunk 268 -300 --scan 3"
    "minecraft:disk|4,6|--seeds 3 --chunks 8 --salt 7"
    "minecraft:ore|6,7|--seeds 3 --chunks 8 --salt 7"
    "minecraft:underwater_magma|6|--seeds 3 --chunks 8 --salt 7"
    "minecraft:multiface_growth|7,9|--seeds 1 --chunk -42 111 --scan 4"
    "minecraft:speleothem_cluster|7|--seeds 1 --chunk -55 -200 --scan 9"
    "minecraft:sequence|1,7|--seeds 1 --chunk -42 111 --scan 3"
    "minecraft:simple_random_selector|7,9|--seeds 3 --chunks 12 --salt 13"
    "minecraft:spring_feature|8|--seeds 3 --chunks 8 --salt 7"
    "minecraft:bamboo|9|--seeds 1 --chunk -358 -372 --scan 9"
    "minecraft:block_column|9|--seeds 3 --chunks 8 --salt 7"
    "minecraft:random_boolean_selector|9|--seeds 3 --chunks 12 --salt 7"
    "minecraft:random_selector|9|--seeds 3 --chunks 8 --salt 7"
    "minecraft:simple_block|9|--seeds 1 --chunk 0 52 --scan 5"
    "minecraft:tree|9|--seeds 1 --chunk 13 -400 --scan 9"
    "minecraft:vegetation_patch|9|--seeds 3 --chunks 8 --salt 7"
    "minecraft:vines|9|--seeds 3 --chunks 8 --salt 7"
    "minecraft:weighted_random_selector|9|--seeds 3 --chunks 8 --salt 7"
    "minecraft:freeze_top_layer|10|--seeds 1 --chunk 255 -109 --scan 9"
)

# 全部类型一起跑一次整步对拍：这批坐标会触发读 primed 高度图的地物
# （freeze_top_layer / 高度图放置修饰器 / vegetation_patch / 树的地面检查），
# 是 heightmap-e2e.sh 改坏确认所用的同一片世界。
# Whole-step run with all types: these coordinates trigger the features that
# read primed heightmaps, the same world heightmap-e2e.sh breaks against.
ALL_ARGS="--seeds 3 --chunks 8 --salt 7"
all_out=$("$BIN" gen-world "$JAVA" --through features $ALL_ARGS --all-features 2>&1)
if ! printf '%s\n' "$all_out" | grep -q '0 mismatches'; then
    echo "all-features clean run FAILED: $(printf '%s\n' "$all_out" | tail -3 | tr '\n' ' ')" >&2
    exit 1
fi
echo "all-features clean run: $(printf '%s\n' "$all_out" | tail -1)"

selected=("$@")
printf '%-42s %-6s %8s %8s %10s  %s\n' TYPE STEPS CANDIDATES BROKEN MISMATCHES RESULT
failed=0
for row in "${CASES[@]}"; do
    IFS='|' read -r kind steps args <<<"$row"
    if [ ${#selected[@]} -gt 0 ]; then
        wanted=0
        for s in "${selected[@]}"; do
            [ "$s" = "$kind" ] && wanted=1
        done
        [ "$wanted" -eq 0 ] && continue
    fi
    # shellcheck disable=SC2086
    clean=$("$BIN" gen-world "$JAVA" --through features $args --steps "$steps" \
        --feature-types "$kind" --coverage 2>&1)
    clean_rc=$?
    candidates=$(printf '%s\n' "$clean" | awk -v k="$kind" '$2 == k {print $1; exit}')
    [ -z "$candidates" ] && candidates=0
    # shellcheck disable=SC2086
    broke=$("$BIN" gen-world "$JAVA" --through features $args --steps "$steps" \
        --feature-types "$kind" --break-type "$kind" 2>&1)
    break_rc=$?
    broken=$(printf '%s\n' "$broke" | sed -nE 's/.*\(([0-9]+) corrupted cells, ([0-9]+) mismatches\).*/\1 \2/p')
    broken_cells=${broken%% *}
    broken_mismatches=${broken##* }
    [ -z "$broken" ] && broken_cells=0 broken_mismatches=0
    result=PASS
    if [ "$clean_rc" -ne 0 ] || [ "$break_rc" -ne 0 ] || [ "$candidates" -eq 0 ] \
        || [ "$broken_cells" -eq 0 ] || [ "$broken_mismatches" -eq 0 ]; then
        result=FAIL
        failed=1
    fi
    printf '%-42s %-6s %8s %8s %10s  %s\n' \
        "$kind" "$steps" "$candidates" "$broken_cells" "$broken_mismatches" "$result"
    if [ "$result" = FAIL ]; then
        printf '  clean: %s\n' "$(printf '%s\n' "$clean" | tail -3 | tr '\n' ' ')"
        printf '  break: %s\n' "$(printf '%s\n' "$broke" | tail -3 | tr '\n' ' ')"
    fi
done
exit "$failed"
