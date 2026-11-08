//! 用官方代码检查 `mc-bamboo-protocol` 的物品组件编解码。
//!
//! `ComponentOracle.java` 把一大批真实的组件补丁按官方 `DataComponentPatch.STREAM_CODEC`
//! 编码成十六进制；这边解码再编码，要求逐字节相同，然后把自己写出来的字节回传给官方解码，
//! 官方解出来的补丁要和原来的 `equals`。
//!
//! 题目来自每个物品的默认组件、创造模式物品栏的全部物品，以及一批手写的组件。
//! 跑完会列出一个组件类型都没覆盖到的那些类型。
//!
//! 还有几组额外的检查：
//!
//! - 每个物品的默认组件：生成的 `item_components.rs` 要和官方现在编出来的字节一样；
//! - 按 ID 查表的枚举：喂一个越界的原始 ID 进去，官方折回成什么，Bamboo 也要折回成什么；
//! - 同一批物品堆的存档 NBT（`ItemStack.CODEC` 和 `DataComponentPatch.CODEC`）以及
//!   `HashedStack.create` 的组件哈希；
//! - SNBT 的打印和解析，对照官方 `StringTagVisitor`、`SnbtPrinterTagVisitor` 和 `TagParser`。
//!
//! Extra checks besides the network patch bytes:
//!
//! - generated default components must match the bytes the official codec writes now;
//! - by-id enums must fold the same way;
//! - the same stacks' archive NBT (`ItemStack.CODEC` and `DataComponentPatch.CODEC`)
//!   and the `HashedStack.create` component hashes;
//! - SNBT printing and parsing against `StringTagVisitor`, `SnbtPrinterTagVisitor`,
//!   and `TagParser`.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};

use mc_bamboo_data::{item_components, items, version};
use mc_bamboo_nbt::{Accounter, DEFAULT_QUOTA, Reader, Tag, parse_snbt};
use mc_bamboo_protocol::{
    codec::DecodeError,
    components::{
        ComponentPatch,
        archive::stack::{nbt_equal, transcode_patch, transcode_stack},
        cursor::Cursor,
    },
};
use serde::Deserialize;

const USAGE: &str = "usage: components <java> [--show N]";
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
        /// 按注册表 ID 排列的组件类型名。
        component_types: Vec<String>,
    },
    Case {
        index: usize,
        name: String,
        /// 物品的注册表 ID，只有「某个物品的默认组件」这种题才有，其余是 -1。
        item: i32,
        bytes: String,
    },
    /// 一个按 ID 查表的枚举：喂进去的字节和官方折回之后写出来的字节。
    Fold {
        name: String,
        input: String,
        output: String,
        /// 官方解不开时的原因，解得开就是空。
        error: Option<String>,
    },
    Skipped {
        case: String,
        error: String,
    },
    End {
        cases: usize,
    },
    /// 同一份补丁的存档 NBT，以及官方 `HashedStack.create` 的哈希。
    ///
    /// Archive NBT for one patch, plus the `HashedStack.create` hashes.
    Archive {
        index: usize,
        name: String,
        stack: String,
        patch: String,
        added: Vec<(i32, i32)>,
        removed: Vec<i32>,
        #[serde(default)]
        skip_hash: bool,
    },
    /// 官方把一个标签打印成紧凑和缩进两种 SNBT，再解析回去。
    ///
    /// Official compact and pretty SNBT for one tag, parsed back to NBT.
    Snbt {
        name: String,
        source: String,
        compact: String,
        pretty: String,
        parsed: String,
        pretty_parsed: String,
    },
    /// 一段手写的 SNBT。官方解析失败时 `error` 有内容，`parsed` 为空。
    ///
    /// A handwritten SNBT string. `error` is set when official parsing fails.
    SnbtInput {
        text: String,
        error: Option<String>,
        parsed: Option<String>,
    },
    Verified {
        checked: usize,
        mismatches: Vec<String>,
    },
}

/// 一次运行的计数和不一致。
#[derive(Debug, Default)]
struct Report {
    component_types: Vec<String>,
    cases: usize,
    covered: BTreeSet<i32>,
    /// 回传给官方的网络补丁，一行一道题。
    ///
    /// Network-patch replies, one line per case.
    replies: Vec<String>,
    /// 回传给官方的存档 NBT，一行一道存档题。
    ///
    /// Archive NBT replies, one line per archive case.
    archive_replies: Vec<String>,
    archive: usize,
    snbt: usize,
    mismatches: Vec<String>,
    skipped: Vec<String>,
    /// 检查过默认组件的物品数。
    item_defaults: usize,
    /// 试过的枚举名和每个枚举试了几个 ID。
    folds: BTreeMap<String, usize>,
    show: usize,
    ended: bool,
    verified: Option<usize>,
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let show = parse_options(args)?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = crate::oracle::collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;

    let mut report = Report {
        show,
        ..Report::default()
    };
    let mut source = BufReader::new(stdout);
    let mut result = report.read_cases(&mut source);
    if result.is_ok() {
        result = report.send_replies(&mut stdin);
    }
    // 官方那边读到管道结束才往下走，不关掉的话两边互相等。
    drop(stdin);
    if result.is_ok() {
        result = report.read_verdict(&mut source);
    }
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    result.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("ComponentOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

fn parse_options(args: &[&str]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_SHOW),
        ["--show", value] => value.parse().map_err(|_| USAGE.to_owned()),
        _ => Err(USAGE.to_owned()),
    }
}

impl Report {
    /// 读官方写的题目，边读边比较，顺手准备回传的字节。
    fn read_cases(&mut self, source: &mut BufReader<impl Read>) -> Result<(), String> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = source
                .read_line(&mut line)
                .map_err(|error| format!("cannot read from java: {error}"))?;
            if read == 0 {
                return Err("java stopped before the end line".to_owned());
            }
            match parse(&line)? {
                Line::Header {
                    version,
                    protocol,
                    component_types,
                } => self.check_header(&version, protocol, component_types)?,
                Line::Case {
                    index,
                    name,
                    item,
                    bytes,
                } => self.check_case(index, &name, item, &bytes)?,
                Line::Fold {
                    name,
                    input,
                    output,
                    error,
                } => self.check_fold(&name, &input, &output, error.as_deref())?,
                Line::Skipped { case, error } => self.skipped.push(format!("{case}: {error}")),
                Line::Archive {
                    index,
                    name,
                    stack,
                    patch,
                    added,
                    removed,
                    skip_hash,
                } => self.check_archive(
                    index,
                    &name,
                    (&stack, &patch),
                    (&added, &removed, skip_hash),
                )?,
                Line::Snbt {
                    name,
                    source,
                    compact,
                    pretty,
                    parsed,
                    pretty_parsed,
                } => self.check_snbt(&name, &source, &compact, &pretty, &parsed, &pretty_parsed)?,
                Line::SnbtInput {
                    text,
                    error,
                    parsed,
                } => self.check_snbt_input(&text, error.as_deref(), parsed.as_deref())?,
                Line::End { cases } => {
                    self.ended = true;
                    if cases != self.cases {
                        return Err(format!("java wrote {cases} cases, {} arrived", self.cases));
                    }
                    return Ok(());
                }
                Line::Verified { .. } => {
                    return Err("verdict arrived before the end line".to_owned());
                }
            }
        }
    }

    fn check_header(
        &mut self,
        game_version: &str,
        protocol: i32,
        component_types: Vec<String>,
    ) -> Result<(), String> {
        if game_version != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {game_version} (protocol {protocol})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
            ));
        }
        self.component_types = component_types;
        Ok(())
    }

    /// 解码、再编码、比较，然后记下要回传的字节。
    fn check_case(&mut self, index: usize, name: &str, item: i32, hex: &str) -> Result<(), String> {
        if index != self.cases {
            return Err(format!("case {index} arrived as number {}", self.cases));
        }
        self.cases += 1;

        let bytes = unhex(hex)?;
        if item >= 0 {
            self.check_item_defaults(name, item, &bytes);
        }
        match decode(&bytes) {
            Ok(patch) => {
                for (kind, _) in patch.entries() {
                    self.covered.insert(*kind);
                }
                let mut again = Vec::new();
                patch.write(&mut again);
                if again == bytes {
                    self.replies.push(format!("{index} {hex}"));
                } else {
                    self.mismatch(format!(
                        "{name}: official {hex}, bamboo wrote back {}",
                        to_hex(&again)
                    ));
                    self.replies.push(format!("{index} {}", to_hex(&again)));
                }
            }
            Err(error) => {
                self.mismatch(format!("{name}: bamboo cannot decode {hex}: {error}"));
                // 解不出来就把原样的字节回传，官方那边不会再报一次同样的问题。
                self.replies.push(format!("{index} {hex}"));
            }
        }
        Ok(())
    }

    /// 生成的默认组件要和官方现在编出来的字节一样，解出来再编回去也要一样。
    fn check_item_defaults(&mut self, name: &str, item: i32, official: &[u8]) {
        self.item_defaults += 1;
        let Some(&(start, end)) = item_components::RANGES.get(item as usize) else {
            self.mismatch(format!("{name}: 生成的数据里没有物品 {item}"));
            return;
        };
        let generated = &item_components::DEFAULTS[start as usize..end as usize];
        if generated != official {
            self.mismatch(format!(
                "{name}: generated {}, official {}",
                to_hex(generated),
                to_hex(official)
            ));
            return;
        }

        match decode(generated) {
            Ok(patch) => {
                let mut again = Vec::new();
                patch.write(&mut again);
                if again != generated {
                    self.mismatch(format!(
                        "{name}: 生成的默认组件写回去变成了 {}",
                        to_hex(&again)
                    ));
                }
            }
            Err(error) => {
                self.mismatch(format!("{name}: 生成的默认组件解不开：{error}"));
            }
        }
    }

    /// 按 ID 查表的枚举：官方折回成什么，Bamboo 也要折回成什么。
    fn check_fold(
        &mut self,
        name: &str,
        input: &str,
        output: &str,
        official_error: Option<&str>,
    ) -> Result<(), String> {
        let enum_name = name.split_whitespace().next().unwrap_or(name).to_owned();
        *self.folds.entry(enum_name).or_default() += 1;

        let input_bytes = unhex(input)?;
        let expected = unhex(output)?;
        match (decode(&input_bytes), official_error) {
            (Ok(patch), None) => {
                let mut again = Vec::new();
                patch.write(&mut again);
                if again != expected {
                    self.mismatch(format!(
                        "fold {name}: official {output}, bamboo {}",
                        to_hex(&again)
                    ));
                }
            }
            (Ok(_), Some(error)) => self.mismatch(format!(
                "fold {name}: official rejects it ({error}), bamboo accepts it"
            )),
            (Err(error), None) => self.mismatch(format!(
                "fold {name}: official folds it to {output}, bamboo cannot decode: {error}"
            )),
            // 两边都不接受，一致。
            (Err(_), Some(_)) => {}
        }
        Ok(())
    }

    fn send_replies(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        for reply in self.replies.iter().chain(&self.archive_replies) {
            writeln!(stdin, "{reply}").map_err(|error| format!("cannot write to java: {error}"))?;
        }
        stdin
            .flush()
            .map_err(|error| format!("cannot write to java: {error}"))
    }

    fn read_verdict(&mut self, source: &mut BufReader<impl Read>) -> Result<(), String> {
        let mut line = String::new();
        source
            .read_line(&mut line)
            .map_err(|error| format!("cannot read from java: {error}"))?;
        match parse(&line)? {
            Line::Verified {
                checked,
                mismatches,
            } => {
                self.verified = Some(checked);
                for mismatch in mismatches {
                    self.mismatch(format!("official read back a different patch: {mismatch}"));
                }
                Ok(())
            }
            _ => Err(format!("expected a verdict, got {}", line.trim())),
        }
    }

    /// 存档 NBT 往返，以及 `HashedStack.create` 的哈希。
    ///
    /// Archive NBT round trip, and the `HashedStack.create` hashes.
    fn check_archive(
        &mut self,
        index: usize,
        name: &str,
        nbt: (&str, &str),
        hashes: (&[(i32, i32)], &[i32], bool),
    ) -> Result<(), String> {
        let (stack_hex, patch_hex) = nbt;
        let (added, removed, skip_hash) = hashes;
        if index != self.archive {
            return Err(format!(
                "archive case {index} arrived as number {}",
                self.archive
            ));
        }
        self.archive += 1;
        let stack_bytes = unhex(stack_hex)?;
        let patch_bytes = unhex(patch_hex)?;
        let stack_out = match transcode_stack(&stack_bytes) {
            Ok(stack) => {
                if !nbt_equal(&stack.bytes, &stack_bytes) {
                    self.mismatch(format!(
                        "{name}: stack nbt differs, official {stack_hex}, bamboo {}",
                        to_hex(&stack.bytes)
                    ));
                }
                if !skip_hash && !same_ids(&stack.added, added) {
                    self.mismatch(format!(
                        "{name}: added hashes differ, official {added:?}, bamboo {:?}",
                        stack.added
                    ));
                }
                if !skip_hash && !same_ids(removed, &stack.removed) {
                    self.mismatch(format!(
                        "{name}: removed components differ, official {removed:?}, bamboo {:?}",
                        stack.removed
                    ));
                }
                to_hex(&stack.bytes)
            }
            Err(error) => {
                self.mismatch(format!("{name}: bamboo cannot decode stack nbt: {error}"));
                stack_hex.to_owned()
            }
        };
        let patch_out = match transcode_patch(&patch_bytes) {
            Ok(bytes) => {
                if !nbt_equal(&bytes, &patch_bytes) {
                    self.mismatch(format!(
                        "{name}: patch nbt differs, official {patch_hex}, bamboo {}",
                        to_hex(&bytes)
                    ));
                }
                to_hex(&bytes)
            }
            Err(error) => {
                self.mismatch(format!("{name}: bamboo cannot decode patch nbt: {error}"));
                patch_hex.to_owned()
            }
        };
        self.archive_replies
            .push(format!("A {index} {stack_out} {patch_out}"));
        Ok(())
    }

    /// 同一份标签，官方和 Bamboo 打印出来的 SNBT 要一样；再把这段 SNBT 解析回去，
    /// 两边的标签也要一样。打印和解析分开比：`NaNf` 这种字面量官方解析成字符串，
    /// 再打印就和原来的浮点不一样。
    ///
    /// The same tag must print as the same SNBT, and parsing that SNBT must produce
    /// the same tag. Printing and parsing are separate: official parsing turns a
    /// literal such as `NaNf` into a string, so printing the parsed tag does not
    /// reproduce the original float.
    fn check_snbt(
        &mut self,
        name: &str,
        source: &str,
        compact: &str,
        pretty: &str,
        parsed: &str,
        pretty_parsed: &str,
    ) -> Result<(), String> {
        self.snbt += 1;
        let source = read_tag(&unhex(source)?)?;
        let printed = source.to_snbt();
        if printed != compact {
            self.mismatch(format!(
                "{name}: compact SNBT differs, official {compact}, bamboo {printed}"
            ));
        }
        let pretty_printed = source.to_pretty_snbt();
        if pretty_printed != pretty {
            self.mismatch(format!(
                "{name}: pretty SNBT differs, official {pretty}, bamboo {pretty_printed}"
            ));
        }
        let official = unhex(parsed)?;
        let pretty_official = unhex(pretty_parsed)?;
        match parse_snbt(compact) {
            Ok(tag) => {
                if !nbt_equal(&write_tag(&tag), &official) {
                    self.mismatch(format!("{name}: compact SNBT parsed to a different tag"));
                }
            }
            Err(error) => {
                self.mismatch(format!(
                    "{name}: bamboo cannot parse compact SNBT {compact}: {error}"
                ));
            }
        }
        match parse_snbt(pretty) {
            Ok(tag) => {
                if !nbt_equal(&write_tag(&tag), &pretty_official) {
                    self.mismatch(format!("{name}: pretty SNBT parsed to a different tag"));
                }
            }
            Err(error) => {
                self.mismatch(format!("{name}: bamboo cannot parse pretty SNBT: {error}"));
            }
        }
        Ok(())
    }

    /// 手写的 SNBT。合法输入两边解析结果一样，非法输入两边都拒绝。
    ///
    /// Handwritten SNBT. Legal input parses the same; illegal input fails on both sides.
    fn check_snbt_input(
        &mut self,
        text: &str,
        official_error: Option<&str>,
        parsed: Option<&str>,
    ) -> Result<(), String> {
        self.snbt += 1;
        match (parse_snbt(text), official_error, parsed) {
            (Ok(tag), None, Some(parsed)) => {
                let official = unhex(parsed)?;
                if !nbt_equal(&write_tag(&tag), &official) {
                    self.mismatch(format!(
                        "snbt {text}: parsed tag differs, official {parsed}, bamboo {}",
                        to_hex(&write_tag(&tag))
                    ));
                }
            }
            (Ok(_), Some(error), _) => self.mismatch(format!(
                "snbt {text}: official rejects it ({error}), bamboo accepts it"
            )),
            (Err(error), None, _) => self.mismatch(format!(
                "snbt {text}: official accepts it, bamboo rejects it: {error}"
            )),
            (Err(_), Some(_), _) => {}
            (Ok(_), None, None) => {
                self.mismatch(format!("snbt {text}: official accepted it without a tag"));
            }
        }
        Ok(())
    }

    fn mismatch(&mut self, text: String) {
        self.mismatches.push(text);
    }

    fn finish(&self) -> Result<(), String> {
        if !self.ended {
            return Err("java did not write an end line".to_owned());
        }
        let checked = self.verified.ok_or("java did not write a verdict")?;
        if checked != self.cases {
            return Err(format!(
                "bamboo sent {} cases back, official verified {checked}",
                self.cases
            ));
        }

        let uncovered: Vec<&str> = self
            .component_types
            .iter()
            .enumerate()
            .filter(|(kind, _)| !self.covered.contains(&(*kind as i32)))
            .map(|(_, name)| name.as_str())
            .collect();

        if self.item_defaults != items::ITEMS.len() {
            return Err(format!(
                "官方给了 {} 个物品的默认组件，物品有 {} 个",
                self.item_defaults,
                items::ITEMS.len()
            ));
        }

        println!(
            "COMPONENTS: {} cases, {} of {} component types covered",
            self.cases,
            self.covered.len(),
            self.component_types.len()
        );
        println!(
            "COMPONENTS ITEM DEFAULTS: {} items match the generated table",
            self.item_defaults
        );
        let folds: Vec<String> = self
            .folds
            .iter()
            .map(|(name, count)| format!("{name}({count})"))
            .collect();
        println!(
            "COMPONENTS ENUM FOLDS: {} enums, {}",
            self.folds.len(),
            folds.join(" ")
        );
        println!(
            "COMPONENTS ARCHIVE: {} stacks, COMPONENTS SNBT: {} samples",
            self.archive, self.snbt
        );
        if !uncovered.is_empty() {
            println!("COMPONENTS UNCOVERED: {}", uncovered.join(" "));
        }
        for skipped in &self.skipped {
            println!("COMPONENTS SKIPPED CASE: {skipped}");
        }

        if self.mismatches.is_empty() {
            println!("COMPONENTS RESULT: ok");
            return Ok(());
        }
        for mismatch in self.mismatches.iter().take(self.show) {
            println!("COMPONENTS MISMATCH: {mismatch}");
        }
        Err(format!(
            "COMPONENTS RESULT: {} of {} cases differ",
            self.mismatches.len(),
            self.cases
        ))
    }
}

/// 解码一份补丁，要求刚好用完全部字节。
fn decode(bytes: &[u8]) -> Result<ComponentPatch, DecodeError> {
    let mut cursor = Cursor::new(bytes);
    let patch = ComponentPatch::read(&mut cursor)?;
    cursor.finish()?;
    Ok(patch)
}

fn parse(line: &str) -> Result<Line, String> {
    serde_json::from_str(line.trim()).map_err(|error| format!("bad component oracle line: {error}"))
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("odd hex length {}", text.len()));
    }
    (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|error| format!("bad hex: {error}"))
        })
        .collect()
}

fn read_tag(bytes: &[u8]) -> Result<Tag, String> {
    let mut reader = Reader::new(bytes, Accounter::new(DEFAULT_QUOTA));
    let tag = reader.tag().map_err(|error| error.to_string())?;
    if reader.read() != bytes.len() {
        return Err("trailing nbt".to_owned());
    }
    Ok(tag)
}

fn write_tag(tag: &Tag) -> Vec<u8> {
    let mut bytes = Vec::new();
    tag.write_network(&mut bytes);
    bytes
}

fn same_ids<T: Ord + Clone>(left: &[T], right: &[T]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort();
    right.sort();
    left == right
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex("00ff10"), Ok(vec![0, 255, 16]));
        assert_eq!(to_hex(&[0, 255, 16]), "00ff10");
        assert!(unhex("0").is_err());
        assert!(unhex("zz").is_err());
    }

    #[test]
    fn options_are_parsed() {
        assert_eq!(parse_options(&[]), Ok(DEFAULT_SHOW));
        assert_eq!(parse_options(&["--show", "3"]), Ok(3));
        assert!(parse_options(&["--nope"]).is_err());
    }
}
