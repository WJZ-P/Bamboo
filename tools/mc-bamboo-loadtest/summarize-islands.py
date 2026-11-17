#!/usr/bin/env python3
"""汇总「岛」基线：K 组相距很远的玩家 + 机器 + 掉落物，现在单线程一个 Region。

用法：summarize-islands.py <结果目录>（run-heavy.sh 写出的 on/ 或 off/ 目录）

每个 islands-K 场景先用 summarize-heavy.py 的同一套稳态筛选（场景搭好、人数到
目标 90%、已加载区块到本轮峰值、丢掉 ready 后第一份）算出阶段平均耗时，
再另外读每份报告的 sub-phases，取 ticket_update。

「K 组各自独立跑」的理论上限按同一轮（INTERLEAVE=1 时同一轮里的各场景是挨着跑的）
里的 islands-1 当「一组单独跑」：
    主控串行段 S = delivery + weather_and_time + tick_tail + ticket_update
    其余是跟着岛走的部分 P = T − S
    K 个岛在 ≥ K 条线程上完美并行时每刻约 S_K + P_1，上限 = T_K / (S_K + P_1)
没扣线程调度、跨岛消息、合并拆分的开销，是乐观上界。
另外按 chunk-phases 的区块 x 坐标把三阶段耗时分到各组，看各组是否均衡。

Island baselines: K far-apart groups of bots with machines and items, measured on
today's single-threaded single region. The independent-run bound uses islands-1 of
the same round as "one group alone".
"""

import importlib.util
import json
import re
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("heavy", HERE / "summarize-heavy.py")
heavy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(heavy)

ANSI = re.compile(r"\x1b\[[0-9;]*m")
HEADER = re.compile(r"tick profile: (\d+) ticks")
# 和 scenario.rs 的 GROUP_STRIDE_CHUNKS 一致；组原点在 x = g × 64 个区块。
GROUP_STRIDE_CHUNKS = 64
SERIAL_PHASES = ("delivery", "weather_and_time", "tick_tail")


def sub_phases(path):
    """每份报告的 sub-phases 平均值，和 parse_reports 的顺序一一对应。"""
    out = []
    current = None
    section = None
    for raw in path.read_text(errors="replace").splitlines():
        line = ANSI.sub("", raw)
        if HEADER.search(line):
            current = {}
            out.append(current)
            section = None
            continue
        if current is None:
            continue
        if line.startswith("sub-phases"):
            section = "subs"
            continue
        if line.startswith("chunk-phases") or line.startswith("metrics ("):
            section = None
            continue
        match = heavy.PHASE.match(line)
        if match and section == "subs":
            current[match.group(1)] = float(match.group(3))
    return out


def round_summary(directory):
    log = directory / "server.log"
    if not log.exists():
        return None
    reports = heavy.parse_reports(log)
    subs = sub_phases(log)
    client = directory / "client.json"
    target = json.loads(client.read_text()).get("bots", 1) if client.exists() else 1
    rows = []
    for report, sub in zip(reports, subs):
        parsed = heavy.analyse_report(report)
        if parsed:
            rows.append((parsed, sub, report["chunks"]))
    rows = rows[1:] or rows
    peak = max((r[0]["loaded_chunks"] for r in rows), default=0)
    steady = [
        r for r in rows
        if r[0]["loaded_chunks"] >= 0.99 * peak and r[0]["online_players"] >= 0.9 * target
    ] or rows
    if not steady:
        return None
    weights = [r[0]["samples"] for r in steady]
    tick = heavy.weighted([r[0]["tick_mean"] for r in steady], weights)
    phases = {
        name: heavy.weighted([r[0]["phases"][name] for r in steady], weights)
        for name in heavy.ALL_PHASES
    }
    ticket = heavy.weighted([r[1].get("ticket_update", 0.0) for r in steady], weights)
    serial = sum(phases[name] for name in SERIAL_PHASES) + ticket
    groups = {}
    for _, _, chunks in steady:
        for x, _z, a, b, c in chunks:
            group = (x + 16) // GROUP_STRIDE_CHUNKS
            groups[group] = groups.get(group, 0) + a + b + c
    return {
        "tick": tick,
        "p50": heavy.weighted([r[0]["p50"] for r in steady], weights),
        "p95": heavy.weighted([r[0]["p95"] for r in steady], weights),
        "p99": heavy.weighted([r[0]["p99"] for r in steady], weights),
        "phases": phases,
        "ticket_update": ticket,
        "serial": serial,
        "loaded": heavy.weighted([r[0]["loaded_chunks"] for r in steady], weights),
        "group_three": groups,
    }


def main():
    root = Path(sys.argv[1])
    scenes = sorted(
        (p.name for p in root.iterdir() if p.is_dir() and p.name.startswith("islands-")),
        key=lambda name: int(name.split("-")[1]),
    )
    per = {}
    for name in scenes:
        per[name] = {
            d.name: s for d in sorted((root / name).glob("round-*"))
            if (s := round_summary(d))
        }
    base = per.get("islands-1", {})
    print("== 岛基线（每格是轮次中位数）==")
    print("场景 | 轮 | tick p50/p95/p99 (ms) | tick 平均 | 已加载区块 | 主控串行段 S | "
          "跟岛走 P | K 组独立跑的上限 | 各组三阶段 max/mean")
    out = []
    for name in scenes:
        rounds = per[name]
        if not rounds:
            continue
        k = int(name.split("-")[1])
        bounds = []
        balance = []
        for round_name, s in rounds.items():
            alone = base.get(round_name)
            if alone:
                parallel = alone["tick"] - alone["serial"]
                bounds.append(s["tick"] / (s["serial"] + parallel))
            values = list(s["group_three"].values())
            if values and sum(values):
                balance.append(max(values) / (sum(values) / len(values)))
        med = lambda key: statistics.median(r[key] for r in rounds.values())
        row = {
            "scenario": name, "groups": k, "rounds": len(rounds),
            "p50": med("p50"), "p95": med("p95"), "p99": med("p99"),
            "tick": med("tick"), "loaded": med("loaded"),
            "serial": med("serial"), "parallel": med("tick") - med("serial"),
            "bound": statistics.median(bounds) if bounds else None,
            "balance": statistics.median(balance) if balance else None,
            "phases": {
                p: statistics.median(r["phases"][p] for r in rounds.values())
                for p in heavy.ALL_PHASES
            },
            "ticket_update": med("ticket_update"),
        }
        out.append(row)
        bound = f"{row['bound']:.2f}×" if row["bound"] else "—"
        bal = f"{row['balance']:.2f}" if row["balance"] else "—"
        print(f"{name} | {row['rounds']} | {row['p50']:.3f}/{row['p95']:.3f}/{row['p99']:.3f} | "
              f"{row['tick']:.3f} | {row['loaded']:.0f} | {row['serial']:.3f} | "
              f"{row['parallel']:.3f} | {bound} | {bal}")
    print()
    print("== 各阶段平均耗时 ms（括号：占一刻）==")
    for row in out:
        parts = [
            f"{p} {v:.3f}({v / row['tick'] * 100:.1f}%)"
            for p, v in row["phases"].items() if v >= 0.0005
        ]
        print(f"{row['scenario']}: " + ", ".join(parts)
              + f"; ticket_update {row['ticket_update']:.3f}")
    (root / "islands-summary.json").write_text(json.dumps(out, indent=2))
    print(f"\njson: {root / 'islands-summary.json'}")


if __name__ == "__main__":
    main()
