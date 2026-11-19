#!/usr/bin/env python3
"""汇总 run-long.sh 的一次长时间压测。

读 `server.log` 里的每一份剖析报告（BAMBOO_PROFILE 秒一份）、`samples.txt` 的内存 / CPU / 负载
采样和 `client.json`，打印：

- 每刻耗时（主控的 `tick_total`）：按样本数加权的均值、各份报告 p50 / p99 的中位数和最大、
  全程最大；主控 TPS；各岛自己的 TPS 最低值。
- 拆合：合并 / 拆分的次数（剖析器的 `island_merge` / `island_split` 样本数）、单次耗时的加权
  均值、各份报告 p95 的最大、全程最大；照做前等了几刻（`island_topology_wait`）；分区器统计的
  次数、取消的拆分、不变量失败；跨岛搬人的次数；整个世界命令的拼 / 拆。
- 丢刻（`island_dropped_ticks` 的合计和「an island can't keep up」的条数）、欠刻（各岛 `owed=`
  的最大值）、判死次数。
- 内存曲线：开头、每 10 分钟、结尾的 RSS，前后两半各自的线性斜率（找只涨不落）。
- 日志里的 WARN / ERROR 按消息归类计数，panic 和不变量失败的原文。

跨报告的分位数是「各份报告的分位数」再取中位数 / 最大，不是全程精确的分位数。

Summarise one long run of run-long.sh: tick times, splits and merges,
dropped and owed ticks, deaths, the memory curve, and warnings. Percentiles
across reports are medians / maxima of per-report percentiles, not exact
whole-run percentiles.
"""

import json
import os
import re
import statistics
import sys
from collections import Counter

HEADER = re.compile(r"^(\S+)\s.*tick profile: (\d+) ticks over ([\d.]+) s")
METRIC = re.compile(
    r"^\s+(\w+)\s+(?:samples|count)=(\d+)\s+mean=\s*([\d.]+)\s+p50=\s*([\d.]+)\s+p95=\s*([\d.]+)"
    r"\s+p99=\s*([\d.]+)\s+max=\s*([\d.]+)"
)
ISLAND = re.compile(
    r"^\s+island (\d+)\s+(.*?)\s*ticks=(\d+) tps=([\d.]+) busy mean=([\d.]+) p50=([\d.]+)"
    r" p99=([\d.]+) max=([\d.]+) dropped=(\d+) owed=(\d+)"
)
MERGES = re.compile(r"^\s+merges total=(\d+)")
SPLITS = re.compile(r"^\s+splits total=(\d+) .* cancelled=(\d+)")
INVARIANTS = re.compile(r"^\s+invariant_failures=(\d+)")
LEVEL = re.compile(r"^\S+\s+(WARN|ERROR)\s+(\S+?):\s+(.*)$")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def parse_log(path):
    reports = []
    current = None
    warnings = Counter()
    panics = []
    for raw in open(path, errors="replace"):
        line = ANSI.sub("", raw.rstrip("\n"))
        if "panicked" in line or "assertion" in line:
            panics.append(line[:400])
        level = LEVEL.match(line)
        if level:
            message = re.sub(r"\d+(\.\d+)?", "N", level.group(3))
            message = re.sub(r"\{.*", "", message).strip()[:120]
            warnings[(level.group(1), message)] += 1
        head = HEADER.match(line)
        if head:
            if current is not None:
                reports.append(current)
            current = {
                "ticks": int(head.group(2)),
                "seconds": float(head.group(3)),
                "metrics": {},
                "islands": [],
                "merges": 0,
                "splits": 0,
                "cancelled": 0,
                "invariant_failures": 0,
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
            current["islands"].append(
                {
                    "id": int(i.group(1)),
                    "ticks": int(i.group(3)),
                    "tps": float(i.group(4)),
                    "dropped": int(i.group(9)),
                    "owed": int(i.group(10)),
                }
            )
            continue
        for pattern, keys in ((MERGES, ("merges",)), (SPLITS, ("splits", "cancelled")),
                              (INVARIANTS, ("invariant_failures",))):
            found = pattern.match(line)
            if found:
                for key, value in zip(keys, found.groups()):
                    current[key] += int(value)
    if current is not None:
        reports.append(current)
    return reports, warnings, panics


def combine(reports, name):
    """一个指标跨全部报告：样本数、加权均值、p50 / p95 / p99 的中位数和最大、全程最大。"""
    rows = [r["metrics"][name] for r in reports if name in r["metrics"] and r["metrics"][name]["samples"] > 0]
    if not rows:
        return None
    samples = sum(row["samples"] for row in rows)
    mean = sum(row["mean"] * row["samples"] for row in rows) / samples
    out = {"samples": samples, "mean": mean, "max": max(row["max"] for row in rows)}
    for key in ("p50", "p95", "p99"):
        values = [row[key] for row in rows]
        out[key + "_median"] = statistics.median(values)
        out[key + "_max"] = max(values)
    return out


def describe(label, stats, unit="ms"):
    if stats is None:
        return f"{label}: none"
    return (
        f"{label}: samples={stats['samples']} mean={stats['mean']:.3f} "
        f"p50(med/max)={stats['p50_median']:.3f}/{stats['p50_max']:.3f} "
        f"p95(med/max)={stats['p95_median']:.3f}/{stats['p95_max']:.3f} "
        f"p99(med/max)={stats['p99_median']:.3f}/{stats['p99_max']:.3f} max={stats['max']:.3f} {unit}"
    )


def slope(points):
    """最小二乘的斜率（每小时多少 MiB）。"""
    if len(points) < 2:
        return float("nan")
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    mx = statistics.fmean(xs)
    my = statistics.fmean(ys)
    den = sum((x - mx) ** 2 for x in xs)
    if den == 0:
        return float("nan")
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den * 3600


def main():
    directory = sys.argv[1]
    reports, warnings, panics = parse_log(os.path.join(directory, "server.log"))
    # 第一份报告含搭场景，不算。
    # The first report includes building the scene; skip it.
    steady = reports[1:] if len(reports) > 1 else reports
    seconds = sum(r["seconds"] for r in steady)
    ticks = sum(r["ticks"] for r in steady)
    print(f"reports={len(steady)} covering {seconds:.0f} s, master ticks={ticks}, "
          f"TPS={ticks / seconds if seconds else float('nan'):.2f}")
    tps = [r["ticks"] / r["seconds"] for r in steady if r["seconds"] > 0]
    if tps:
        print(f"master TPS per report: min={min(tps):.2f} median={statistics.median(tps):.2f}")
    island_tps = [i["tps"] for r in steady for i in r["islands"] if i["ticks"] > 0]
    if island_tps:
        low = sorted(island_tps)[: max(1, len(island_tps) // 100)]
        print(f"island TPS per report: min={min(island_tps):.2f} 1%-low mean={statistics.fmean(low):.2f} "
              f"median={statistics.median(island_tps):.2f} (island-reports={len(island_tps)})")
    print(describe("tick_total", combine(steady, "tick_total")))
    print(describe("master_busy", combine(steady, "master_busy")))
    print(describe("island_busy", combine(steady, "island_busy")))
    print(describe("island_handoff", combine(steady, "island_handoff")))
    print(describe("island_queue_wait", combine(steady, "island_queue_wait")))
    counts = [r["metrics"].get("island_count", {}).get("mean") for r in steady]
    counts = [c for c in counts if c is not None]
    if counts:
        print(f"island_count per report mean: min={min(counts):.2f} median={statistics.median(counts):.2f} "
              f"max={max(counts):.2f}")
    players = [r["metrics"].get("online_players", {}).get("mean") for r in steady]
    players = [p for p in players if p is not None]
    if players:
        print(f"online_players per report mean: min={min(players):.1f} median={statistics.median(players):.1f}")
    loaded = [r["metrics"].get("loaded_chunks", {}).get("mean") for r in steady]
    loaded = [p for p in loaded if p is not None]
    if loaded:
        print(f"loaded_chunks per report mean: min={min(loaded):.0f} median={statistics.median(loaded):.0f} "
              f"max={max(loaded):.0f}")
    print(describe("island_merge", combine(steady, "island_merge")))
    print(describe("island_split", combine(steady, "island_split")))
    print(describe("island_topology_wait", combine(steady, "island_topology_wait"), "ticks"))
    deferrals = combine(steady, "island_split_deferrals")
    if deferrals:
        print(f"island_split_deferrals: total={deferrals['mean'] * deferrals['samples']:.0f}")
    print(describe("island_unify", combine(steady, "island_unify")))
    print(describe("island_world_command", combine(steady, "island_world_command")))
    print(describe("island_divide", combine(steady, "island_divide")))
    crossings = combine(steady, "island_crossings")
    if crossings:
        print(f"island_crossings: players moved={crossings['mean'] * crossings['samples']:.0f}")
    merges = sum(r["merges"] for r in steady)
    splits = sum(r["splits"] for r in steady)
    cancelled = sum(r["cancelled"] for r in steady)
    failures = sum(r["invariant_failures"] for r in steady)
    hours = seconds / 3600 if seconds else float("nan")
    print(f"partitioner: merges={merges} ({merges / hours:.0f}/h) splits={splits} ({splits / hours:.0f}/h) "
          f"cancelled splits={cancelled} invariant_failures={failures}")
    dropped = combine(steady, "island_dropped_ticks")
    dropped_total = dropped["mean"] * dropped["samples"] if dropped else 0
    dropped_islands = sum(i["dropped"] for r in steady for i in r["islands"])
    owed = max((i["owed"] for r in steady for i in r["islands"]), default=0)
    print(f"dropped ticks={dropped_total:.0f} (per-island sum {dropped_islands}), max owed at a report={owed}")
    log = open(os.path.join(directory, "server.log"), errors="replace").read()
    print(f"declared dead={log.count('was declared dead')} zombies={log.count('declared dead came back')}")

    samples_path = os.path.join(directory, "samples.txt")
    if os.path.exists(samples_path):
        rows = []
        for line in open(samples_path):
            parts = line.split()
            if len(parts) == 4 and parts[0] != "time":
                rows.append(tuple(float(p) for p in parts))
        if rows:
            t0 = rows[0][0]
            points = [(t - t0, rss / 1024) for t, rss, _, _ in rows]
            marks = [points[0]]
            for minute in range(10, 1000, 10):
                near = [p for p in points if abs(p[0] - minute * 60) < 15]
                if near:
                    marks.append(near[0])
            marks.append(points[-1])
            print("rss MiB: " + " ".join(f"{p[0] / 60:.0f}m={p[1]:.0f}" for p in marks))
            half = len(points) // 2
            print(f"rss slope MiB/h: first half={slope(points[:half]):.1f} second half={slope(points[half:]):.1f} "
                  f"peak={max(p[1] for p in points):.0f}")
            cpu = (rows[-1][2] - rows[0][2]) / max(rows[-1][0] - rows[0][0], 1)
            loads = [row[3] for row in rows]
            print(f"process CPU={cpu:.2f} cores; load1 min={min(loads):.1f} median={statistics.median(loads):.1f} "
                  f"max={max(loads):.1f}")

    client_path = os.path.join(directory, "client.json")
    if os.path.exists(client_path):
        client = json.load(open(client_path))
        keys = ("connected", "login_failures", "drops", "keep_alive_rtt_ms")
        print("client: " + json.dumps({k: client.get(k) for k in keys if k in client})[:600])
    state_path = os.path.join(directory, "server-state.txt")
    if os.path.exists(state_path):
        print("server at the end: " + open(state_path).read().strip())
    print("warnings / errors:")
    for (level, message), count in warnings.most_common(15):
        print(f"  {count:>6} {level} {message}")
    if panics:
        print("panics / assertions:")
        for line in panics[:10]:
            print("  " + line)


if __name__ == "__main__":
    main()
