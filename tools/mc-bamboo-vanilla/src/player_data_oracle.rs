//! 用官方 26.3 的类检查带槽位物品、玩家数据和 `level.dat` 的编解码。
//!
//! `PlayerDataOracle.java` 在 JVM 里直接调官方的 `ItemStack.CODEC` /
//! `ItemStackWithSlot.CODEC`、`TagValueInput` 和 `PrimaryLevelData.parse` /
//! `createTag`，不启动服务端、不接受 EULA。stdin 一行一个 JSON 请求，stdout 一行
//! 一个 JSON 结果；官方的 `CompoundTag` 用 `HashMap`，键序是哈希序，所以按键
//! 排序的规范 JSON 比较。
//!
//! 每个用例四个方向都过一遍：Bamboo 写出的字节让官方读、官方写回来按键相等；
//! 官方写出的字节 Bamboo 读、再写回去按键相等；顺带逐字段比对官方
//! `TagValueInput` 读出来的值。
//!
//! Persistent item, player-data and level.dat codecs checked against the official
//! classes. No server is started and no EULA is accepted.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_data::{items::STONE, version};
use mc_bamboo_nbt::{Accounter, Tag, read_named, write_named};
use mc_bamboo_protocol::{
    components::archive::slots::{self, INVENTORY_SLOTS, SlottedItem},
    item::ItemStack,
};
use mc_bamboo_storage::{
    level_dat::{self, LevelData, SpawnData},
    player_data::{self, Abilities, PlayerData},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: player-data <java> [--show N]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug, Deserialize)]
struct Response {
    #[allow(dead_code, reason = "the command name is part of the oracle protocol")]
    command: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    original: Option<Value>,
    #[serde(default)]
    written: Option<Value>,
    #[serde(default)]
    canonical: Option<Value>,
    #[serde(default)]
    nbt: Option<String>,
    #[serde(default, rename = "writtenNbt")]
    written_nbt: Option<String>,
    #[serde(default)]
    gzip: Option<String>,
    #[serde(default, rename = "gzipWritten")]
    gzip_written: Option<String>,
    #[serde(default)]
    problems: Vec<usize>,
    #[serde(default)]
    fields: Option<Value>,
    /// `xp-arithmetic` 的逐题结果，每行六个整数（进度是原始位）。
    ///
    /// Per-case results of `xp-arithmetic`, six integers each (progress as raw bits).
    #[serde(default)]
    results: Option<Vec<Value>>,
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
    show: usize,
    cases: usize,
}

impl Report {
    fn check(&mut self, condition: bool, what: impl Into<String>) {
        self.checks += 1;
        if !condition {
            self.failures.push(what.into());
        }
    }

    fn finish(&self) -> Result<(), String> {
        for failure in self.failures.iter().take(self.show.max(1)) {
            eprintln!("  {failure}");
        }
        if self.failures.is_empty() {
            println!(
                "player-data oracle: {} checks passed in {} cases",
                self.checks, self.cases
            );
            Ok(())
        } else {
            Err(format!(
                "player-data oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// 启动官方进程并跑完所有题目。`command` 已经是能运行 `PlayerDataOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let show = parse_show(args)?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let logs = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
    let mut report = Report {
        show,
        ..Default::default()
    };
    let mut lines = BufReader::new(stdout).lines();
    let result = run_cases(&mut stdin, &mut lines, &mut report);
    drop(stdin);
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let logs = logs.recv().unwrap_or_default().join("\n");
    if let Err(error) = &result {
        return Err(format!("{error}\n{logs}"));
    }
    if !status.success() {
        return Err(format!("PlayerDataOracle.java failed: {status}\n{logs}"));
    }
    report.finish()
}

fn parse_show(args: &[&str]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_SHOW),
        ["--show", value] => value.parse().map_err(|_| USAGE.to_owned()),
        _ => Err(USAGE.to_owned()),
    }
}

fn ask(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    request: Value,
) -> Result<Response, String> {
    let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .map_err(|error| format!("cannot write to java: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("cannot flush java stdin: {error}"))?;
    let answer = lines
        .next()
        .ok_or("PlayerDataOracle.java closed stdout unexpectedly")?
        .map_err(|error| format!("cannot read from java: {error}"))?;
    let response: Response =
        serde_json::from_str(&answer).map_err(|error| format!("bad oracle response: {error}"))?;
    if let Some(error) = &response.error {
        return Err(format!("PlayerDataOracle.java: {error}"));
    }
    Ok(response)
}

fn run_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    // 1. 物品（单个复合标签）。
    let cases = item_cases();
    for (name, stack, extra_keys) in &cases {
        let mut tag = slots::encode_item(stack).map_err(|error| format!("{name}: {error}"))?;
        if *extra_keys && let Tag::Compound(entries) = &mut tag {
            entries.push(("bamboo:extra".to_owned(), Tag::Int(77)));
        }
        let nbt = named_bytes(&tag);
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-read", "nbt": b64(&nbt)}),
        )?;
        report.cases += 1;
        report.check(
            answer.original.as_ref() == Some(&canonical_tag(&tag)),
            format!("{name}: official read of a Bamboo item tag differs"),
        );
        // 官方的 MapCodec 忽略多余的键，写回时不再出现。
        let written = answer
            .written_nbt
            .as_deref()
            .ok_or("item-read has no writtenNbt")?;
        let written_tag = parse_named(&decode_b64(written)?)?;
        let clean = slots::encode_item(stack).map_err(|error| error.to_string())?;
        report.check(
            tags_equal(&written_tag, &clean),
            format!("{name}: official item write-back differs from Bamboo's item tag"),
        );
        report.check(
            answer.written.as_ref() == Some(&canonical_tag(&clean)),
            format!("{name}: official item canonical JSON differs"),
        );
    }

    // 2. 带槽位的物品列表：空、满、带组件、数量边界 1 / 99、多余键、坏条目。
    for (name, items, bad) in list_cases() {
        let list = slots::encode_list(&items).map_err(|error| format!("{name}: {error}"))?;
        let mut root = Tag::Compound(vec![("Items".to_owned(), list.clone())]);
        if bad
            && let Tag::Compound(entries) = &mut root
            && let Some((_, Tag::List(values))) = entries.iter_mut().find(|(key, _)| key == "Items")
        {
            values.push(Tag::Int(-1)); // 解不开的条目
        }
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-read", "list": true, "nbt": b64(&named_bytes(&root))}),
        )?;
        report.cases += 1;
        report.check(
            answer.original.as_ref() == Some(&canonical_tag(&root)),
            format!("{name}: official read of a Bamboo item list differs"),
        );
        report.check(
            answer.problems.len() == usize::from(bad),
            format!(
                "{name}: official reported {:?} bad entries, expected {bad}",
                answer.problems
            ),
        );
        let written = answer
            .written_nbt
            .as_deref()
            .ok_or("item-read has no writtenNbt")?;
        let written_tag = parse_named(&decode_b64(written)?)?;
        report.check(
            tags_equal(
                &written_tag,
                &Tag::Compound(vec![("Items".to_owned(), list)]),
            ),
            format!("{name}: official item-list write-back differs"),
        );
        let decoded = slots::decode_list(
            field(&written_tag, "Items").ok_or("writtenNbt has no Items")?,
            INVENTORY_SLOTS,
        );
        report.check(
            decoded.items == items,
            format!("{name}: Bamboo re-decode differs"),
        );
        report.check(
            decoded.problems.is_empty(),
            format!("{name}: Bamboo re-decode of the official write-back has problems"),
        );
    }

    // 官方写出的物品列表，Bamboo 读进来再写出去按键相等。
    for (name, specs) in official_item_specs() {
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-write", "items": specs}),
        )?;
        report.cases += 1;
        let bytes = decode_b64(answer.nbt.as_deref().unwrap_or_default())?;
        let root = parse_named(&bytes)?;
        report.check(
            answer.canonical.as_ref() == Some(&canonical_tag(&root)),
            format!("{name}: official item-list canonical JSON differs"),
        );
        let items = field(&root, "Items").ok_or("official item list has no Items")?;
        let decoded = slots::decode_list(items, INVENTORY_SLOTS);
        let rewritten = slots::encode_list(&decoded.items).map_err(|error| error.to_string())?;
        report.check(
            tags_equal(items, &rewritten),
            format!("{name}: Bamboo item-list write-back differs"),
        );
    }

    // 3. 玩家数据：双向，逐字段用官方 TagValueInput 核对。
    for (name, data) in &player_cases() {
        let bytes = data.to_gzip().map_err(|error| format!("{name}: {error}"))?;
        let answer = ask(
            stdin,
            lines,
            json!({"command": "player-read", "gzip": b64(&bytes)}),
        )?;
        report.cases += 1;
        check_gzip(report, name, &bytes, &answer, "player-read", |gzip| {
            PlayerData::from_gzip(gzip)
                .and_then(|data| data.to_gzip())
                .map_err(|error| error.to_string())
        })?;
        check_player_fields(report, name, data, answer.fields.as_ref());

        // 官方写出的同一份内容，Bamboo 读进来再写出去按键相等。
        let official = ask(
            stdin,
            lines,
            json!({"command": "player-write", "player": player_spec(data)}),
        )?;
        report.cases += 1;
        let gzip = decode_b64(official.gzip.as_deref().unwrap_or_default())?;
        let decoded = PlayerData::from_gzip(&gzip).map_err(|error| format!("{name}: {error}"))?;
        let again = decoded.to_gzip().map_err(|error| error.to_string())?;
        let official_tag = gzip_tag(&gzip)?;
        let again_tag = gzip_tag(&again)?;
        report.check(
            canonical_tag(&official_tag) == canonical_tag(&again_tag),
            format!("{name}: Bamboo's player write-back differs from the official file"),
        );
        report.check(
            official.canonical.as_ref() == Some(&canonical_tag(&official_tag)),
            format!("{name}: official player canonical JSON differs"),
        );
        // D9：官方有而 Bamboo 不认识的键原样写回。
        if official_tag_has_unknown(&official_tag) {
            report.check(
                field(&again_tag, "bamboo:unknown") == field(&official_tag, "bamboo:unknown"),
                format!("{name}: unknown top-level key was not preserved"),
            );
        }
    }

    // 3a. 生命 / 饥饿的运行时存档往返：正式的 `runtime_player_data` →
    // `restore_inventory`，逐字段按原始位比较，不另写一份转换逻辑。
    #[allow(
        clippy::type_complexity,
        reason = "the eight official save fields are passed as one row"
    )]
    let health_cases: [(f32, i16, i16, f32, i32, i32, f32, f32); 5] = [
        (20.0, 0, 0, 0.0, 20, 0, 5.0, 0.0),
        (7.5, 9, 3, 4.25, 13, 77, 1.5, 3.75),
        (0.0, i16::MAX, i16::MAX, 0.0, 0, i32::MAX, 0.0, 39.5),
        (1.0, 1, 0, 0.5, 1, 1, 0.25, 0.1),
        (19.9375, 10, 19, 7.0, 19, 80, 6.0, 12.75),
    ];
    for (index, &(health, hurt, death, absorption, food, tick, saturation, exhaustion)) in
        health_cases.iter().enumerate()
    {
        let got = mc_bamboo_game::replay_health_save(
            health, hurt, death, absorption, food, tick, saturation, exhaustion,
        );
        let expected = [
            health.to_bits() as i32,
            i32::from(hurt),
            i32::from(death),
            absorption.to_bits() as i32,
            food,
            tick,
            saturation.to_bits() as i32,
            exhaustion.to_bits() as i32,
        ];
        report.cases += 1;
        report.check(
            got == expected,
            format!("health save case {index}: official fields {expected:?}, bamboo {got:?}"),
        );
    }

    // 3a2. 玩家属性的运行时存档往返：只存基础值 + 永久修饰符，临时修饰符不落盘，>20 HP 不截断。
    {
        let (max_bits, hp_bits, has_perm, had_transient) = mc_bamboo_game::replay_attributes_save();
        report.cases += 1;
        report.check(
            max_bits == 30.0_f64.to_bits()
                && hp_bits == 28.5_f32.to_bits()
                && has_perm
                && !had_transient,
            format!(
                "attributes save round-trip: max={} hp={} has_perm={has_perm} had_transient={had_transient}",
                f64::from_bits(max_bits),
                f32::from_bits(hp_bits)
            ),
        );
    }

    // 3b. 玩家经验：官方 Player / ServerPlayer 的公式、存档往返，逐位比较。
    run_xp_cases(stdin, lines, report)?;
    run_xp_seed_cases(stdin, lines, report)?;
    run_criteria_cases(stdin, lines, report)?;

    // 4. level.dat：双向，逐字段用官方 PrimaryLevelData.parse 核对。
    for (name, level) in &level_cases() {
        let bytes = level
            .to_gzip()
            .map_err(|error| format!("{name}: {error}"))?;
        let answer = ask(
            stdin,
            lines,
            json!({"command": "level-read", "gzip": b64(&bytes)}),
        )?;
        report.cases += 1;
        check_gzip(report, name, &bytes, &answer, "level-read", |gzip| {
            LevelData::from_gzip(gzip)
                .and_then(|level| level.to_gzip())
                .map_err(|error| error.to_string())
        })?;
        check_level_fields(report, name, level, answer.fields.as_ref());

        let official = ask(
            stdin,
            lines,
            json!({"command": "level-write", "level": level_spec(level)}),
        )?;
        report.cases += 1;
        let gzip = decode_b64(official.gzip.as_deref().unwrap_or_default())?;
        let decoded = LevelData::from_gzip(&gzip).map_err(|error| format!("{name}: {error}"))?;
        let again = decoded.to_gzip().map_err(|error| error.to_string())?;
        let official_tag = gzip_tag(&gzip)?;
        let again_tag = gzip_tag(&again)?;
        report.check(
            canonical_tag(&official_tag) == canonical_tag(&again_tag),
            format!("{name}: Bamboo's level.dat write-back differs from the official file"),
        );
        for key in ["bamboo:unknown", "bamboo:root"] {
            let official_value = field(&official_tag, key);
            if official_value.is_some() {
                report.check(
                    field(&again_tag, key) == official_value,
                    format!("{name}: unknown root key {key} was not preserved"),
                );
            }
        }
        report.check(
            official.canonical.as_ref() == Some(&canonical_tag(&official_tag)),
            format!("{name}: official level.dat canonical JSON differs"),
        );
        if let (Some(Tag::Compound(official_data)), Some(Tag::Compound(again_data))) = (
            field(&official_tag, "Data").cloned(),
            field(&again_tag, "Data").cloned(),
        ) && let Some(unknown) = field_of(&official_data, "bamboo:unknown")
        {
            report.check(
                field_of(&again_data, "bamboo:unknown") == Some(unknown),
                format!("{name}: unknown Data key was not preserved"),
            );
        }
    }
    Ok(())
}

/// Bamboo 写出的 gzip 文件：官方读、官方写回、Bamboo 再读再写，按键比较。
fn check_gzip(
    report: &mut Report,
    name: &str,
    original: &[u8],
    answer: &Response,
    command: &str,
    round_trip: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let original_tag = gzip_tag(original)?;
    report.check(
        answer.original.as_ref() == Some(&canonical_tag(&original_tag)),
        format!("{name}: official read of Bamboo's gzip differs ({command})"),
    );
    let written = answer
        .gzip_written
        .as_deref()
        .ok_or(format!("{command} has no gzipWritten"))?;
    let written_bytes = decode_b64(written)?;
    let written_tag = gzip_tag(&written_bytes)?;
    report.check(
        answer.written.as_ref() == Some(&canonical_tag(&written_tag)),
        format!("{name}: official write-back canonical JSON differs ({command})"),
    );
    let again = round_trip(&written_bytes)?;
    let again_tag = gzip_tag(&again)?;
    let left = canonical_tag(&written_tag);
    let right = canonical_tag(&again_tag);
    report.check(
        left == right,
        format!(
            "{name}: Bamboo write-back of the official {command} differs: official={} again={}",
            serde_json::to_string(&left).unwrap_or_default(),
            serde_json::to_string(&right).unwrap_or_default()
        ),
    );
    Ok(())
}

/// 一组经验公式题：三字段的初值、操作和参数。进度用原始位传，浮点逐位比较。
///
/// One batch of experience-arithmetic cases: initial fields, operation and
/// argument. Progress is carried as raw bits and compared bit for bit.
fn xp_cases() -> Vec<(&'static str, i32, u32, i32, i32, i32, i64)> {
    let bit = |value: f32| value.to_bits();
    let mut cases = Vec::new();
    for (level, progress, total, score) in [
        (0, 0.0_f32, 0, 0),
        (0, 0.5, 3, 12),
        (1, 0.125, 9, -3),
        (14, 0.9, 200, 200),
        (15, 0.0, 350, 7),
        (29, 0.75, 1395, -99),
        (30, 0.5, 2000, 0),
        (45, 0.999_999, 5000, 4321),
        (0, -0.0, 0, 0),
        (2, f32::NAN, 17, 5),
        (3, 0.25, i32::MAX, 0),
        (3, 0.25, i32::MIN, 1),
    ] {
        for points in [0, 1, 7, 17, -1, -8, -100, 2477, 100_000, -100_000] {
            cases.push((
                "give_points",
                level,
                bit(progress),
                total,
                score,
                points,
                0x1234_5678_i64,
            ));
        }
        for levels in [1, 5, -1, -5, i32::MAX, i32::MIN, 0] {
            cases.push((
                "give_levels",
                level,
                bit(progress),
                total,
                score,
                levels,
                0x1234_5678_i64,
            ));
        }
        for points in [0, 1, 6, 7, 30, 1000] {
            cases.push((
                "set_points",
                level,
                bit(progress),
                total,
                score,
                points,
                0x1234_5678_i64,
            ));
        }
        for level_value in [0, 1, 15, 30, 100] {
            cases.push((
                "set_levels",
                level,
                bit(progress),
                total,
                score,
                level_value,
                0x1234_5678_i64,
            ));
        }
        for cost in [0, 1, 3, 30, 1000] {
            cases.push((
                "on_enchantment",
                level,
                bit(progress),
                total,
                score,
                cost,
                0x5eed_1234_5678_i64,
            ));
        }
        cases.push(("query_points", level, bit(progress), total, score, 0, 0));
        cases.push(("need", level, bit(progress), total, score, 0, 0));
    }
    // 存档往返也覆盖极端值：`XpP` 的每一位都要写回。
    cases
}

fn run_xp_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    let cases = xp_cases();
    let specs: Vec<Value> = cases
        .iter()
        .map(|&(op, level, bits, total, score, value, seed)| {
            json!({
                "op": op,
                "level": level,
                "progress_bits": bits as i32,
                "total": total,
                "score": score,
                "value": value,
                "seed": seed,
            })
        })
        .collect();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "xp-arithmetic", "cases": specs}),
    )?;
    let results = answer
        .results
        .as_ref()
        .ok_or("xp-arithmetic has no results")?;
    report.check(
        results.len() == cases.len(),
        format!(
            "xp: official returned {} results, expected {}",
            results.len(),
            cases.len()
        ),
    );
    for (index, (row, case)) in results.iter().zip(&cases).enumerate() {
        let &(op, level, bits, total, score, value, seed) = case;
        let expected =
            mc_bamboo_game::replay_experience(op, level, bits, total, score, value, seed);
        let actual = row
            .as_array()
            .ok_or_else(|| format!("xp case {index}: result is not an array"))?;
        report.cases += 1;
        for (want, got) in expected.iter().zip(actual) {
            report.check(
                got.as_i64() == Some(i64::from(*want)),
                format!(
                    "xp case {index} ({op} level={level} bits={bits} total={total} score={score} value={value}): expected {want}, official {got}"
                ),
            );
        }
    }

    // 存档往返：Bamboo 写出的 XpP/XpLevel/XpTotal/XpSeed/Score 必须原样读回。
    for &(level, bits, total, seed, score) in &[
        (0, 0u32, 0, 0, 0),
        (30, 0.5_f32.to_bits(), 2000, -77, 99),
        (17, 0.125_f32.to_bits(), 1234, 0x1234_5678, -1234),
        (i32::MAX, f32::NAN.to_bits(), i32::MAX, i32::MIN, i32::MIN),
    ] {
        let got = mc_bamboo_game::replay_experience_save(level, bits, total, seed, score);
        // `XpSeed == 0` 时官方会重抽，Bamboo 沿用构造时的种子，所以只比位模式不同的值。
        let want = [
            level,
            bits as i32,
            total,
            if seed == 0 { 0 } else { seed },
            score,
        ];
        report.cases += 1;
        report.check(
            got[0] == want[0] && got[1] == want[1] && got[2] == want[2] && got[4] == want[4],
            format!("xp save: {got:?} differs from {want:?}"),
        );
        if seed != 0 {
            report.check(
                got[3] == want[3],
                format!("xp save: XpSeed changed {got:?} expected {want:?}"),
            );
        }
    }
    Ok(())
}

/// `XpSeed` 读档时的随机数消耗：新玩家 / 存档写 0 要抽一次，存档非 0 不抽。
/// 比较附魔种子本身和「恢复后再取一次 nextInt」的值，多取/少取都会被抓到。
///
/// Random draws on loading `XpSeed`: a new player or a zero saved seed draws
/// once, a non-zero saved seed draws none. Compare the seed and the next
/// `nextInt` after restore so an extra or missing draw is caught.
fn run_xp_seed_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    let cases: Vec<(Option<i32>, i64)> = vec![
        (None, 0),
        (Some(0), 0),
        (Some(-77), 0),
        (Some(i32::MIN), 0x1234_5678_i64),
        (Some(i32::MAX), -0x5eed_1234_i64),
        (None, 0x5eed_1234_5678_i64),
        (Some(0), 0x5eed_1234_5678_i64),
        (Some(12345), 0x5eed_1234_5678_i64),
    ];
    let specs: Vec<Value> = cases
        .iter()
        .map(|&(saved, seed)| {
            json!({
                "saved": saved.unwrap_or(0),
                "seed": seed,
            })
        })
        .collect();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "xp-seed-load", "cases": specs}),
    )?;
    let results = answer
        .results
        .as_ref()
        .ok_or("xp-seed-load has no results")?;
    report.check(
        results.len() == cases.len(),
        format!(
            "xp seed: official returned {} results, expected {}",
            results.len(),
            cases.len()
        ),
    );
    for (index, (row, &(saved, seed))) in results.iter().zip(&cases).enumerate() {
        let official = row
            .as_array()
            .ok_or_else(|| format!("xp seed case {index}: result is not an array"))?;
        let ours = mc_bamboo_game::replay_experience_seed(saved, seed);
        report.cases += 1;
        for (want, got) in ours.iter().zip(official) {
            report.check(
                got.as_i64() == Some(i64::from(*want)),
                format!(
                    "xp seed case {index} (saved={saved:?} seed={seed}) field: official {got}, bamboo {want}"
                ),
            );
        }
    }
    Ok(())
}

/// `ServerPlayer.doTick` 的 xp / level 只读准则，官方用 `Mth.ceil(float)`。
///
/// The xp / level read-only criteria of `ServerPlayer.doTick`, using
/// official `Mth.ceil(float)`.
fn run_criteria_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    let cases: [(i32, i32); 10] = [
        (0, 0),
        (1, 1),
        (7, 1),
        (i32::MAX, 30),
        (i32::MIN, -1),
        (12345, 45),
        (-99, 0),
        (1000, 100),
        (2_147_483_646, 2_147_483_646),
        (0, i32::MAX),
    ];
    let specs: Vec<Value> = cases
        .iter()
        .map(|&(total, level)| json!({"total": total, "level": level}))
        .collect();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "xp-criteria", "cases": specs}),
    )?;
    let results = answer
        .results
        .as_ref()
        .ok_or("xp-criteria has no results")?;
    report.check(
        results.len() == cases.len(),
        format!(
            "criteria: official returned {} results, expected {}",
            results.len(),
            cases.len()
        ),
    );
    for (index, (row, &(total, level))) in results.iter().zip(&cases).enumerate() {
        let official = row
            .as_array()
            .ok_or_else(|| format!("criteria case {index}: result is not an array"))?;
        let ours = mc_bamboo_game::replay_criteria_values(total, level);
        report.cases += 1;
        for (value, got) in [ours[4], ours[5]].iter().zip(official) {
            report.check(
                got.as_i64() == Some(i64::from(*value)),
                format!(
                    "criteria case {index} (total={total} level={level}): official {got}, bamboo {value}"
                ),
            );
        }
    }
    Ok(())
}

fn check_player_fields(report: &mut Report, name: &str, data: &PlayerData, fields: Option<&Value>) {
    let Some(fields) = fields else {
        report.check(false, format!("{name}: player-read has no fields"));
        return;
    };
    let get = |key: &str| fields.get(key).cloned().unwrap_or(Value::Null);
    report.check(
        get("data_version") == json!(data.data_version),
        format!("{name}: DataVersion"),
    );
    report.check(get("air") == json!(data.air), format!("{name}: Air"));
    report.check(
        get("ticks_frozen") == json!(data.ticks_frozen),
        format!("{name}: TicksFrozen"),
    );
    report.check(get("fire") == json!(data.fire), format!("{name}: Fire"));
    report.check(
        get("on_ground") == json!(data.on_ground),
        format!("{name}: OnGround"),
    );
    report.check(
        get("health").as_f64() == Some(f64::from(data.health)),
        format!(
            "{name}: Health official={:?} bamboo={:?}",
            get("health").as_f64(),
            f64::from(data.health)
        ),
    );
    report.check(
        get("xp_level") == json!(data.xp_level),
        format!("{name}: XpLevel"),
    );
    // `XpP` 是 float：用原始位比较，NaN 也要一致。官方 `FloatTag.valueOf`
    // 把 `-0.0` 归一成 +0.0，所以期望值同样归一。
    // `XpP` is a float: compare raw bits so NaN agrees too. Official
    // `FloatTag.valueOf` normalizes `-0.0` to +0.0, so normalize the expectation.
    let expected_bits = if data.xp_p == 0.0 {
        0
    } else {
        data.xp_p.to_bits()
    };
    report.check(
        get("xp_p")
            .as_f64()
            .map(|value| value as f32)
            .map(f32::to_bits)
            == Some(expected_bits),
        format!("{name}: XpP"),
    );
    report.check(
        get("xp_total") == json!(data.xp_total),
        format!("{name}: XpTotal"),
    );
    report.check(
        get("xp_seed") == json!(data.xp_seed),
        format!("{name}: XpSeed"),
    );
    report.check(get("score") == json!(data.score), format!("{name}: Score"));
    // 官方 `GlobalPos.CODEC` 写出来是 {dimension, pos:[x,y,z]} 的规范 JSON。
    let expected_death = match &data.last_death_location {
        Some((dimension, pos)) => json!({
            "type": 10,
            "values": {
                "dimension": {"type": 8, "value": dimension},
                "pos": {"type": 11, "values": [pos[0], pos[1], pos[2]]},
            },
        }),
        None => json!({}),
    };
    report.check(
        get("last_death_location") == expected_death,
        format!("{name}: LastDeathLocation"),
    );
    // `warden_spawn_tracker`：WardenSpawnTracker.CODEC 的三个非负整数，恒写。
    let expected_warden = canonical_tag(&Tag::Compound(vec![
        (
            "ticks_since_last_warning".to_owned(),
            Tag::Int(data.warden_spawn_tracker.ticks_since_last_warning),
        ),
        (
            "warning_level".to_owned(),
            Tag::Int(data.warden_spawn_tracker.warning_level),
        ),
        (
            "cooldown_ticks".to_owned(),
            Tag::Int(data.warden_spawn_tracker.cooldown_ticks),
        ),
    ]));
    report.check(
        get("warden_spawn_tracker") == expected_warden,
        format!(
            "{name}: warden_spawn_tracker official={} bamboo={}",
            get("warden_spawn_tracker"),
            expected_warden
        ),
    );
    report.check(
        get("food_level") == json!(data.food_level),
        format!("{name}: foodLevel"),
    );
    // `attributes`：把 Bamboo 写出的列表按官方 `AttributeInstance.Packed.LIST_CODEC` 再编一次，
    // 和规范 JSON 按键比较。
    let expected_attributes = match &data.attributes {
        Some(Tag::List(attrs)) => canonical_tag(&Tag::List(attrs.clone())),
        _ => json!({"type": 9, "values": []}),
    };
    report.check(
        get("attributes") == expected_attributes,
        format!(
            "{name}: attributes official={} bamboo={}",
            get("attributes"),
            expected_attributes
        ),
    );
    // `active_effects`：把 Bamboo 写出的列表按官方 codec 再编一次，
    // 和规范 JSON 按键比较；空表时官方不写这个键。
    let expected_effects = match &data.active_effects {
        Some(Tag::List(effects)) if !effects.is_empty() => {
            canonical_tag(&Tag::List(effects.clone()))
        }
        _ => json!({"type": 9, "values": []}),
    };
    report.check(
        get("active_effects") == expected_effects,
        format!(
            "{name}: active_effects official={} bamboo={}",
            get("active_effects"),
            expected_effects
        ),
    );
    // `ender_pearls`：官方 `childrenListOrEmpty` 的条数和列表的规范 JSON
    // 都要和 Bamboo 写出的那一份一致。
    let expected_pearls = if data.ender_pearls.is_empty() {
        json!({"type": 9, "values": []})
    } else {
        canonical_tag(&Tag::List(data.ender_pearls.clone()))
    };
    report.check(
        get("ender_pearl_count") == json!(data.ender_pearls.len()),
        format!("{name}: ender_pearls childrenList count"),
    );
    report.check(
        get("ender_pearls") == expected_pearls,
        format!(
            "{name}: ender_pearls NBT official={} bamboo={}",
            get("ender_pearls"),
            expected_pearls
        ),
    );
    // `ServerPlayer.saveParentVehicle` / `loadAndSpawnParentVehicle`：官方按
    // `ValueInput.child("RootVehicle")` 读回，整条标签和 `Entity` 子树逐键一致。
    match &data.root_vehicle {
        Some(tag) => {
            report.check(
                get("root_vehicle_tag") == canonical_tag(tag),
                format!("{name}: RootVehicle official read differs from Bamboo's tag"),
            );
            if let Some(entity) = field(tag, "Entity") {
                report.check(
                    get("root_vehicle_entity") == canonical_tag(entity),
                    format!("{name}: RootVehicle/Entity official read differs"),
                );
            }
            if let Some(attach) = field(tag, "Attach") {
                report.check(
                    get("root_vehicle_attach") == canonical_tag(attach),
                    format!("{name}: RootVehicle/Attach UUID codec differs"),
                );
            }
        }
        None => report.check(
            get("root_vehicle_tag") == Value::Null,
            format!("{name}: RootVehicle must not be written when not riding"),
        ),
    }
    report.check(
        get("selected_item_slot") == json!(data.selected_item_slot),
        format!("{name}: SelectedItemSlot"),
    );
    report.check(
        get("dimension") == json!(data.dimension),
        format!("{name}: Dimension"),
    );
    report.check(
        get("game_type") == json!(data.game_type),
        format!("{name}: playerGameType"),
    );
    report.check(
        get("seen_credits") == json!(data.seen_credits),
        format!("{name}: seenCredits"),
    );
    report.check(
        get("spawn_extra_particles_on_fall") == json!(data.spawn_extra_particles_on_fall),
        format!("{name}: spawn_extra_particles_on_fall"),
    );
    report.check(
        get("inventory")
            == canonical_tag(&slots::encode_list(&data.inventory).unwrap_or(Tag::List(Vec::new()))),
        format!("{name}: Inventory"),
    );
    report.check(
        get("ender_items")
            == canonical_tag(
                &slots::encode_list(&data.ender_items).unwrap_or(Tag::List(Vec::new())),
            ),
        format!("{name}: EnderItems"),
    );
    report.check(
        get("pos")
            == canonical_tag(&Tag::List(
                data.position.iter().map(|v| Tag::Double(*v)).collect(),
            )),
        format!("{name}: Pos"),
    );
    report.check(
        get("rotation")
            == canonical_tag(&Tag::List(
                data.rotation.iter().map(|v| Tag::Float(*v)).collect(),
            )),
        format!("{name}: Rotation"),
    );
    let abilities = canonical_tag(&Tag::Compound(vec![
        (
            "invulnerable".into(),
            Tag::Byte(i8::from(data.abilities.invulnerable)),
        ),
        ("flying".into(), Tag::Byte(i8::from(data.abilities.flying))),
        ("mayfly".into(), Tag::Byte(i8::from(data.abilities.may_fly))),
        (
            "instabuild".into(),
            Tag::Byte(i8::from(data.abilities.instabuild)),
        ),
        (
            "mayBuild".into(),
            Tag::Byte(i8::from(data.abilities.may_build)),
        ),
        ("flySpeed".into(), Tag::Float(data.abilities.fly_speed)),
        ("walkSpeed".into(), Tag::Float(data.abilities.walk_speed)),
    ]));
    report.check(get("abilities") == abilities, format!("{name}: abilities"));
}

fn check_level_fields(report: &mut Report, name: &str, level: &LevelData, fields: Option<&Value>) {
    let Some(fields) = fields else {
        report.check(false, format!("{name}: level-read has no fields"));
        return;
    };
    let get = |key: &str| fields.get(key).cloned().unwrap_or(Value::Null);
    report.check(
        get("level_name") == json!(level.level_name),
        format!("{name}: LevelName"),
    );
    report.check(
        get("game_type") == json!(level.game_type),
        format!("{name}: GameType"),
    );
    report.check(get("time") == json!(level.time), format!("{name}: Time"));
    report.check(
        get("allow_commands") == json!(level.allow_commands),
        format!("{name}: allowCommands"),
    );
    report.check(
        get("initialized") == json!(level.initialized),
        format!("{name}: initialized"),
    );
    let spawn = canonical_tag(&Tag::Compound(vec![
        (
            "dimension".into(),
            Tag::String(level.spawn.dimension.clone()),
        ),
        (
            "pos".into(),
            Tag::IntArray(vec![level.spawn.x, level.spawn.y, level.spawn.z]),
        ),
        ("yaw".into(), Tag::Float(level.spawn.yaw)),
        ("pitch".into(), Tag::Float(level.spawn.pitch)),
    ]));
    report.check(get("spawn") == spawn, format!("{name}: spawn"));
    let difficulty = canonical_tag(&Tag::Compound(vec![
        (
            "difficulty".into(),
            Tag::String(level.difficulty.difficulty.clone()),
        ),
        (
            "hardcore".into(),
            Tag::Byte(i8::from(level.difficulty.hardcore)),
        ),
        (
            "locked".into(),
            Tag::Byte(i8::from(level.difficulty.locked)),
        ),
    ]));
    report.check(
        get("difficulty_settings") == difficulty,
        format!("{name}: difficulty_settings"),
    );
}

fn item_cases() -> Vec<(String, ItemStack, bool)> {
    vec![
        ("item count 1".into(), ItemStack::new(STONE, 1), false),
        ("item count 99".into(), ItemStack::new(STONE, 99), false),
        ("item with damage component".into(), damaged(7), false),
        ("item with extra key".into(), ItemStack::new(STONE, 1), true),
    ]
}

fn damaged(count: i32) -> ItemStack {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
    let mut patch = ComponentPatch::new();
    patch.put(kinds::DAMAGE, PatchEntry::Set(ComponentValue::Damage(3)));
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item: STONE,
        count,
        added,
        removed,
    }
}

fn list_cases() -> Vec<(String, Vec<SlottedItem>, bool)> {
    let full: Vec<_> = (0..INVENTORY_SLOTS)
        .map(|slot| SlottedItem {
            slot: slot as i32,
            stack: ItemStack::new(STONE, 1 + (slot % 99) as i32),
        })
        .collect();
    let mut with_component = full.clone();
    with_component[0] = SlottedItem {
        slot: 0,
        stack: damaged(99),
    };
    vec![
        ("empty inventory".into(), Vec::new(), false),
        (
            "count boundary 1".into(),
            vec![SlottedItem {
                slot: 0,
                stack: ItemStack::new(STONE, 1),
            }],
            false,
        ),
        (
            "count boundary 99".into(),
            vec![SlottedItem {
                slot: 35,
                stack: ItemStack::new(STONE, 99),
            }],
            false,
        ),
        ("full 36-slot inventory".into(), full, false),
        ("inventory with components".into(), with_component, false),
        (
            "inventory with one bad entry".into(),
            vec![
                SlottedItem {
                    slot: 0,
                    stack: ItemStack::new(STONE, 1),
                },
                SlottedItem {
                    slot: 8,
                    stack: damaged(99),
                },
            ],
            true,
        ),
    ]
}

fn official_item_specs() -> Vec<(String, Value)> {
    let mut full = Vec::new();
    for slot in 0..INVENTORY_SLOTS {
        full.push(json!({"slot": slot, "count": 1 + (slot % 99), "component": slot % 3 == 0}));
    }
    vec![
        ("official empty list".into(), json!([])),
        (
            "official count boundaries".into(),
            json!([{"slot": 0, "count": 1}, {"slot": 35, "count": 99, "component": true}]),
        ),
        ("official full list".into(), json!(full)),
    ]
}

fn player_cases() -> Vec<(String, PlayerData)> {
    let mut cases = Vec::new();
    for (name, game_type) in [
        ("survival", 0),
        ("creative", 1),
        ("adventure", 2),
        ("spectator", 3),
    ] {
        let data = PlayerData {
            data_version: version::DATA_VERSION,
            uuid: [0x1234_5678, -0x6543_2110, 0x0fed_cba9, -0x789a_bcdf],
            game_type,
            abilities: Abilities::for_game_type(game_type),
            dimension: player_data::OVERWORLD.to_owned(),
            ..PlayerData::default()
        };
        cases.push((format!("player {name}"), data));
    }
    let mut empty = cases[0].1.clone();
    empty.inventory = Vec::new();
    empty.ender_items = Vec::new();
    cases.push(("player empty inventory".into(), empty));

    let mut full = cases[1].1.clone();
    full.inventory = (0..INVENTORY_SLOTS)
        .map(|slot| SlottedItem {
            slot: slot as i32,
            stack: if slot % 3 == 0 {
                damaged(99)
            } else {
                ItemStack::new(STONE, 1)
            },
        })
        .collect();
    full.ender_items = vec![SlottedItem {
        slot: 26,
        stack: damaged(1),
    }];
    full.selected_item_slot = 8;
    cases.push(("player full inventory with components".into(), full));

    let mut extreme = cases[2].1.clone();
    extreme.position = [1.0e300, -64.0, -1.0e300];
    extreme.motion = [-0.5, 0.0, 1.0 / 3.0];
    extreme.rotation = [-179.5, 89.5];
    extreme.fall_distance = 1.0e100;
    extreme.on_ground = true;
    extreme.portal_cooldown = i32::MAX;
    extreme.xp_p = 0.625;
    extreme.xp_level = 30;
    extreme.xp_total = 12345;
    extreme.xp_seed = -77;
    extreme.score = -99;
    extreme.abilities.fly_speed = 0.25;
    extreme.abilities.walk_speed = 0.5;
    cases.push(("player extreme coordinates".into(), extreme));

    let mut xp_bits = cases[0].1.clone();
    xp_bits.xp_p = -0.0;
    xp_bits.xp_level = 45;
    xp_bits.xp_total = i32::MAX;
    xp_bits.xp_seed = i32::MIN;
    xp_bits.score = i32::MAX;
    cases.push(("player extreme experience".into(), xp_bits));

    // 生命 / 受伤 / 饥饿字段的非默认值往返：1a 起这些键接上运行时状态，
    // `runtime_player_data` 和 `restore_inventory` 都要逐字段保真。
    let mut wounded = cases[0].1.clone();
    wounded.health = 7.5;
    wounded.hurt_time = 9;
    wounded.death_time = 3;
    wounded.absorption = 4.25;
    wounded.food_level = 13;
    wounded.food_tick_timer = 77;
    wounded.food_saturation = 1.5;
    wounded.food_exhaustion = 3.75;
    cases.push(("player wounded health and food".into(), wounded));

    let mut extreme_food = cases[1].1.clone();
    extreme_food.health = 0.0;
    extreme_food.absorption = -0.0;
    extreme_food.hurt_time = i16::MAX;
    extreme_food.death_time = i16::MAX;
    extreme_food.food_level = 0;
    extreme_food.food_tick_timer = i32::MAX;
    extreme_food.food_saturation = 0.0;
    extreme_food.food_exhaustion = 39.5;
    cases.push(("player extreme health and food".into(), extreme_food));

    // 监守者警告等级：WardenSpawnTracker.CODEC 的三个非负整数，带非默认值往返。
    let mut warned = cases[0].1.clone();
    warned.warden_spawn_tracker = player_data::WardenSpawnTracker {
        ticks_since_last_warning: 12345,
        warning_level: 3,
        cooldown_ticks: 200,
    };
    cases.push(("player warden warning".into(), warned));

    // `ShoulderEntityLeft` / `Right`：整只鹦鹉的实体 NBT。官方
    // `ServerPlayer.addAdditionalSaveData` 只在非空时写这两个键。
    let parrot_left = Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:parrot".to_owned())),
        ("Variant".to_owned(), Tag::Int(2)),
    ]);
    let parrot_right = Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:parrot".to_owned())),
        ("Variant".to_owned(), Tag::Int(4)),
        ("Silent".to_owned(), Tag::Byte(1)),
    ]);
    let mut both_shoulders = cases[0].1.clone();
    both_shoulders.shoulder_left = Some(parrot_left.clone());
    both_shoulders.shoulder_right = Some(parrot_right);
    cases.push(("player both shoulder parrots".into(), both_shoulders));
    let mut left_only = cases[0].1.clone();
    left_only.shoulder_left = Some(parrot_left);
    cases.push(("player left shoulder parrot".into(), left_only));

    // 1b：`LastDeathLocation` 只有 Optional 非空才写，`GlobalPos.CODEC` 是
    // dimension 字符串 + pos 三个 int 的复合标签。
    let mut died = cases[0].1.clone();
    died.health = 0.0;
    died.death_time = 5;
    died.last_death_location = Some((player_data::OVERWORLD.to_owned(), [-1234, -59, 9876]));
    cases.push(("player last death location".into(), died));

    // S2：`active_effects` 的非默认往返（`id` / `amplifier` / `duration` /
    // `ambient` / `show_particles` / `show_icon` / `hidden_effect`）。
    let mut with_effects = cases[0].1.clone();
    // 用生产代码的 `effects_tag` 造标签，官方 codec 的省键规则由它负责。
    use mc_bamboo_blocks::entity::mob::{
        effects::{EffectInstance, EffectMap},
        nbt::effects_tag,
    };
    let mut effects = EffectMap::new();
    let mut regeneration = EffectInstance::with_flags(9, 1234, 2, true, false);
    regeneration.show_icon = true;
    regeneration.hidden = Some(Box::new(EffectInstance::new(9, 600)));
    effects.add(regeneration);
    let mut speed = EffectInstance::with_amplifier(0, -1, 255);
    speed.ambient = false;
    effects.add(speed);
    with_effects.active_effects = Some(effects_tag(&effects));
    cases.push(("player active effects".into(), with_effects));

    // 玩家属性表：`AttributeInstance.Packed.LIST_CODEC` 的双向往返（空表、
    // 改基础值、三种永久修饰符、以及不会落盘的临时修饰符）。
    {
        use mc_bamboo_blocks::entity::mob::{
            attributes::{Attributes, Modifier, Operation},
            nbt::attributes_tag,
        };
        use mc_bamboo_data::registries::attribute;

        let mut empty_attrs = cases[0].1.clone();
        empty_attrs.attributes = Some(attributes_tag(&Attributes::player()));
        cases.push(("player empty attributes".into(), empty_attrs));

        let mut with_attrs = cases[0].1.clone();
        let mut attrs = Attributes::player();
        if let Some(inst) = attrs.instance(attribute::MAX_HEALTH) {
            inst.base = 30.0;
            inst.modifiers.push(Modifier {
                id: "minecraft:perm_add".to_owned(),
                amount: 4.5,
                operation: Operation::AddValue,
                permanent: true,
            });
            inst.modifiers.push(Modifier {
                id: "minecraft:transient_skip".to_owned(),
                amount: 99.0,
                operation: Operation::AddValue,
                permanent: false,
            });
        }
        if let Some(inst) = attrs.instance(attribute::MOVEMENT_SPEED) {
            inst.modifiers.push(Modifier {
                id: "minecraft:perm_mul_base".to_owned(),
                amount: 0.25,
                operation: Operation::AddMultipliedBase,
                permanent: true,
            });
            inst.modifiers.push(Modifier {
                id: "minecraft:perm_mul_total".to_owned(),
                amount: -0.1,
                operation: Operation::AddMultipliedTotal,
                permanent: true,
            });
        }
        if let Some(inst) = attrs.instance(attribute::ATTACK_DAMAGE) {
            inst.base = 3.5;
        }
        with_attrs.attributes = Some(attributes_tag(&attrs));
        cases.push((
            "player attributes with permanent modifiers".into(),
            with_attrs,
        ));
    }

    let mut frozen = cases[0].1.clone();
    frozen.fire = 200;
    frozen.air = 137;
    frozen.ticks_frozen = 77;
    cases.push(("player fire air and frost".into(), frozen));

    let mut unknown = cases[3].1.clone();
    let mut tag = unknown.encode();
    if let Tag::Compound(entries) = &mut tag {
        entries.push(("bamboo:unknown".into(), Tag::String("keep".into())));
        if let Some((_, Tag::Compound(abilities))) =
            entries.iter_mut().find(|(key, _)| key == "abilities")
        {
            abilities.push(("bamboo:nested".into(), Tag::Long(9)));
        }
    }
    let mut plain = Vec::new();
    write_named("", &tag, &mut plain);
    unknown = PlayerData::from_named(&plain).expect("unknown keys decode");
    cases.push(("player unknown keys".into(), unknown));

    // 末影珍珠：`ender_pearls` 是 childrenList，每项是一份完整的
    // `ThrownEnderpearl.save()` 实体 NBT 加 `ender_pearl_dimension`。
    let pearl = |x: f64, y: f64, z: f64, owner: bool| -> Tag {
        let item = slots::encode_item(&ItemStack::new(mc_bamboo_data::items::ENDER_PEARL, 1))
            .expect("pearl item encodes");
        let mut entries = vec![
            ("id".into(), Tag::String("minecraft:ender_pearl".into())),
            (
                "Pos".into(),
                Tag::List(vec![Tag::Double(x), Tag::Double(y), Tag::Double(z)]),
            ),
            (
                "Motion".into(),
                Tag::List(vec![
                    Tag::Double(1.25),
                    Tag::Double(-0.03),
                    Tag::Double(0.0),
                ]),
            ),
            (
                "Rotation".into(),
                Tag::List(vec![Tag::Float(90.0), Tag::Float(-4.5)]),
            ),
            ("fall_distance".into(), Tag::Double(0.0)),
            ("Fire".into(), Tag::Short(0)),
            ("Air".into(), Tag::Short(300)),
            ("OnGround".into(), Tag::Byte(0)),
            ("Invulnerable".into(), Tag::Byte(0)),
            ("PortalCooldown".into(), Tag::Int(0)),
            (
                "UUID".into(),
                Tag::IntArray(vec![0, 0x0000_baab_u32 as i32, 0, 77]),
            ),
            ("LeftOwner".into(), Tag::Byte(1)),
            ("HasBeenShot".into(), Tag::Byte(1)),
            ("Item".into(), item),
            (
                "ender_pearl_dimension".into(),
                Tag::String(player_data::OVERWORLD.to_owned()),
            ),
        ];
        if owner {
            entries.push(("Owner".into(), Tag::IntArray(vec![1, 2, 3, 4])));
        }
        Tag::Compound(entries)
    };
    let mut one_pearl = cases[0].1.clone();
    one_pearl.ender_pearls = vec![pearl(3.5, 70.25, -8.0, true)];
    cases.push(("player one ender pearl".into(), one_pearl));

    let mut two_pearls = cases[1].1.clone();
    two_pearls.ender_pearls = vec![
        pearl(1.0e300, -64.0, -1.0e300, false),
        // 官方 `DoubleTag.valueOf` 把 -0.0 归一成 +0.0，这里用普通 0.0。
        pearl(0.0, 320.0, 0.125, true),
    ];
    cases.push(("player two ender pearls".into(), two_pearls));

    // `ServerPlayer.saveParentVehicle`：根载具连同嵌套的 `Passengers` 一起写进玩家档。
    // 官方一侧按 `ValueInput.child("RootVehicle")` 读回，逐键和 Bamboo 写的比。
    let mut mounted = cases[0].1.clone();
    mounted.root_vehicle = Some(root_vehicle_tag(true));
    cases.push(("player root vehicle with nested passengers".into(), mounted));
    let mut simple_mount = cases[2].1.clone();
    simple_mount.root_vehicle = Some(root_vehicle_tag(false));
    cases.push((
        "player root vehicle without passengers".into(),
        simple_mount,
    ));
    // 骑着上鞍的猪下线：用 Bamboo 自己存出来的 `RootVehicle`（猪的装备、血量、乘客），
    // 官方一侧同样读回逐键比。
    // Riding a saddled pig: Bamboo's own saved `RootVehicle` (equipment, health, passenger)
    // read back and compared key by key.
    if let Some(tag) = mc_bamboo_game::pig_riding::replay_pig_root_vehicle() {
        let mut pig_mount = cases[0].1.clone();
        pig_mount.root_vehicle = Some(tag);
        cases.push(("player root vehicle saddled pig".into(), pig_mount));
    }
    cases
}

/// 一份带 `Attach` 和 `Entity` 的 `RootVehicle`；`Passengers` 递归一层。
fn root_vehicle_tag(nested: bool) -> Tag {
    let mut entity = vec![
        ("id".into(), Tag::String("minecraft:horse".into())),
        (
            "Pos".into(),
            Tag::List(vec![
                Tag::Double(2.5),
                Tag::Double(64.0),
                Tag::Double(-8.75),
            ]),
        ),
        (
            "Rotation".into(),
            Tag::List(vec![Tag::Float(90.0), Tag::Float(0.0)]),
        ),
        (
            "UUID".into(),
            Tag::IntArray(vec![0, 0x0000_baab_u32 as i32, 0, 91]),
        ),
        ("SaddleItem".into(), Tag::Compound(vec![])),
    ];
    if nested {
        entity.push((
            "Passengers".into(),
            Tag::List(vec![Tag::Compound(vec![
                ("id".into(), Tag::String("minecraft:skeleton".into())),
                (
                    "Pos".into(),
                    Tag::List(vec![
                        Tag::Double(2.5),
                        Tag::Double(65.0),
                        Tag::Double(-8.75),
                    ]),
                ),
            ])]),
        ));
    }
    Tag::Compound(vec![
        (
            "Attach".into(),
            Tag::IntArray(vec![0, 0x0000_baab_u32 as i32, 0, 91]),
        ),
        ("Entity".into(), Tag::Compound(entity)),
    ])
}

fn player_spec(data: &PlayerData) -> Value {
    let abilities = json!({
        "invulnerable": data.abilities.invulnerable,
        "flying": data.abilities.flying,
        "may_fly": data.abilities.may_fly,
        "instabuild": data.abilities.instabuild,
        "may_build": data.abilities.may_build,
        "fly_speed": data.abilities.fly_speed,
        "walk_speed": data.abilities.walk_speed,
    });
    let items = |items: &[SlottedItem]| -> Value {
        items
            .iter()
            .map(|item| {
                let component = !item.stack.added.is_empty() || !item.stack.removed.is_empty();
                json!({"slot": item.slot, "count": item.stack.count, "component": component})
            })
            .collect::<Vec<_>>()
            .into()
    };
    let mut spec = json!({
        "pos": data.position,
        "motion": data.motion,
        "rotation": data.rotation,
        "fall_distance": data.fall_distance,
        "fire": data.fire,
        "air": data.air,
        "ticks_frozen": data.ticks_frozen,
        "on_ground": data.on_ground,
        "invulnerable": data.invulnerable,
        "portal_cooldown": data.portal_cooldown,
        "uuid_high": uuid_high(data.uuid),
        "uuid_low": uuid_low(data.uuid),
        "health": data.health,
        "hurt_time": data.hurt_time,
        "death_time": data.death_time,
        "absorption": data.absorption,
        "fall_flying": data.fall_flying,
        "inventory": items(&data.inventory),
        "selected_item_slot": data.selected_item_slot,
        "sleep_timer": data.sleep_timer,
        "xp_p": data.xp_p,
        "xp_level": data.xp_level,
        "xp_total": data.xp_total,
        "xp_seed": data.xp_seed,
        "score": data.score,
        "food_level": data.food_level,
        "food_tick_timer": data.food_tick_timer,
        "food_saturation": data.food_saturation,
        "food_exhaustion": data.food_exhaustion,
        "abilities": abilities,
        "ender_items": items(&data.ender_items),
        "dimension": data.dimension,
        "game_type": data.game_type,
        "seen_credits": data.seen_credits,
        "spawn_extra_particles_on_fall": data.spawn_extra_particles_on_fall,
        "warden_spawn_tracker": {
            "ticks_since_last_warning": data.warden_spawn_tracker.ticks_since_last_warning,
            "warning_level": data.warden_spawn_tracker.warning_level,
            "cooldown_ticks": data.warden_spawn_tracker.cooldown_ticks,
        },
    });
    if let Some(previous) = data.previous_game_type {
        spec["previous_game_type"] = json!(previous);
    }
    if let Some((dimension, pos)) = &data.last_death_location {
        spec["last_death_dimension"] = json!(dimension);
        spec["last_death_x"] = json!(pos[0]);
        spec["last_death_y"] = json!(pos[1]);
        spec["last_death_z"] = json!(pos[2]);
    }
    if !data.ender_pearls.is_empty() {
        // 每项是 Bamboo 写出的「带空名字的复合标签」字节，Java 侧用官方
        // `NbtIo.read` 解回来再装进 `ender_pearls` 列表。
        let pearls: Vec<Value> = data
            .ender_pearls
            .iter()
            .map(|tag| {
                let mut bytes = Vec::new();
                mc_bamboo_nbt::write_named("", tag, &mut bytes);
                json!(b64(&bytes))
            })
            .collect();
        spec["ender_pearls"] = json!(pearls);
    }
    if let Some(tag) = &data.root_vehicle {
        // Java 侧用官方 `NbtIo.read` 解回整条复合标签；`Attach` 是 UUIDUtil.CODEC
        // 的四项 int array，`Entity` 里可以有嵌套的 `Passengers`。
        let mut bytes = Vec::new();
        mc_bamboo_nbt::write_named("", tag, &mut bytes);
        spec["root_vehicle"] = json!(b64(&bytes));
    }
    if !data.preserved.is_empty() {
        spec["unknown_tag"] = json!("keep");
    }
    if let Some(Tag::List(attrs)) = &data.attributes {
        let rows: Vec<Value> = attrs
            .iter()
            .filter_map(|tag| {
                let Tag::Compound(entries) = tag else {
                    return None;
                };
                let get = |key: &str| {
                    entries
                        .iter()
                        .rev()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                let id = match get("id") {
                    Some(Tag::String(v)) => v,
                    _ => return None,
                };
                let base = match get("base") {
                    Some(Tag::Double(v)) => v,
                    _ => 0.0,
                };
                let mut row = json!({"id": id, "base": base});
                if let Some(Tag::List(mods)) = get("modifiers") {
                    let mod_rows: Vec<Value> = mods
                        .iter()
                        .filter_map(|m| {
                            let Tag::Compound(me) = m else {
                                return None;
                            };
                            let mget = |key: &str| {
                                me.iter()
                                    .rev()
                                    .find(|(name, _)| name == key)
                                    .map(|(_, value)| value.clone())
                            };
                            let mid = match mget("id") {
                                Some(Tag::String(v)) => v,
                                _ => return None,
                            };
                            let amount = match mget("amount") {
                                Some(Tag::Double(v)) => v,
                                _ => 0.0,
                            };
                            let op = match mget("operation") {
                                Some(Tag::String(v)) => v,
                                _ => return None,
                            };
                            Some(json!({"id": mid, "amount": amount, "operation": op}))
                        })
                        .collect();
                    row["modifiers"] = json!(mod_rows);
                }
                Some(row)
            })
            .collect();
        spec["attributes"] = json!(rows);
    }
    if let Some(Tag::List(effects)) = &data.active_effects {
        let rows: Vec<Value> = effects
            .iter()
            .filter_map(|tag| {
                let Tag::Compound(entries) = tag else {
                    return None;
                };
                let get = |key: &str| {
                    entries
                        .iter()
                        .rev()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                let text = |key: &str| match get(key) {
                    Some(Tag::String(value)) => value,
                    _ => String::new(),
                };
                let int = |key: &str, default: i32| match get(key) {
                    Some(Tag::Int(value)) => value,
                    _ => default,
                };
                let bool_flag = |key: &str, default: bool| match get(key) {
                    Some(Tag::Byte(value)) => value != 0,
                    _ => default,
                };
                let mut row = json!({
                    "id": text("id"),
                    "amplifier": int("amplifier", 0),
                    "duration": int("duration", 0),
                    "ambient": bool_flag("ambient", false),
                    "show_particles": bool_flag("show_particles", true),
                    "show_icon": bool_flag("show_icon", bool_flag("show_particles", true)),
                });
                if get("hidden_effect").is_some() {
                    // 官方 codec 的 `hidden_effect` 有自己的默认值；这里只放一层，
                    // 让 Java 侧 `MobEffectInstance.CODEC.parse` 补齐缺省键。
                    if let Some(Tag::Compound(hidden)) = get("hidden_effect") {
                        row["hidden_effect"] = json!({
                            "amplifier": hidden.iter().rev()
                                .find(|(k, _)| k == "amplifier")
                                .and_then(|(_, v)| match v { Tag::Int(v) => Some(*v), _ => None })
                                .unwrap_or(0),
                            "duration": hidden.iter().rev()
                                .find(|(k, _)| k == "duration")
                                .and_then(|(_, v)| match v { Tag::Int(v) => Some(*v), _ => None })
                                .unwrap_or(0),
                            "ambient": false,
                            "show_particles": true,
                        });
                    }
                }
                Some(row)
            })
            .collect();
        spec["active_effects"] = json!(rows);
    }
    spec
}

fn level_cases() -> Vec<(String, LevelData)> {
    let mut cases = Vec::new();
    for (name, game_type) in [
        ("survival", 0),
        ("creative", 1),
        ("adventure", 2),
        ("spectator", 3),
    ] {
        let level = LevelData {
            game_type,
            level_name: format!("bamboo-{name}"),
            time: 12345 + i64::from(game_type),
            last_played: 1_700_000_000_000,
            spawn: SpawnData {
                dimension: level_dat::OVERWORLD.to_owned(),
                x: 8,
                y: -60,
                z: 8,
                yaw: 0.0,
                pitch: 0.0,
            },
            ..LevelData::default()
        };
        cases.push((format!("level {name}"), level));
    }
    let mut extreme = cases[2].1.clone();
    extreme.spawn = SpawnData {
        x: 30_000_000,
        y: -2032,
        z: -30_000_000,
        yaw: 179.5,
        pitch: -90.0,
        dimension: level_dat::OVERWORLD.to_owned(),
    };
    extreme.time = 1_000_000_000_007;
    extreme.last_played = 1_700_000_000_123;
    extreme.level_name = "竹 bamboo".to_owned();
    extreme.difficulty = level_dat::DifficultySettings {
        difficulty: "hard".into(),
        hardcore: true,
        locked: true,
    };
    extreme.version_history = vec![4000, 5023];
    extreme.allow_commands = true;
    extreme.initialized = false;
    cases.push(("level extreme values".into(), extreme));

    // 官方有而 Bamboo 不认识的键：顶层、Data、Version 各放一个。
    let mut unknown = cases[0].1.clone();
    let mut root = unknown.encode_root();
    if let Tag::Compound(entries) = &mut root {
        entries.push(("bamboo:root".into(), Tag::String("keep".into())));
        if let Some((_, Tag::Compound(data))) = entries.iter_mut().find(|(key, _)| key == "Data") {
            data.push(("bamboo:data".into(), Tag::Long(-7)));
            if let Some((_, Tag::Compound(version))) =
                data.iter_mut().find(|(key, _)| key == "Version")
            {
                version.push(("bamboo:version".into(), Tag::Byte(3)));
            }
        }
    }
    let mut plain = Vec::new();
    write_named("", &root, &mut plain);
    unknown = LevelData::from_named(&plain).expect("unknown keys decode");
    cases.push(("level unknown keys".into(), unknown));
    cases
}

fn level_spec(level: &LevelData) -> Value {
    json!({
        "level_name": level.level_name,
        "game_type": level.game_type,
        "difficulty": level.difficulty.difficulty,
        "hardcore": level.difficulty.hardcore,
        "locked": level.difficulty.locked,
        "allow_commands": level.allow_commands,
        "initialized": level.initialized,
        "time": level.time,
        "last_played": level.last_played,
        "spawn": {
            "x": level.spawn.x, "y": level.spawn.y, "z": level.spawn.z,
            "yaw": level.spawn.yaw, "pitch": level.spawn.pitch,
        },
    })
}

fn uuid_high(uuid: [i32; 4]) -> i64 {
    (i64::from(uuid[0]) << 32) | (i64::from(uuid[1]) & 0xffff_ffff)
}

fn uuid_low(uuid: [i32; 4]) -> i64 {
    (i64::from(uuid[2]) << 32) | (i64::from(uuid[3]) & 0xffff_ffff)
}

fn official_tag_has_unknown(tag: &Tag) -> bool {
    field(tag, "bamboo:unknown").is_some()
}

fn field_of<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => field_of(entries, key),
        _ => None,
    }
}

fn named_bytes(tag: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    write_named("", tag, &mut out);
    out
}

fn parse_named(bytes: &[u8]) -> Result<Tag, String> {
    let (name, entries) = read_named(bytes, Accounter::unlimited()).map_err(|e| e.to_string())?;
    if !name.is_empty() {
        return Err(format!("root name {name:?} is not empty"));
    }
    Ok(Tag::Compound(entries))
}

fn gzip_tag(bytes: &[u8]) -> Result<Tag, String> {
    let (name, entries) = mc_bamboo_nbt::read_compressed(
        bytes,
        mc_bamboo_nbt::Compression::Gzip,
        Accounter::unlimited(),
    )
    .map_err(|e| e.to_string())?;
    if !name.is_empty() {
        return Err(format!("gzip root name {name:?} is not empty"));
    }
    Ok(Tag::Compound(entries))
}

/// 官方 `String.valueOf(float)` 出来的文本，和 `NumericTag.box().toString()` 一致。
fn java_f32(value: f32) -> String {
    mc_bamboo_nbt::java_float_to_string(value)
}

fn tags_equal(left: &Tag, right: &Tag) -> bool {
    canonical_tag(left) == canonical_tag(right)
}

fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!({"type": 1, "value": value.to_string()}),
        Tag::Short(value) => json!({"type": 2, "value": value.to_string()}),
        Tag::Int(value) => json!({"type": 3, "value": value.to_string()}),
        Tag::Long(value) => json!({"type": 4, "value": value.to_string()}),
        Tag::Float(value) => json!({"type": 5, "value": java_f32(*value)}),
        Tag::Double(value) => {
            json!({"type": 6, "value": mc_bamboo_nbt::java_double_to_string(*value)})
        }
        Tag::String(value) => json!({"type": 8, "value": value}),
        Tag::ByteArray(values) => json!({"type": 7, "values": values}),
        Tag::IntArray(values) => json!({"type": 11, "values": values}),
        Tag::LongArray(values) => json!({"type": 12, "values": values}),
        Tag::List(values) => json!({
            "type": 9,
            "values": values.iter().map(canonical_tag).collect::<Vec<_>>()
        }),
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut map = serde_json::Map::new();
            for (key, value) in sorted {
                map.insert(key.clone(), canonical_tag(value));
            }
            json!({"type": 10, "values": Value::Object(map)})
        }
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64_stub::encode;
    encode(bytes)
}

fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    base64_stub::decode(text)
}

/// 自带的 base64，避免为这个开发工具再引入一个依赖。
mod base64_stub {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let a = chunk[0] as u32;
            let b = chunk.get(1).copied().unwrap_or(0) as u32;
            let c = chunk.get(2).copied().unwrap_or(0) as u32;
            let value = a << 16 | b << 8 | c;
            out.push(ALPHABET[(value >> 18) as usize & 63] as char);
            out.push(ALPHABET[(value >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(value >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[value as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(input: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut buffer = 0_u32;
        let mut bits = 0;
        for byte in input.bytes() {
            if byte == b'=' {
                break;
            }
            let value = ALPHABET
                .iter()
                .position(|&candidate| candidate == byte)
                .ok_or_else(|| format!("bad base64 byte {byte}"))? as u32;
            buffer = buffer << 6 | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        Ok(out)
    }
}
