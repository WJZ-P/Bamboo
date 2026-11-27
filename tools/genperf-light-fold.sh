#!/usr/bin/env bash
# 在同一轮交替运行 main 与精确折写的 Director 基准。
# Alternate main and the exact fold in the same Director benchmark round.
set -euo pipefail

main="${1:?Usage: genperf-light-fold.sh MAIN_BENCH EXACT_BENCH OUTPUT_DIR ROUND}"
exact="${2:?Expected path to exact benchmark}"
out="${3:?Expected output directory}"
round="${4:?Expected round number}"
[[ "$round" =~ ^[1-9][0-9]*$ ]] || { echo 'ROUND must be a positive integer' >&2; exit 2; }
mkdir -p -- "$out"

sample_cpu() {
    if command -v powershell.exe >/dev/null; then
        powershell.exe -NoProfile -NonInteractive -Command +            '(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor | Where-Object Name -eq "_Total").PercentProcessorTime' +            2>/dev/null | tr -d '\r\n' || true
    fi
}

run() {
    local label="$1" bench="$2" threads="$3"
    local prefix="$out/r${round}-t${threads}-$label"
    printf 'start=%s label=%s threads=%s cpu=%s%%\n' "$(date -u +'%FT%TZ')" "$label" "$threads" "$(sample_cpu)"
    "$bench" --chunks 5 --seed 2026 --threads "$threads" --label "r${round}-$label" \
        --dump-blocks "$prefix.blocks" >"$prefix.log" 2>&1 &
    wait "$!"
}

for threads in 1 4 8; do
    if (( round % 2 == 1 )); then
        run main "$main" "$threads"
        run exact "$exact" "$threads"
    else
        run exact "$exact" "$threads"
        run main "$main" "$threads"
    fi
    cmp "$out/r$round-t$threads-main.blocks" "$out/r$round-t$threads-exact.blocks"
    cmp "$out/r$round-t1-main.blocks" "$out/r$round-t$threads-exact.blocks"
    printf 'round=%s threads=%s block_bytes_equal=%s\n' "$round" "$threads" \
        "$(wc -c <"$out/r$round-t$threads-main.blocks")"
done
