#!/usr/bin/env python3
"""汇总 compare-islands.sh 的结果。

每次运行只取假玩家全部进服以后（开跑 + 进人时间 + 2 秒之后）到假玩家结束之间打出的剖析报告。
每份报告给出：主控每刻耗时（tick_total）的 p50 / p99 / 最大、主控的实际 TPS（报告里的刻数 / 报告覆盖
的秒数）、混合驱动下每个岛自己的 TPS 和每刻耗时、交接、拆合、整个世界命令的拼和拆、丢刻和判死。
同一场景、同一轮里各配置相对 `off-t1`（多岛关）的比值是主要结论，绝对数随机器负载变化。

Summarise compare-islands.sh: only reports printed while every bot is in
(start + ramp + 2 s until the bots end) count.
"""

import json
import os
import re
import statistics
import sys
from datetime import datetime, timezone

HEADER = re.compile(r"^(\S+)\s.*tick profile: (\d+) ticks over ([\d.]+) s")
METRIC = re.compile(
    r"^\s+(\w+)\s+samples=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+p95=\s*([\d.]+)"
    r"\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)"
)
ISLAND = re.compile(
    r"^\s+island (\d+)\s+(.*?)\s*ticks=(\d+) tps=([\d.]+) busy mean=([\d.]+) p50=([\d.]+)"
    r" p99=([\d.]+) max=([\d.]+) dropped=(\d+) owed=(\d+)"
)
STATE = re.compile(r"chunks=(\d+) players=(\d+) entities=(\d+)")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
RAMP = 2.0


def parse_time(stamp):
    stamp = stamp.rstrip("Z")
    if "." in stamp:
        head, frac = stamp.split(".", 1)
        stamp = head + "." + frac[:6]
        fmt = "%Y-%m-%dT%H:%M:%S.%f"
    else:
        fmt = "%Y-%m-%dT%H:%M:%S"
    return datetime.strptime(stamp, fmt).replace(tzinfo=timezone.utc).timestamp()


def reports(path, start, end):
    out = []
    current = None
    for raw in open(path, errors="replace"):
        line = ANSI.sub("", raw.rstrip("\n"))
        head = HEADER.match(line)
        if head:
            if current is not None:
                out.append(current)
            stamp = parse_time(head.group(1))
            current = {
                "time": stamp,
                "ticks": int(head.group(2)),
                "seconds": float(head.group(3)),
                "metrics": {},
                "islands": [],
            }
            continue
        if current is None:
            continue
        m = METRIC.match(line)
        if m:
            current["metrics"].setdefault(
                m.group(1),
                {
                    "samples": int(m.group(2)),
                    "mean": float(m.group(3)),
                    "p50": float(m.group(4)),
                    "p95": float(m.group(5)),
                    "p99": float(m.group(6)),
                    "max": float(m.group(7)),
                },
            )
            continue
        i = ISLAND.match(line)
        if i:
            state = STATE.search(i.group(2))
            current["islands"].append(
                {
                    "id": int(i.group(1)),
                    "chunks": int(state.group(1)) if state else None,
                    "players": int(state.group(2)) if state else None,
                    "entities": int(state.group(3)) if state else None,
                    "ticks": int(i.group(3)),
                    "tps": float(i.group(4)),
                    "busy_mean": float(i.group(5)),
                    "busy_p99": float(i.group(7)),
                    "busy_max": float(i.group(8)),
                    "dropped": int(i.group(9)),
                }
            )
    if current is not None:
        out.append(current)
    # 报告打在区间末尾：区间整个落在窗口里才算。
    return [r for r in out if r["time"] - r["seconds"] >= start and r["time"] <= end + 0.5]


def load_of(path):
    try:
        text = open(path).read()
    except OSError:
        return float("nan")
    m = re.search(r"load averages?:\s*([\d.]+)", text)
    return float(m.group(1)) if m else float("nan")


def summarize_run(directory):
    try:
        cpu_start, cpu_end, wall_start, wall_end = map(
            float, open(os.path.join(directory, "cpu.txt")).read().split()
        )
    except (OSError, ValueError):
        return None
    log = os.path.join(directory, "server.log")
    picked = reports(log, wall_start + RAMP + 2.0, wall_end)
    text = open(log, errors="replace").read()
    run = {
        "reports": len(picked),
        "load": (load_of(os.path.join(directory, "load-before.txt"))
                 + load_of(os.path.join(directory, "load-after.txt"))) / 2,
        "cpu": (cpu_end - cpu_start) / max(wall_end - wall_start, 1e-9),
        "deaths": text.count("was declared dead"),
        "drops": text.count("dropping the ticks it owes"),
    }
    if not picked:
        return run
    ticks = sum(r["ticks"] for r in picked)
    seconds = sum(r["seconds"] for r in picked)
    run["tps"] = ticks / seconds if seconds else float("nan")

    def series(name, key):
        values = [r["metrics"][name][key] for r in picked
                  if name in r["metrics"] and r["metrics"][name]["samples"] > 0]
        return values

    def med(name, key):
        values = series(name, key)
        return statistics.median(values) if values else float("nan")

    run["tick_p50"] = med("tick_total", "p50")
    run["tick_p99"] = med("tick_total", "p99")
    run["tick_mean"] = med("tick_total", "mean")
    run["tick_max"] = max(series("tick_total", "max") or [float("nan")])
    run["master_busy"] = med("master_busy", "mean")
    run["island_busy"] = med("island_busy", "mean")
    run["handoff_mean"] = med("island_handoff", "mean")
    run["handoff_p50"] = med("island_handoff", "p50")
    run["handoff_p95"] = med("island_handoff", "p95")
    run["queue_wait"] = med("island_queue_wait", "mean")
    run["island_count"] = med("island_count", "mean")
    run["loaded"] = med("loaded_chunks", "mean")
    for name in ("island_merge", "island_split", "island_unify", "island_divide"):
        samples = [(r["metrics"][name]["samples"], r["metrics"][name]["mean"], r["metrics"][name]["max"])
                   for r in picked if name in r["metrics"]]
        count = sum(s for s, _, _ in samples)
        total = sum(s * m for s, m, _ in samples)
        run[name] = {
            "count": count,
            "mean": total / count if count else float("nan"),
            "max": max((x for s, _, x in samples if s), default=float("nan")),
        }
    per_island = {}
    for r in picked:
        for isl in r["islands"]:
            per_island.setdefault(isl["id"], []).append(isl)
    run["islands"] = {
        str(k): {
            "tps": statistics.median(i["tps"] for i in v),
            "busy_mean": statistics.median(i["busy_mean"] for i in v),
            "busy_p99": statistics.median(i["busy_p99"] for i in v),
            "entities": max((i["entities"] for i in v if i["entities"] is not None), default=None),
            "players": max((i["players"] for i in v if i["players"] is not None), default=None),
            "chunks": max((i["chunks"] for i in v if i["chunks"] is not None), default=None),
            "dropped": sum(i["dropped"] for i in v),
        }
        for k, v in sorted(per_island.items())
    }
    return run


def fmt(value, digits=2):
    if value is None or (isinstance(value, float) and value != value):
        return "-"
    return f"{value:.{digits}f}"


def main():
    root = sys.argv[1]
    scenes = sys.argv[2:] or sorted(os.listdir(root))
    result = {}
    for scene in scenes:
        scene_dir = os.path.join(root, scene)
        if not os.path.isdir(scene_dir):
            continue
        result[scene] = {}
        for config in sorted(os.listdir(scene_dir)):
            config_dir = os.path.join(scene_dir, config)
            for round_name in sorted(os.listdir(config_dir)):
                run = summarize_run(os.path.join(config_dir, round_name))
                if run is not None:
                    result[scene].setdefault(config, {})[round_name] = run
    with open(os.path.join(root, "islands-compare.json"), "w") as out:
        json.dump(result, out, indent=1, sort_keys=True)
    for scene, configs in result.items():
        print(f"\n## {scene}")
        print("config     round  load   TPS    p50    p99     max    mean  /off  master  island  cpu   hand p50/p95(us) q(ms) isl  merge/split   unify/divide(ms)  deaths drops")
        base = configs.get("off-t1", {})
        for config, rounds in sorted(configs.items()):
            for round_name, run in sorted(rounds.items()):
                ref = base.get(round_name, {})
                ratio = (run.get("tick_mean", float("nan")) / ref["tick_mean"]
                         if ref.get("tick_mean") else float("nan"))
                merges = run.get("island_merge", {})
                splits = run.get("island_split", {})
                unify = run.get("island_unify", {})
                divide = run.get("island_divide", {})
                print(
                    f"{config:<10} {round_name[-1]:>5} {fmt(run['load'], 1):>5} {fmt(run.get('tps')):>6}"
                    f" {fmt(run.get('tick_p50')):>6} {fmt(run.get('tick_p99')):>6} {fmt(run.get('tick_max')):>7}"
                    f" {fmt(run.get('tick_mean')):>6} {fmt(ratio):>5} {fmt(run.get('master_busy')):>6}"
                    f" {fmt(run.get('island_busy')):>6} {fmt(run['cpu']):>5}"
                    f" {fmt(run.get('handoff_p50', float('nan')) * 1000, 0):>5}/{fmt(run.get('handoff_p95', float('nan')) * 1000, 0):<5}"
                    f" {fmt(run.get('queue_wait')):>5} {fmt(run.get('island_count'), 1):>4}"
                    f" {merges.get('count', 0)}/{splits.get('count', 0)}"
                    f" ({fmt(merges.get('mean'))}/{fmt(splits.get('mean'))})"
                    f" {unify.get('count', 0)}x {fmt(unify.get('mean'))}/{fmt(divide.get('mean'))}"
                    f" {run['deaths']:>3} {run['drops']:>3}"
                )
                islands = run.get("islands", {})
                if islands:
                    parts = [f"#{k}:{fmt(v['tps'], 1)}tps/{fmt(v['busy_mean'])}ms/{v['entities']}e"
                             for k, v in islands.items()]
                    print("             islands " + " ".join(parts))


if __name__ == "__main__":
    main()
