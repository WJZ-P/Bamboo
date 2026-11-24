#!/usr/bin/env bash
# 全量重跑数据导出，断言生成文件逐字节没变。
# 前提：先跑过一次 `generate`，`target/vanilla` 里有官方报告和依赖 jar。
#
# Rerun the full data export and assert the generated files are unchanged.
# Requires a prior `generate` run so `target/vanilla` holds the official reports and jars.
#
# 用法 / usage: tools/check-generated.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 1

JAVA=${JAVA:-"${JAVA_HOME:+$JAVA_HOME/bin/java}"}
JAVA=${JAVA:-java}

export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only

# extract 用上次 generate 留下的报告，不重新跑官方数据生成器。
# extract reuses the reports left by generate; it does not rerun the official data generator.
REPORTS=target/vanilla/generated/reports
if [ ! -d "$REPORTS" ]; then
    echo "缺少 $REPORTS，先跑：cargo run -p mc-bamboo-vanilla -- generate <server.jar>" >&2
    exit 1
fi

# 生成文件全部由生成器产出：generated/ 目录，以及写进 mc-bamboo-worldgen 的丛林神殿指令表。
# Every generated file is a generator output, including the jungle temple table under mc-bamboo-worldgen.
GENERATED=(
    crates/mc-bamboo-data/src/generated
    crates/mc-bamboo-worldgen/src/structure/post_process/jungle_temple_ops.rs
)

if ! git diff --quiet -- "${GENERATED[@]}"; then
    echo "生成文件有未提交改动，先提交或还原再检查" >&2
    git diff --stat -- "${GENERATED[@]}" >&2
    exit 1
fi

# 跑两遍：一遍要和提交的文件一致，两遍之间也要一致，才能挡住 HashSet 这类不稳定顺序。
# Run twice: each pass must match the committed files, and the two passes must agree, so
# unstable iteration orders (e.g. HashSet) cannot slip through.
for pass in 1 2; do
    echo "第 $pass 遍全量导出 / full export pass $pass"
    cargo run -p mc-bamboo-vanilla --no-default-features -- extract "$JAVA"
    if ! git diff --quiet -- "${GENERATED[@]}"; then
        echo "第 $pass 遍导出改变了生成文件：生成器与生成文件不一致" >&2
        git diff --stat -- "${GENERATED[@]}" >&2
        exit 1
    fi
done

echo "生成文件与生成器一致（两遍全量导出无 diff）"
