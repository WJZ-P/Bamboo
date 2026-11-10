#!/usr/bin/env python3
"""把每个场景的服务端剖析报告和客户端统计汇总成一张表。

用法：summarize.py <结果目录> [场景名...]

服务端每 10 秒打一份报告、打完就清零样本，所以一行汇总值没法从直方图里精确合并：
百分比取各份报告按样本数加权的平均，最大值取各份报告的最大值；阶段和数量指标
按样本数加权平均。只统计报告里在线人数峰值达到目标 90% 以上的报告，
其余报告的值在文档里注明是「多份 10 秒报告按样本数加权的平均」。
"""

import json
import re
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*m")
PHASE = re.compile(
    r"^\s+(\S+)\s+count=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+"
    r"p95=\s*([\d.]+)\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)\s*$"
)
METRIC = re.compile(
    r"^\s+(\S+)\s+samples=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+"
    r"p95=\s*([\d.]+)\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)\s+unit=(\S+)\s*$"
)
HEADER = re.compile(r"tick profile: (\d+) ticks")


def parse_reports(path):
    reports = []
    current = None
    for raw in path.read_text(errors="replace").splitlines():
        line = ANSI.sub("", raw)
        header = HEADER.search(line)
        if header:
            current = {"ticks": int(header.group(1)), "phases": {}, "metrics": {}}
            reports.append(current)
            continue
        if current is None:
            continue
        phase = PHASE.match(line)
        if phase:
            name = phase.group(1)
            current["phases"][name] = [float(value) for value in phase.groups()[1:]]
            continue
        metric = METRIC.match(line)
        if metric:
            name = metric.group(1)
            current["metrics"][name] = [float(value) for value in metric.groups()[1:7]]
    return reports


def weighted(values, weights):
    total = sum(weights)
    if total == 0:
        return 0.0
    return sum(value * weight for value, weight in zip(values, weights)) / total


def metric_wmean(reports, name, column):
    selected = [report["metrics"][name] for report in reports if name in report["metrics"]]
    samples = [row[0] for row in selected]
    return weighted([row[column] for row in selected], samples)


def summarize(root, name):
    directory = Path(root) / name
    client = json.loads((directory / "client.json").read_text())
    reports = parse_reports(directory / "server.log")
    bots = client["bots"]
    # 只取在线人数峰值达到目标 90% 以上的报告，把空服和 ramp 排除掉。
    steady = [
        report
        for report in reports
        if "online_players" in report["metrics"]
        and report["metrics"]["online_players"][5] >= 0.9 * bots
    ]
    if not steady:
        steady = reports[1:] or reports
    total = [report["metrics"]["tick_total"] for report in steady if "tick_total" in report["metrics"]]
    jitter = [report["metrics"]["tick_jitter"] for report in steady if "tick_jitter" in report["metrics"]]
    weights = [row[0] for row in total]
    jweights = [row[0] for row in jitter]
    latency = [report["metrics"]["chunk_latency_ticks"] for report in steady
               if "chunk_latency_ticks" in report["metrics"]]
    latency_samples = [row[0] for row in latency]

    phase_names = set().union(*(report["phases"].keys() for report in steady)) if steady else set()
    phases = []
    for phase in phase_names:
        rows = [report["phases"][phase] for report in steady if phase in report["phases"]]
        mean = weighted([row[1] for row in rows], [row[0] for row in rows])
        phases.append((phase, mean))
    phases.sort(key=lambda item: (-item[1], item[0]))

    def pct(rows, column, w):
        return weighted([row[column] for row in rows], w)

    top = ", ".join(f"{phase} {mean:.3f}" for phase, mean in phases[:3])
    return {
        "scenario": name,
        "bots": bots,
        "reports": len(steady),
        "ticks": sum(weights),
        "p50": pct(total, 2, weights) if total else 0.0,
        "p95": pct(total, 3, weights) if total else 0.0,
        "p99": pct(total, 4, weights) if total else 0.0,
        "max": max((row[5] for row in total), default=0.0),
        "jitter_p50": pct(jitter, 2, jweights) if jitter else 0.0,
        "jitter_p99": pct(jitter, 4, jweights) if jitter else 0.0,
        "top": top,
        "chunks_per_tick": metric_wmean(steady, "chunks_sent", 1),
        "chunk_latency_p95": weighted([row[3] for row in latency], latency_samples)
        if sum(latency_samples) else None,
        "bytes_in": client["mean_bytes_in"],
        "drops": len(client["drops"]),
        "login_fails": sum(item["count"] for item in client["login_failures"]),
        "rtt_p95": client["rtt_p95_ms"],
    }


COLUMNS = [
    ("scenario", "scenario"),
    ("bots", "bots"),
    ("reports", "reports"),
    ("tick p50", "p50"),
    ("tick p95", "p95"),
    ("tick p99", "p99"),
    ("tick max", "max"),
    ("jitter p50", "jitter_p50"),
    ("jitter p99", "jitter_p99"),
    ("top-3 phases (mean ms)", "top"),
    ("chunks/tick", "chunks_per_tick"),
    ("chunk p95", "chunk_latency_p95"),
    ("client B/s", "bytes_in"),
    ("drops", "drops"),
    ("login fails", "login_fails"),
]


def format_cell(key, value):
    if value is None:
        return "n/a"
    if isinstance(value, float):
        if key in ("bytes_in", "chunks_per_tick"):
            return f"{value:.1f}"
        return f"{value:.3f}"
    return str(value)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    root = Path(sys.argv[1])
    names = sys.argv[2:] or sorted(
        path.name for path in root.iterdir() if (path / "client.json").exists()
    )
    rows = []
    for name in names:
        if not (root / name / "client.json").exists():
            print(f"missing client report for {name}", file=sys.stderr)
            continue
        rows.append(summarize(root, name))
    table = [[format_cell(key, row[key]) for _, key in COLUMNS] for row in rows]
    header = [title for title, _ in COLUMNS]
    widths = [max(len(header[i]), *(len(row[i]) for row in table)) for i in range(len(header))]
    print("| " + " | ".join(title.ljust(widths[i]) for i, title in enumerate(header)) + " |")
    print("|" + "|".join("-" * (widths[i] + 2) for i in range(len(header))) + "|")
    for row in table:
        print("| " + " | ".join(cell.ljust(widths[i]) for i, cell in enumerate(row)) + " |")
    (root / "summary.tsv").write_text(
        "\n".join(
            ["\t".join(header)] + ["\t".join(row) for row in table]
        ) + "\n"
    )
    (root / "summary.json").write_text(json.dumps(rows, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
