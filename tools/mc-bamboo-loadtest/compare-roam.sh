#!/bin/bash
# 岛的实施计划第 11 步的调参：`islands-roam` 在几组分区器参数（或线程数）下交替跑，同一时段比较。
#
# 用法：compare-roam.sh <工作树>
#   CONFIGS（默认 "base: delay60:BAMBOO_ISLAND_SPLIT_DELAY=60 split3:BAMBOO_ISLAND_SPLIT_DISTANCE=3"）：
#     每项「名字:环境变量=值,环境变量=值」，名字后面为空就是缺省参数。
#   ROUNDS（默认 2）：单数轮按 CONFIGS 的顺序、双数轮倒过来（A-B-C-C-B-A）。
#   DURATION（默认 600 秒）、PROFILE_SECS（默认 30）、OUT（默认 /tmp/bamboo-roam）、VERIFY（默认 1：
#     只统计，不开每刻全量对照，耗时数字不带对照的开销）。
#
# Tuning for island plan step 11: alternate `islands-roam` under several
# partition parameter sets (or thread counts) in the same period. Each run is
# run-long.sh with the given environment; odd rounds go in CONFIGS order,
# even rounds reversed.
set -u
TREE=$(cd "${1:?worktree}" && pwd)
read -r -a CONFIG_LIST <<< "${CONFIGS:-base: delay60:BAMBOO_ISLAND_SPLIT_DELAY=60 split3:BAMBOO_ISLAND_SPLIT_DISTANCE=3}"
ROUNDS=${ROUNDS:-2}
export DURATION=${DURATION:-600}
export PROFILE_SECS=${PROFILE_SECS:-30}
export VERIFY=${VERIFY:-1}
OUT=${OUT:-/tmp/bamboo-roam}
mkdir -p "$OUT"

for round in $(seq 1 "$ROUNDS"); do
    order=("${CONFIG_LIST[@]}")
    if [ $((round % 2)) -eq 0 ]; then
        order=()
        for ((i = ${#CONFIG_LIST[@]} - 1; i >= 0; i--)); do
            order+=("${CONFIG_LIST[$i]}")
        done
    fi
    for config in "${order[@]}"; do
        name=${config%%:*}
        settings=${config#*:}
        vars=()
        if [ -n "$settings" ]; then
            IFS=, read -r -a vars <<< "$settings"
        fi
        env ${vars[@]+"${vars[@]}"} OUT="$OUT" \
            "$TREE/tools/mc-bamboo-loadtest/run-long.sh" "$TREE" "$name-round-$round" > /dev/null 2>&1
        echo "== $name round $round"
        grep -E '^(reports|tick_total|master_busy|island_busy|island_count|island_merge|island_split|partition|dropped|declared|process|server at)' \
            "$OUT/$name-round-$round/summary.txt"
    done
done
