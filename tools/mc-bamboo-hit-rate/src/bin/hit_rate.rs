//! 红石机器缓存命中率实验：在解释执行的同时录制每次触发，离线算命中率。
//!
//! ```bash
//! cargo run --release -p mc-bamboo-hit-rate -- [刻数] [机器名]
//! ```
//!
//! 不写缓存、不改变行为。输出的是 Markdown 表格 + 逐台机器的详细统计，
//! 数字可以直接贴进 `docs/redstone-cache-experiment.md`。

use mc_bamboo_hit_rate::{machines, world::Machine};

use std::collections::BTreeMap;

use mc_bamboo_blocks::recording::{
    ChunkSection, HitRates, Hits, Read, Record, Uncacheable, Write, hit_rates,
};


const DEFAULT_TICKS: i64 = 4000;

fn percentile(mut values: Vec<u64>, p: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = (values.len() - 1) * p / 100;
    values[index]
}

fn pct(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        return "—".to_owned();
    }
    format!("{:.1}%", 100.0 * numerator as f64 / denominator as f64)
}

fn nanos_pct(nanos: u128, total: u128) -> String {
    if total == 0 {
        return "—".to_owned();
    }
    format!("{:.1}%", 100.0 * nanos as f64 / total as f64)
}

/// 一台机器的一行结果。
fn summarize(name: &str, ticks: i64, records: &[Record], rates: &HitRates) -> String {
    let mut out = String::new();
    out.push_str(&format!("## {name}\n\n"));
    out.push_str(&format!(
        "- 运行 {ticks} 刻，{} 次触发，平均每刻 {:.1} 次。\n",
        records.len(),
        records.len() as f64 / ticks as f64
    ));
    let triggers_per_tick: BTreeMap<i64, usize> =
        records.iter().fold(BTreeMap::new(), |mut map, r| {
            *map.entry(r.tick).or_default() += 1;
            map
        });
    let per_tick: Vec<u64> = triggers_per_tick.values().map(|&n| n as u64).collect();
    out.push_str(&format!(
        "- 每刻触发数：min {}，p50 {}，p90 {}，max {}（有触发的刻 {} / {ticks}）。\n",
        per_tick.iter().copied().min().unwrap_or(0),
        percentile(per_tick.clone(), 50),
        percentile(per_tick.clone(), 90),
        per_tick.iter().copied().max().unwrap_or(0),
        per_tick.len(),
    ));
    let read_counts: Vec<u64> = records.iter().map(|r| r.read_positions() as u64).collect();
    let write_counts: Vec<u64> = records.iter().map(|r| r.write_positions() as u64).collect();
    let nanos: Vec<u64> = records.iter().map(|r| r.nanos).collect();
    out.push_str(&format!(
        "- 每次触发读位置数：p50 {}，p90 {}，p99 {}；写位置数：p50 {}，p90 {}，p99 {}。\n",
        percentile(read_counts.clone(), 50),
        percentile(read_counts.clone(), 90),
        percentile(read_counts, 99),
        percentile(write_counts.clone(), 50),
        percentile(write_counts.clone(), 90),
        percentile(write_counts, 99),
    ));
    out.push_str(&format!(
        "- 每次触发耗时（纳秒）：p50 {}，p90 {}，p99 {}（含录制开销）。\n",
        percentile(nanos.clone(), 50),
        percentile(nanos.clone(), 90),
        percentile(nanos, 99),
    ));
    let total = records.len();
    let reasons: BTreeMap<&str, usize> = records.iter().fold(BTreeMap::new(), |mut map, r| {
        for reason in &r.uncacheable {
            let key = match reason {
                Uncacheable::Random => "random",
                Uncacheable::Entities => "entities",
                Uncacheable::Time => "time",
            };
            *map.entry(key).or_default() += 1;
        }
        map
    });
    let strict_uncacheable = records.iter().filter(|r| !r.cacheable(true)).count();
    out.push_str(&format!(
        "- 不能缓存（严格）：{} / {}（{}）；按原因：随机数 {}，实体 {}，时间 {}。\n",
        strict_uncacheable,
        total,
        pct(strict_uncacheable, total),
        reasons.get("random").copied().unwrap_or(0),
        reasons.get("entities").copied().unwrap_or(0),
        reasons.get("time").copied().unwrap_or(0),
    ));
    let cacheable_strict = total - strict_uncacheable;
    let cacheable_optimistic = records.iter().filter(|r| r.cacheable(false)).count();
    out.push_str(&format!(
        "- 可缓存触发数：严格 {}，乐观 {}（把时间读换成相对量后的上界）。\n\n",
        cacheable_strict, cacheable_optimistic,
    ));
    out.push_str("| 口径 | 键 | 命中 / 可缓存 | 命中率 | 可省耗时占比 | 记录数 | 键同效果不同 |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    let row = |label: &str, key: &str, hits: &Hits, denominator: usize| {
        format!(
            "| {label} | {key} | {} / {denominator} | {} | {} | {} | {} |\n",
            hits.hits,
            pct(hits.hits, denominator),
            nanos_pct(hits.saved_nanos, rates.total_nanos),
            hits.records,
            hits.mismatched_effects,
        )
    };
    out.push_str(&row("严格", "精确", &rates.exact_strict, cacheable_strict));
    out.push_str(&row("严格", "段哈希", &rates.section_strict, cacheable_strict));
    out.push_str(&row("乐观", "精确", &rates.exact_optimistic, cacheable_optimistic));
    out.push_str(&row("乐观", "段哈希", &rates.section_optimistic, cacheable_optimistic));
    out.push_str("\n记录数随触发数的增长（严格口径，精确 / 段哈希）：\n\n```\n");
    for (count, exact, section) in &rates.exact_strict.growth {
        out.push_str(&format!("{count}: {exact} / {section}\n"));
    }
    out.push_str("```\n\n");
    let mut memory = 0_usize;
    let mut reads = 0_usize;
    let mut writes = 0_usize;
    let mut sections = 0_usize;
    for record in records {
        memory += record.memory_bytes();
        reads += record.reads.len();
        writes += record.writes.len();
        sections += record.sections.len();
    }
    let unique = rates.exact_strict.records;
    let avg_reads = reads.checked_div(total).unwrap_or(0);
    let avg_writes = writes.checked_div(total).unwrap_or(0);
    let avg_sections = sections.checked_div(total).unwrap_or(0);
    let per_record = std::mem::size_of::<Record>()
        + avg_reads * std::mem::size_of::<Read>()
        + avg_writes * std::mem::size_of::<Write>()
        + avg_sections * std::mem::size_of::<(ChunkSection, u128)>();
    out.push_str(&format!(
        "- 平均每条记录：读 {} 项、写 {} 项、段 {} 个，约 {} 字节；`Record` 本身 {} 字节，\n  \
         全部 {} 条记录共 {:.1} MiB，去重后 {} 条缓存记录约 {:.1} KiB。\n",
        avg_reads,
        avg_writes,
        avg_sections,
        per_record,
        std::mem::size_of::<Record>(),
        total,
        memory as f64 / 1024.0 / 1024.0,
        unique,
        (unique * per_record) as f64 / 1024.0,
    ));
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ticks = args
        .first()
        .and_then(|text| text.parse::<i64>().ok())
        .unwrap_or(DEFAULT_TICKS);
    let only = args.get(1).map(String::as_str);
    let mut table = String::from(
        "| 机器 | 触发数 | 不能缓存 | 精确命中率 | 段哈希命中率 | 可省耗时（精确） | 记录数 | 漏洞数 |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for build in machines::all() {
        let probe = build();
        if let Some(name) = only
            && !probe.name.contains(name)
        {
            continue;
        }
        let name = probe.name;
        let (records, mismatches) = Machine::compare(build, ticks);
        let step = (records.len() / 20).max(1);
        let rates = hit_rates(&records, step);
        let strict_uncacheable = records.iter().filter(|r| !r.cacheable(true)).count();
        let cacheable = records.len() - strict_uncacheable;
        println!("{}", summarize(name, ticks, &records, &rates));
        println!("开/关录制逐刻比较：{mismatches} 刻不一致（应为 0）。\n");
        table.push_str(&format!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} |\n",
            records.len(),
            pct(strict_uncacheable, records.len()),
            pct(rates.exact_strict.hits, cacheable),
            pct(rates.section_strict.hits, cacheable),
            nanos_pct(rates.exact_strict.saved_nanos, rates.total_nanos),
            rates.exact_strict.records,
            rates.exact_strict.mismatched_effects + rates.section_strict.mismatched_effects,
        ));
    }
    println!("## 汇总\n\n{table}");
}
