#!/usr/bin/env python3
"""把客户端区块到达测量和服务端剖析报告汇成一张表。

用法：summarize-chunk-trace.py <结果目录> [结果目录...]

服务端每 10 秒打一份报告；只取在线人数峰值达到 1 的报告，按样本数加权算
每刻的数量指标。客户端一侧的延迟分位数直接取自 trace.json。
"""

import json
import re
import sys
from pathlib import Path

from summarize import parse_reports, weighted

HEADER = [
    "run", "front p50", "front p95", "all p50", "all p95", "near p50", "near p95",
    "lead p50", "lead p95", "gap p50", "gap p95", "arrived", "unarrived",
    "compute_in_flight", "compute_backlog", "chunks_sent/tick", "chunks_delivered/tick",
    "loaded_chunks", "chunk_latency_us p95", "tick p95",
]


def cell(value):
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.1f}"
    return str(value)


def quantile(report, key, field):
    group = report.get(key, {})
    return group.get(field)


def summarize(directory):
    trace = json.loads((directory / "trace.json").read_text())
    reports = [
        report
        for report in parse_reports(directory / "server.log")
        if "online_players" in report["metrics"]
        and report["metrics"]["online_players"][5] >= 0.9
    ]
    if not reports:
        reports = parse_reports(directory / "server.log")[1:]

    def metric(name, column):
        rows = [r["metrics"][name] for r in reports if name in r["metrics"]]
        return weighted([row[column] for row in rows], [row[0] for row in rows]) if rows else None

    return [
        directory.name,
        quantile(trace, "front_latency_ms", "p50"),
        quantile(trace, "front_latency_ms", "p95"),
        quantile(trace, "latency_ms", "p50"),
        quantile(trace, "latency_ms", "p95"),
        quantile(trace, "near_latency_ms", "p50"),
        quantile(trace, "near_latency_ms", "p95"),
        quantile(trace, "ahead_at_arrival_chunks", "p50"),
        quantile(trace, "ahead_at_arrival_chunks", "p95"),
        quantile(trace, "gap_chunks", "p50"),
        quantile(trace, "gap_chunks", "p95"),
        trace.get("chunks_arrived"),
        trace.get("chunks_unarrived"),
        metric("compute_in_flight", 1),
        metric("compute_backlog", 1),
        metric("chunks_sent", 1),
        metric("chunks_delivered", 1),
        metric("loaded_chunks", 1),
        metric("chunk_latency_us", 3),
        metric("tick_total", 3),
    ]


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    rows = [[cell(value) for value in summarize(Path(arg))] for arg in sys.argv[1:]]
    widths = [max(len(HEADER[i]), *(len(row[i]) for row in rows)) for i in range(len(HEADER))]
    print("| " + " | ".join(HEADER[i].ljust(widths[i]) for i in range(len(HEADER))) + " |")
    print("|" + "|".join("-" * (width + 2) for width in widths) + "|")
    for row in rows:
        print("| " + " | ".join(value.ljust(widths[i]) for i, value in enumerate(row)) + " |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
