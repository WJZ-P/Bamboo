#!/usr/bin/env python3
"""汇总重负载基线：阶段占比、按区块分布、单元划分和 Amdahl 上限。

用法：summarize-heavy.py <结果目录> [场景名...]

每个场景有 ROUNDS 轮，每轮的 server.log 里有若干份 10 秒剖析报告。
只统计「搭场景已完成、在线人数达到目标」的稳态报告（按 loaded_chunks 达到
本轮最大值判断），首份报告通常还在交付区块，按预热丢掉。
每台机器上同时有别的任务在跑，绝对耗时噪声大，因此报告里的 p50/p95/p99
和阶段平均耗时取轮次中位数，占比一律在同一轮内部算完再取中位数。

chunk-phases 段的每行是「区块 x z 计划刻ns 方块事件ns 方块实体ns」，
是整份报告窗口的累计值。单元按设计稿 4.1：忙区块（三阶段合计 > 0）
按切比雪夫距离 ≤ 2 连成连通块。
"""

import json
import re
import statistics
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*m")
HEADER = re.compile(r"tick profile: (\d+) ticks")
PHASE = re.compile(
    r"^\s+(\S+)\s+count=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+"
    r"p95=\s*([\d.]+)\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)\s*$"
)
METRIC = re.compile(
    r"^\s+(\S+)\s+samples=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+"
    r"p95=\s*([\d.]+)\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)\s+unit=(\S+)\s*$"
)
CHUNK = re.compile(r"^\s+chunk (-?\d+) (-?\d+) (\d+) (\d+) (\d+)\s*$")

THREE = ("scheduled_ticks", "block_events", "block_entities")
ALL_PHASES = (
    "inbox", "delivery", "light_updates", "weather_and_time", "scheduled_ticks",
    "random_ticks", "block_events", "entities", "entity_compute", "entity_commit",
    "block_entities", "player_misc", "views", "block_change_broadcast",
    "entity_sync_and_tracking", "chunk_sending", "flush", "tick_tail",
)
THREADS = 8


def parse_reports(path):
    reports = []
    current = None
    # 场景搭好之前（真实地形预生成）的报告全是空服，不能算进稳态。
    after_ready = False
    for raw in path.read_text(errors="replace").splitlines():
        line = ANSI.sub("", raw)
        if "bench scenario ready" in line:
            after_ready = True
        header = HEADER.search(line)
        if header:
            current = {"ticks": int(header.group(1)), "phases": {}, "metrics": {},
                       "chunks": [], "section": "phases", "after_ready": after_ready}
            reports.append(current)
            continue
        if current is None:
            continue
        if line.startswith("phases (by mean)"):
            current["section"] = "phases"
            continue
        if line.startswith("metrics ("):
            current["section"] = "metrics"
            continue
        if line.startswith("sub-phases"):
            current["section"] = "subs"
            continue
        if line.startswith("chunk-phases"):
            current["section"] = "chunks"
            continue
        chunk = CHUNK.match(line)
        if chunk and current["section"] == "chunks":
            x, z, a, b, c = (int(v) for v in chunk.groups())
            current["chunks"].append((x, z, a, b, c))
            continue
        phase = PHASE.match(line)
        if phase and current["section"] == "phases":
            current["phases"][phase.group(1)] = [float(v) for v in phase.groups()[1:]]
            continue
        metric = METRIC.match(line)
        if metric and current["section"] == "metrics":
            current["metrics"][metric.group(1)] = [float(v) for v in metric.groups()[1:7]]
    return reports


def weighted(values, weights):
    total = sum(weights)
    if total == 0:
        return 0.0
    return sum(v * w for v, w in zip(values, weights)) / total


def units(chunks):
    """按切比雪夫 ≤2 把忙区块连成单元，返回每个单元的三阶段总耗时。"""
    busy = {c for c in chunks if sum(c[2:]) > 0}
    seen = set()
    sizes = []
    for start in sorted(busy):
        if start in seen:
            continue
        stack = [start]
        seen.add(start)
        group = []
        while stack:
            at = stack.pop()
            group.append(at)
            for other in busy:
                if other in seen:
                    continue
                if max(abs(other[0] - at[0]), abs(other[1] - at[1])) <= 2:
                    seen.add(other)
                    stack.append(other)
        sizes.append((len(group), sum(sum(c[2:]) for c in group)))
    # 理论上限取决于最贵的那个单元，不是最大的那个。
    return sorted(sizes, key=lambda group: group[1], reverse=True)


def analyse_report(report):
    phases = report["phases"]
    metrics = report["metrics"]
    total = metrics.get("tick_total")
    if not total or not total[0] or not report["after_ready"]:
        return None
    chunks = report["chunks"]
    three = sum(phases.get(name, [0, 0])[1] for name in THREE)
    grand = sum(sum(c[2:]) for c in chunks)
    active = [c for c in chunks if sum(c[2:]) > 0]
    ranked = sorted(active, key=lambda c: sum(c[2:]), reverse=True)
    tops = {}
    for n in (1, 4, 16):
        tops[n] = sum(sum(c[2:]) for c in ranked[:n]) / grand if grand else 0.0
    groups = units(chunks)
    biggest = groups[0][1] / grand if grand and groups else 0.0
    return {
        "samples": total[0],
        "p50": total[2], "p95": total[3], "p99": total[4],
        "tick_mean": total[1],
        "phases": {name: phases.get(name, [0, 0])[1] for name in ALL_PHASES},
        "three": three,
        "three_share": three / total[1] if total[1] else 0.0,
        "active_chunks": len(active),
        "tops": tops,
        "unit_count": len(groups),
        "largest_unit_chunks": groups[0][0] if groups else 0,
        "largest_unit_share": biggest,
        "loaded_chunks": metrics.get("loaded_chunks", [0, 0])[1],
        "online_players": metrics.get("online_players", [0, 0, 0, 0, 0, 0])[5],
    }


def median_or(values, default=0.0):
    return statistics.median(values) if values else default


def summarise(root, name):
    rounds = sorted((root / name).glob("round-*"))
    per_round = []
    for directory in rounds:
        log = directory / "server.log"
        if not log.exists():
            continue
        reports = parse_reports(log)
        client_path = directory / "client.json"
        target = 1
        if client_path.exists():
            target = json.loads(client_path.read_text()).get("bots", 1)
        parsed = [r for r in (analyse_report(x) for x in reports) if r]
        # 只留「场景已搭好 + 在线人数达到目标 90% + 区块全部交付」的报告；
        # 另外丢掉 ready 之后的第一份（可能跨了搭场景那一刻）。
        parsed = parsed[1:] or parsed
        peak = max((r["loaded_chunks"] for r in parsed), default=0)
        steady = [
            r for r in parsed
            if r["loaded_chunks"] >= 0.99 * peak and r["online_players"] >= 0.9 * target
        ]
        if not steady:
            steady = parsed
        if not steady:
            continue
        weights = [r["samples"] for r in steady]
        merged = {"samples": sum(weights)}
        for key in ("p50", "p95", "p99", "tick_mean", "three", "three_share",
                    "active_chunks", "unit_count", "largest_unit_share",
                    "largest_unit_chunks", "loaded_chunks", "online_players"):
            merged[key] = weighted([r[key] for r in steady], weights)
        merged["phases"] = {
            phase: weighted([r["phases"][phase] for r in steady], weights)
            for phase in ALL_PHASES
        }
        merged["tops"] = {
            n: weighted([r["tops"][n] for r in steady], weights) for n in (1, 4, 16)
        }
        # 阶段占比在同一轮内部算，避免跨轮绝对耗时噪声。
        merged["shares"] = {
            phase: merged["phases"][phase] / merged["tick_mean"] if merged["tick_mean"] else 0.0
            for phase in ALL_PHASES
        }
        # 8 线程、完美并行，但最大的单元是串行的：三阶段的下界是
        # max(总量 / 8, 最大单元量)，其余部分不变。
        parallel = max(merged["three"] / THREADS,
                       merged["three"] * merged["largest_unit_share"])
        serial = merged["tick_mean"] - merged["three"]
        new_total = serial + parallel
        merged["speedup"] = merged["tick_mean"] / new_total if new_total > 0 else 1.0
        merged["shortening"] = 1.0 - 1.0 / merged["speedup"] if merged["speedup"] > 0 else 0.0
        per_round.append(merged)
    if not per_round:
        return None
    out = {"scenario": name, "rounds": len(per_round)}
    for key in ("p50", "p95", "p99", "tick_mean", "three", "three_share",
                "active_chunks", "unit_count", "largest_unit_chunks",
                "largest_unit_share", "speedup", "shortening", "loaded_chunks"):
        out[key] = median_or([r[key] for r in per_round])
    out["tops"] = {
        n: median_or([r["tops"][n] for r in per_round]) for n in (1, 4, 16)
    }
    out["phases"] = {
        phase: median_or([round_["phases"][phase] for round_ in per_round])
        for phase in ALL_PHASES
    }
    out["shares"] = {
        phase: median_or([round_["shares"][phase] for round_ in per_round])
        for phase in ALL_PHASES
    }
    return out


def fmt(v, digits=3):
    return f"{v:.{digits}f}"


def main():
    root = Path(sys.argv[1])
    names = sys.argv[2:]
    if not names:
        names = sorted(p.name for p in root.iterdir() if p.is_dir())
    results = [r for r in (summarise(root, n) for n in names) if r]
    out = root / "heavy-summary.json"
    out.write_text(json.dumps(results, indent=2, ensure_ascii=False))

    print("== tick 与阶段（每场景取轮次中位数；括号里是占一刻的百分比）==")
    header = ("场景", "轮", "tick p50/p95/p99 (ms)", "tick mean", "三阶段合计", "占比")
    print(" | ".join(header))
    for r in results:
        print(" | ".join([
            r["scenario"], str(r["rounds"]),
            "/".join(fmt(r[k]) for k in ("p50", "p95", "p99")),
            fmt(r["tick_mean"]), fmt(r["three"]), f"{r['three_share'] * 100:.1f}%",
        ]))

    print()
    print("== 各阶段平均耗时 ms（括号：占一刻的百分比）==")
    for r in results:
        parts = []
        for phase in ALL_PHASES:
            value = r["phases"][phase]
            if value <= 0.0005:
                continue
            parts.append(f"{phase} {fmt(value)}({r['shares'][phase] * 100:.1f}%)")
        print(f"{r['scenario']}: " + ", ".join(parts))

    print()
    print("== 按区块分布 / 单元 / Amdahl 上限（8 线程，最大单元为串行段）==")
    print("场景 | 忙区块 | 单元数 | 最大单元区块数 | 最大单元占三阶段 | 前1/4/16区块 | 已加载区块 | 理论加速 | 最多缩短")
    for r in results:
        tops = "/".join(f"{r['tops'][n] * 100:.1f}%" for n in (1, 4, 16))
        print(" | ".join([
            r["scenario"], str(round(r["active_chunks"])), str(round(r["unit_count"])),
            str(round(r["largest_unit_chunks"])),
            f"{r['largest_unit_share'] * 100:.1f}%", tops,
            str(round(r["loaded_chunks"])), fmt(r["speedup"]),
            f"{r['shortening'] * 100:.1f}%",
        ]))
    print(f"\njson report: {out}")


if __name__ == "__main__":
    main()
