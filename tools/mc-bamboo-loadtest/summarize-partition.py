#!/usr/bin/env python3
"""汇总第 2 步：划分耗时、单元形状、越界和按实际划分重算的理论上限。

用法：summarize-partition.py <结果目录>
    结果目录里有 `on/` 和 `off/` 两棵 `run-heavy.sh` 的结果，每棵每个场景
    有 ROUNDS 轮 server.log。开关打开和关闭时的 tick p50 / p95 在**同一轮编号**
    内比较，再取轮次中位数；机器上有别的任务在跑，绝对毫秒数只作参考。

`partition (step 2, ...)` 段每行是一个指标；`halo N ...` 两段分别对应
`halo = 1 / 2`。第 0 步的 `summarize-heavy.py` 另算离线理论上限，本脚本
最后把两份数字并排打印。
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
KV = re.compile(r"(\w+)=([-\d.]+)")
OUT_BLOCK = re.compile(r"out-block (\S+) (\d+)")
UNIT_SIZE = re.compile(r"unit-size (\d+) (\d+)")

THREADS = 8
HALOS = (1, 2)


def parse_reports(path):
    reports = []
    current = None
    after_ready = False
    for raw in path.read_text(errors="replace").splitlines():
        line = ANSI.sub("", raw)
        if "bench scenario ready" in line:
            after_ready = True
        header = HEADER.search(line)
        if header:
            current = {
                "ticks": int(header.group(1)),
                "phases": {},
                "metrics": {},
                "partition": {},
                "halos": {},
                "blocks": {},
                "sizes": {},
                "section": "phases",
                "after_ready": after_ready,
            }
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
        if line.startswith("partition (step 2"):
            current["section"] = "partition"
            continue
        phase = PHASE.match(line)
        if phase and current["section"] == "phases":
            current["phases"][phase.group(1)] = [float(v) for v in phase.groups()[1:]]
            continue
        metric = METRIC.match(line)
        if metric and current["section"] == "metrics":
            current["metrics"][metric.group(1)] = [float(v) for v in metric.groups()[1:7]]
            continue
        if current["section"] != "partition":
            continue
        stripped = line.strip()
        if stripped.startswith("halo "):
            match = re.match(r"halo (\d+) (.+)", stripped)
            if match:
                halo = int(match.group(1))
                rest = match.group(2)
                blocks = OUT_BLOCK.match(rest)
                if blocks:
                    current["blocks"].setdefault(halo, {})[blocks.group(1)] = int(blocks.group(2))
                elif rest.startswith("speedup_input"):
                    current["halos"].setdefault(halo, {}).update(
                        {"si_" + k: float(v) for k, v in KV.findall(rest)}
                    )
                else:
                    current["halos"][halo] = {k: float(v) for k, v in KV.findall(rest)}
            continue
        size = UNIT_SIZE.match(stripped)
        if size:
            current["sizes"][int(size.group(1))] = int(size.group(2))
            continue
        label = stripped.split(" ", 1)[0].split("=", 1)[0]
        values = {k: float(v) for k, v in KV.findall(stripped)}
        if label == "partition_ns":
            current["partition"]["partition_" + "mean"] = values.get("mean", 0.0)
            current["partition"]["partition_p50"] = values.get("p50", 0.0)
            current["partition"]["partition_p95"] = values.get("p95", 0.0)
        elif label == "partition_share":
            for k, v in values.items():
                current["partition"][k] = v
        elif label == "triggers_per_tick":
            current["partition"]["triggers_per_tick"] = values.get("mean", 0.0)
        elif label == "units":
            current["partition"]["units"] = values.get("mean", 0.0)
            current["partition"]["unit_p50"] = values.get("p50", 0.0)
            current["partition"]["unit_p95"] = values.get("p95", 0.0)
            for k in ("busy", "size_mean", "merges", "splits"):
                current["partition"][k] = values.get(k, 0.0)
        elif label == "triggers_in_window":
            current["partition"]["ticks"] = values.get("ticks", 0.0)
    return reports


def weighted(values, weights):
    total = sum(weights)
    return sum(v * w for v, w in zip(values, weights)) / total if total else 0.0


def analyse(report):
    if not report["after_ready"] or not report["partition"]:
        return None
    total = report["metrics"].get("tick_total")
    if not total or not total[0]:
        return None
    p = report["partition"]
    three = ["scheduled_ticks", "block_events", "block_entities"]
    three_mean = sum(report["phases"].get(name, [0, 0])[1] for name in three)
    out = {
        "samples": total[0],
        "p50": total[2],
        "p95": total[3],
        "tick_mean": total[1],
        "three": three_mean,
        "partition_ns": p.get("partition_mean", 0.0),
        "partition_p50": p.get("partition_p50", 0.0),
        "partition_p95": p.get("partition_p95", 0.0),
        "partition_share": p.get("partition_share", 0.0),
        "tick_mean_ms": p.get("tick_mean", 0.0),
        "three_mean_ms": p.get("three_mean", 0.0),
        "three_share": p.get("three_share", 0.0),
        "units": p.get("units", 0.0),
        "unit_p50": p.get("unit_p50", 0.0),
        "unit_p95": p.get("unit_p95", 0.0),
        "busy": p.get("busy", 0.0),
        "size_mean": p.get("size_mean", 0.0),
        "merges": p.get("merges", 0.0),
        "splits": p.get("splits", 0.0),
        "triggers_per_tick": p.get("triggers_per_tick", 0.0),
        "ticks": report["ticks"],
        "halos": {},
        "blocks": {},
        "sizes": {},
    }
    for halo in HALOS:
        h = report["halos"].get(halo, {})
        out["halos"][halo] = {
            "out": h.get("out_triggers", 0.0),
            "unassigned": h.get("unassigned", 0.0),
            "units": h.get("out_units", 0.0),
            "max_distance": h.get("max_distance", 0.0),
            "reads": h.get("reads", 0.0),
            "writes": h.get("writes", 0.0),
            "out_per_tick": h.get("out_per_tick_mean", 0.0),
            "out_share": h.get("out_share", 0.0),
            "tail_share": h.get("tail_ns", 0.0) / (p.get("three_mean", 1.0) * 1_000_000 * max(total[0], 1)) if p.get("three_mean") else 0.0,
            "speedup_input": {},
        }
        out["blocks"][halo] = report["blocks"].get(halo, {})
        si = report["halos"].get(halo, {})
        out["halos"][halo]["speedup_input"] = {
            k: si.get("si_" + k, 0.0) for k in ("three_ns", "tail_ns", "other_ns", "biggest_unit_ns", "per_unit_ns")
        }
    out["sizes"] = report["sizes"]
    # 按实际划分重算上限：三阶段 S，越界尾段进串行，最贵单元串行，
    # 其余按 8 线程；整刻 T 只扣掉三阶段的缩短量。
    ticks = max(report["ticks"], 1)
    for halo in HALOS:
        si = out["halos"][halo]["speedup_input"]
        s = si["three_ns"] / ticks
        tail = si["tail_ns"] / ticks
        other = si["other_ns"] / ticks
        biggest = si["biggest_unit_ns"] / ticks
        parallel = max(0.0, s - tail - other)
        lower = max(parallel / THREADS, biggest) if s else 0.0
        serial = max(0.0, s - parallel)
        new_three = serial + lower
        tick = p.get("tick_mean", 0.0) * 1_000_000
        new_tick = tick - s + new_three
        out["halos"][halo]["speedup"] = tick / new_tick if new_tick > 0 else 1.0
        out["halos"][halo]["shortening"] = 1.0 - 1.0 / out["halos"][halo]["speedup"]
    return out


def summarise(root):
    per_round = []
    for directory in sorted((root / "on").glob("*/*")):
        if not directory.is_dir():
            continue
        log = directory / "server.log"
        if not log.exists():
            continue
        client_path = directory / "client.json"
        target = 1
        if client_path.exists():
            target = json.loads(client_path.read_text()).get("bots", 1)
        parsed = [x for x in (analyse(r) for r in parse_reports(log)) if x]
        parsed = parsed[1:] or parsed
        peak = max((r["tick_mean_ms"] for r in parsed), default=0.0)
        steady = [r for r in parsed if r["tick_mean_ms"] >= 0.9 * peak]
        if not steady:
            steady = parsed
        if not steady:
            continue
        weights = [r["samples"] for r in steady]
        merged = {"samples": sum(weights), "blocks": {}, "sizes": {}}
        scalar = (
            "p50", "p95", "tick_mean", "three", "partition_ns", "partition_p50",
            "partition_p95", "partition_share", "tick_mean_ms", "three_mean_ms",
            "three_share", "units", "unit_p50", "unit_p95", "busy", "size_mean",
            "merges", "splits", "triggers_per_tick", "ticks",
        )
        for key in scalar:
            merged[key] = weighted([r[key] for r in steady], weights)
        for halo in HALOS:
            for block in set().union(*(r["blocks"].get(halo, {}).keys() for r in steady)):
                merged["blocks"].setdefault(halo, {})[block] = weighted(
                    [r["blocks"].get(halo, {}).get(block, 0.0) for r in steady], weights
                )
        for size in set().union(*(set(r["sizes"].keys()) for r in steady)):
            merged["sizes"][size] = weighted(
                [r["sizes"].get(size, 0.0) for r in steady], weights
            )
        merged["halos"] = {}
        for halo in HALOS:
            merged["halos"][halo] = {}
            for key in steady[0]["halos"][halo]:
                if key == "speedup_input":
                    continue
                merged["halos"][halo][key] = weighted(
                    [r["halos"][halo][key] for r in steady], weights
                )
            for key in ("speedup", "shortening"):
                merged["halos"][halo][key] = weighted(
                    [r["halos"][halo][key] for r in steady], weights
                )
        per_round.append((directory.parent.name + "/" + directory.name, merged))
    return per_round


def by_scenario(per_round):
    groups = {}
    for name, r in per_round:
        groups.setdefault(name.split("/")[0], []).append(r)
    out = {}
    for name, rounds in groups.items():
        merged = {"rounds": len(rounds)}
        for key in rounds[0]:
            if isinstance(rounds[0][key], dict):
                continue
            merged[key] = median([r[key] for r in rounds])
        merged["halos"] = {}
        for halo in HALOS:
            merged["halos"][halo] = {}
            for key in rounds[0]["halos"][halo]:
                if key == "speedup_input":
                    continue
                merged["halos"][halo][key] = median([r["halos"][halo][key] for r in rounds])
            for key in ("speedup", "shortening"):
                merged["halos"][halo][key] = median([r["halos"][halo][key] for r in rounds])
        merged["blocks"] = {}
        merged["sizes"] = {}
        for halo in HALOS:
            for k in set().union(*(r["blocks"].get(halo, {}).keys() for r in rounds)):
                merged["blocks"].setdefault(halo, {})[k] = median(
                    [r["blocks"].get(halo, {}).get(k, 0) for r in rounds]
                )
        for k in set().union(*(set(r["sizes"].keys()) for r in rounds)):
            merged["sizes"][k] = median([r["sizes"].get(k, 0) for r in rounds])
        out[name] = merged
    return out


def median(values, default=0.0):
    return statistics.median(values) if values else default


def main():
    root = Path(sys.argv[1])
    on = by_scenario(summarise(root))
    off_rounds = {}
    for directory in sorted((root / "off").glob("*/*")):
        if not directory.is_dir():
            continue
        # 关闭时没有 partition 段，只读剖析报告的 tick 分位数。
        reports = parse_reports(directory / "server.log") if (directory / "server.log").exists() else []
        ready = [r for r in reports if r["after_ready"] and r["metrics"].get("tick_total")]
        parsed = [
            {"p50": r["metrics"]["tick_total"][2], "p95": r["metrics"]["tick_total"][3],
             "mean": r["metrics"]["tick_total"][1], "samples": r["metrics"]["tick_total"][0]}
            for r in ready
        ]
        parsed = parsed[1:] or parsed
        if parsed:
            w = [r["samples"] for r in parsed]
            off_rounds.setdefault(directory.parent.name, []).append(
                {k: weighted([r[k] for r in parsed], w) for k in ("p50", "p95", "mean")}
            )
    off = {
        name: {k: median([r[k] for r in rounds]) for k in ("p50", "p95", "mean")}
        for name, rounds in off_rounds.items()
    }

    keys = sorted(on)
    out = {"scenarios": keys, "on": on, "off": off}
    (root / "partition-summary.json").write_text(json.dumps(out, indent=2, ensure_ascii=False, default=str))

    print("== 划分阶段：每刻耗时与占一刻（开关打开）==")
    print("场景 | 轮 | 划分 ns p50/p95 | 占一刻 | tick p50/p95 | 三阶段占一刻")
    for key in keys:
        r = on[key]
        print(" | ".join([
            key,
            str(r["rounds"]), f"{r['partition_p50']:.0f}/{r['partition_p95']:.0f}",
            f"{r['partition_share']:.3f}%",
            f"{r['p50']:.3f}/{r['p95']:.3f}",
            f"{r['three_share']:.1f}%",
        ]))
    print()
    print("== 开关开 / 关的 tick p50 / p95（同轮编号配对，逐对比值取中位数）==")
    print("场景 | 关 p50/p95 | 开 p50/p95 | p50 比值 | p95 比值")
    ratios = []
    for key in keys:
        # on 的目录名是 <场景>/round-N；off 用同样的名字。
        name = key
        if name not in off:
            continue
        a, b = off[name], on[key]
        ratios.append((b["p50"] / a["p50"] if a["p50"] else 1.0,
                       b["p95"] / a["p95"] if a["p95"] else 1.0))
        print(" | ".join([
            name, f"{a['p50']:.3f}/{a['p95']:.3f}", f"{b['p50']:.3f}/{b['p95']:.3f}",
            f"{b['p50'] / a['p50']:.3f}" if a["p50"] else "-",
            f"{b['p95'] / a['p95']:.3f}" if a["p95"] else "-",
        ]))
    if ratios:
        print(f"逐对比值中位数: p50={median([x[0] for x in ratios]):.3f} p95={median([x[1] for x in ratios]):.3f}")
    print()
    print("== 单元：个数、大小分布、每秒合并 / 拆分 ==")
    for key in keys:
        r = on[key]
        chunks = r["sizes"]
        dist = "/".join(f"{size}:{count}" for size, count in sorted(chunks.items())[:8])
        print(f"{key}: 单元={r['units']:.1f} 忙区块={r['busy']:.1f} 平均大小={r['size_mean']:.2f} "
              f"合并={r['merges'] * 20:.2f}/s 拆分={r['splits'] * 20:.2f}/s 大小分布={dist}")
    print()
    print("== 越界：每刻占比、halo 对比、读 / 写、主要方块 ==")
    for key in keys:
        r = on[key]
        for halo in HALOS:
            h = r["halos"][halo]
            blocks = sorted(r["blocks"].get(halo, {}).items(), key=lambda x: -x[1])[:3]
            print(f"{key} halo={halo}: 越界/刻={h['out']:.1f} 占比={h['out_share'] * 100:.2f}% "
                  f"未归属={h['unassigned']:.0f} 涉及单元={h['units']:.0f} 最远={h['max_distance']:.0f}区块 "
                  f"读={h['reads']:.0f} 写={h['writes']:.0f} 尾段={h['tail_share'] * 100:.2f}% "
                  f"主要方块={blocks}")
    print()
    print("== 按实际划分重算的理论上限（8 线程，越界单元进串行尾段）==")
    for key in keys:
        r = on[key]
        print(f"{key}: halo1={r['halos'][1]['speedup']:.3f}× (缩短 {r['halos'][1]['shortening'] * 100:.1f}%) "
              f"halo2={r['halos'][2]['speedup']:.3f}× (缩短 {r['halos'][2]['shortening'] * 100:.1f}%)")
    print(f"\njson report: {root / 'partition-summary.json'}")


if __name__ == "__main__":
    main()
