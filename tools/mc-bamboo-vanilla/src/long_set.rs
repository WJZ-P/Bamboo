//! `long-set`：官方 `LongOpenHashSet` 的迭代顺序对拍。
//!
//! Java 侧 `JavaSetOracle.java --longset` 生成几千组插入序列与最终迭代顺序，Rust 侧
//! `feature::java_long_set::JavaLongSet` 逐个复算并逐条比。键 0、负数、掩码冲突、
//! 跨扩容边界、重复键都在用例里。`--break-self-test` 故意交换前两个输出值。

use std::process::Command;

const USAGE: &str = "usage: long-set <java> [--cases N] [--break-self-test]";

#[derive(Debug)]
struct Options {
    cases: usize,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut cases = 3000_usize;
        let mut break_self_test = false;
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" => cases = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?,
                "--break-self-test" => break_self_test = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(Self {
            cases,
            break_self_test,
        })
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .args(["--longset", "--cases", &options.cases.to_string()])
        .output()
        .map_err(|error| format!("cannot run JavaSetOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JavaSetOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("JavaSetOracle wrote non-UTF-8 output: {error}"))?;
    let mut lines = stdout.lines();
    let mut cases = 0_usize;
    let mut comparisons = 0_u64;
    let mut mismatches = Vec::new();
    while let Some(line) = lines.next() {
        if !line.starts_with("CASE ") {
            continue;
        }
        let case = line.to_owned();
        let ops = lines.next().unwrap_or("");
        let order = lines.next().unwrap_or("");
        let ops = ops.strip_prefix("OPS ").unwrap_or("");
        let order = order.strip_prefix("ORDER ").unwrap_or("");
        let mut set = mc_bamboo_worldgen::feature::java_long_set::JavaLongSet::new();
        for value in ops.split(';').filter(|s| !s.is_empty()) {
            let parsed: i64 = value
                .parse()
                .map_err(|_| format!("{case}: bad op {value}"))?;
            set.insert(parsed);
        }
        let mut expected: Vec<i64> = order
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| format!("{case}: bad order value {value}"))
            })
            .collect::<Result<_, _>>()?;
        if options.break_self_test && expected.len() >= 2 {
            expected.swap(0, 1);
        }
        let actual = set.to_vec();
        comparisons += actual.len().max(expected.len()) as u64;
        cases += 1;
        if actual != expected && mismatches.len() < 8 {
            let first = actual
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(actual.len().min(expected.len()));
            mismatches.push(format!(
                "{case}: {} vs {} entries; first difference at {first} ({:?} vs {:?})",
                actual.len(),
                expected.len(),
                actual.get(first),
                expected.get(first)
            ));
        }
    }
    println!(
        "long-set: {cases} cases, {comparisons} comparisons, {} mismatches",
        mismatches.len()
    );
    if options.break_self_test {
        if mismatches.is_empty() {
            return Err("long-set self-test broke the order but reported 0 mismatches".to_owned());
        }
        println!(
            "long-set self-test: detector works ({} mismatches)",
            mismatches.len()
        );
        return Ok(());
    }
    if cases != options.cases {
        return Err(format!(
            "long-set compared {cases} cases, expected {}",
            options.cases
        ));
    }
    if !mismatches.is_empty() {
        for message in &mismatches {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!("long-set found {} mismatches", mismatches.len()));
    }
    println!("long-set: 0 mismatches");
    Ok(())
}
