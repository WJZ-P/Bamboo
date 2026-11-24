//! 进度树、坐标、包字节和存档 JSON 的官方对拍。
//!
//! `AdvancementOracle.java` 在进程内加载官方 26.3 的资源和 `ServerAdvancementManager`，
//! 不启动服务端、不接受 EULA。默认 JVM 下官方的兄弟顺序由身份哈希决定，每次进程都可能
//! 不同；本对拍把当次顺序导出，作为 Bamboo 排版的输入，逐项比较全部显示节点的坐标和
//! 单条 `UpdateAdvancements` 包的字节。至少跑 3 个独立 Java 进程来覆盖不同排列。
//!
//! Vanilla comparison for the advancement tree, layout, packet bytes and save file.
//! No server or EULA: the Java side loads the official manager in-process.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use mc_bamboo_advancements::{
    AdvancementTree, Frame, TreeOrder,
    progress_file::{AdvancementProgress, ProgressFile, parse_obtained_time},
};
use mc_bamboo_data::version;
use serde_json::Value;

use crate::oracle::collect_log;

const DEFAULT_RUNS: usize = 3;
const DEFAULT_SHOW: usize = 8;

#[derive(Debug)]
struct Options {
    runs: usize,
    show: usize,
    break_layout: bool,
    break_definitions: bool,
    break_packet: bool,
    break_save: bool,
    /// 把排版对不上的那次兄弟顺序连同官方坐标写到这个目录，每个失败一组文件。
    save_failures: Option<String>,
    /// 只跑 Bamboo 这一侧，读回存档的兄弟顺序和官方坐标重放比较，不起 Java 进程。
    replay: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            runs: DEFAULT_RUNS,
            show: DEFAULT_SHOW,
            break_layout: false,
            break_definitions: false,
            break_packet: false,
            break_save: false,
            save_failures: None,
            replay: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--runs" => {
                    options.runs = args
                        .next()
                        .ok_or("--runs 需要数量")?
                        .parse()
                        .map_err(|_| "--runs 需要正整数".to_owned())?;
                }
                "--show" => {
                    options.show = args
                        .next()
                        .ok_or("--show 需要数量")?
                        .parse()
                        .map_err(|_| "--show 需要非负整数".to_owned())?;
                }
                "--save-failures" => {
                    options.save_failures =
                        Some(args.next().ok_or("--save-failures 需要目录")?.to_string());
                }
                "--replay" => {
                    options.replay =
                        Some(args.next().ok_or("--replay 需要文件或目录")?.to_string());
                }
                // 改坏确认：四处独立开关，每处都要被对拍报出来。
                "--break-layout" => options.break_layout = true,
                "--break-definitions" => options.break_definitions = true,
                "--break-packet" => options.break_packet = true,
                "--break-save" => options.break_save = true,
                _ => return Err(format!("未知参数 {arg}")),
            }
        }
        if options.runs == 0 {
            return Err("--runs 至少要 1".to_owned());
        }
        Ok(options)
    }
}

/// 一组可重放的排版输入：`external_layout_order` 的完整结果加上官方坐标。
/// x / y 用 f64 存 JSON 数字，重放时按 f32 比较，和在线对拍的读取方式一致。
/// One replayable layout case: the full external sibling order plus official coordinates.
#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayCase {
    digest: String,
    child_order: Vec<Vec<String>>,
    positions: Vec<ReplayPosition>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayPosition {
    id: String,
    x: f64,
    y: f64,
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
    show: usize,
}

impl Report {
    fn check(&mut self, ok: bool, what: impl Into<String>) {
        self.checks += 1;
        if !ok {
            self.failures.push(what.into());
        }
    }

    fn finish(&self) -> Result<(), String> {
        for failure in self.failures.iter().take(self.show.max(1)) {
            eprintln!("  {failure}");
        }
        if self.failures.is_empty() {
            println!("advancement-oracle: {} checks passed", self.checks);
            Ok(())
        } else {
            Err(format!(
                "advancement-oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// `command` 已经是能运行 `AdvancementOracle.java` 的命令。
pub fn run(command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    if let Some(path) = &options.replay {
        return replay(path, options.show);
    }
    let mut report = Report {
        show: options.show,
        ..Default::default()
    };
    let tree_once = |subcommand: &'static str, command: &Command| -> Result<Vec<Value>, String> {
        let mut copy = clone_command(command);
        copy.arg(subcommand);
        lines(copy)
    };
    let command = command;

    // 1. 全部定义：id、父子、判据名和触发器类型、requirements、显示、奖励。
    let tree_lines = tree_once("tree", &command)?;
    compare_definitions(&tree_lines, &mut report, options.break_definitions)?;

    // 2. 至少 3 个独立 Java 进程，每次的兄弟顺序都不同；每次比较全部坐标和包字节。
    // 同一个 JVM 里先导出兄弟顺序再算坐标，`layout` 输出自带 order 行，不用另起进程。
    let mut digests = Vec::new();
    for run in 0..options.runs {
        let mut run_command = clone_command(&command);
        run_command.args(["layout", &format!("{}", run * 7)]);
        let layout_lines = lines(run_command)?;
        // 同一份 `layout-node` 兄弟顺序：Java 侧从跑三步的那棵官方排版树导出。
        let digest = layout_digest(&layout_lines);
        digests.push(digest.clone());
        let child_order = external_layout_order(&layout_lines)?;
        let mut tree = AdvancementTree::parse(
            mc_bamboo_data::ADVANCEMENTS_JSON,
            TreeOrder::External,
            Some(&child_order),
        )
        .map_err(|error| format!("解析进度树失败：{error}"))?;
        if options.break_layout && run == 0 {
            // 打乱第一棵有显示的根下的兄弟顺序，坐标和包字节必须报出来。
            let root = tree.roots[0];
            let mut broken = child_order.clone();
            broken[root].reverse();
            tree.apply_order(TreeOrder::External, Some(&broken))
                .map_err(|error| format!("改坏排版失败：{error}"))?;
        }
        let failures_before = report.failures.len();
        compare_layout(
            &tree,
            &layout_lines,
            run,
            &digest,
            &mut report,
            options.break_packet,
        )?;
        if let Some(dir) = &options.save_failures {
            if report.failures.len() > failures_before {
                save_failure(dir, run, &digest, &child_order, &layout_lines)?;
            }
        }
    }
    report.check(
        digests.len() >= 3
            && digests
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 2,
        "三次运行的官方兄弟顺序应该至少有两组不同",
    );
    println!(
        "advancement-oracle: {} 个独立 Java 进程，兄弟顺序摘要 {:?}",
        digests.len(),
        digests
    );

    // 3. 存档 JSON：官方写出的对象，Bamboo 读回再写，键顺序和值都要一致。
    let save_lines = tree_once("save", &command)?;
    compare_save(&save_lines, &mut report, options.break_save)?;
    report.finish()
}

/// `Command` 没有 Clone，对拍要跑多次，从程序路径和参数重建一份。
fn clone_command(source: &Command) -> Command {
    let mut copy = Command::new(source.get_program());
    copy.args(source.get_args());
    if let Some(dir) = source.get_current_dir() {
        copy.current_dir(dir);
    }
    copy
}

fn lines(mut command: Command) -> Result<Vec<Value>, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法运行 java：{error}"))?;
    let logs = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java 没有 stdout")?;
    let mut result = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("读取 java stdout 失败：{error}"))?;
        if line.trim().is_empty() || line.starts_with('[') {
            continue;
        }
        result.push(
            serde_json::from_str(&line)
                .map_err(|error| format!("官方输出不是 JSON：{error}\n{line}"))?,
        );
    }
    let status = child
        .wait()
        .map_err(|error| format!("无法等待 java：{error}"))?;
    let logs = logs.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("AdvancementOracle.java 失败了：{status}\n{logs}"));
    }
    Ok(result)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn compare_definitions(
    lines: &[Value],
    report: &mut Report,
    break_self_test: bool,
) -> Result<(), String> {
    let header = lines
        .iter()
        .find(|line| line["type"] == "header")
        .ok_or("官方没有输出 header")?;
    report.check(
        header["version"] == version::GAME_VERSION,
        "官方的游戏版本和生成数据不一致",
    );
    let tree = AdvancementTree::global();
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0;
    if break_self_test {
        report.check(false, "改坏确认：故意写错一个进度定义");
    }
    for line in lines.iter().filter(|line| line["type"] == "definition") {
        let id = line["id"].as_str().ok_or("definition 没有 id")?;
        total += 1;
        seen.insert(id.to_owned());
        let node = tree
            .node(id)
            .unwrap_or_else(|| panic!("Bamboo 缺进度 {id}"));
        let parent = line["parent"].as_str().unwrap_or("");
        report.check(
            node.advancement.parent.as_deref().unwrap_or("") == parent,
            format!("{id}: 父进度不同"),
        );
        let official_criteria = line["criteria"]
            .as_object()
            .ok_or("definition 的 criteria 不是对象")?;
        let names: std::collections::BTreeSet<&str> =
            official_criteria.keys().map(String::as_str).collect();
        let ours: std::collections::BTreeSet<&str> = node
            .advancement
            .criteria
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        report.check(names == ours, format!("{id}: 判据名集合不同"));
        for (name, criterion) in official_criteria {
            let trigger = criterion["trigger"].as_str().unwrap_or("");
            let ours = node
                .advancement
                .criteria
                .iter()
                .find(|(ours_name, _)| ours_name == name)
                .map(|(_, criterion)| criterion.trigger.as_str())
                .unwrap_or("");
            report.check(trigger == ours, format!("{id}/{name}: 触发器类型不同"));
        }
        let official_requirements: Vec<Vec<String>> = line["requirements"]
            .as_array()
            .ok_or("requirements 不是数组")?
            .iter()
            .map(|group| strings(Some(group)))
            .collect();
        // 官方的 requirements 按名字排序过，Bamboo 保留数据包顺序，按集合比较。
        let canon = |groups: &[Vec<String>]| -> std::collections::BTreeSet<Vec<String>> {
            groups
                .iter()
                .map(|group| {
                    let mut group = group.clone();
                    group.sort();
                    group
                })
                .collect()
        };
        report.check(
            canon(&official_requirements) == canon(&node.advancement.requirements),
            format!("{id}: requirements 不同"),
        );
        let display = &line["display"];
        let has_display = display.as_object().is_some_and(|object| !object.is_empty());
        report.check(
            has_display == node.advancement.display.is_some(),
            format!("{id}: 显示信息有无不同"),
        );
        if let Some(ours) = &node.advancement.display {
            let official_frame = display["frame"].as_str().unwrap_or("task");
            report.check(
                Frame::from_name(official_frame) == Some(ours.frame),
                format!("{id}: 边框不同"),
            );
            report.check(
                display["showToast"].as_bool().unwrap_or(true) == ours.show_toast,
                format!("{id}: show_toast 不同"),
            );
            report.check(
                display["announceToChat"].as_bool().unwrap_or(true) == ours.announce_to_chat,
                format!("{id}: announce_to_chat 不同"),
            );
            report.check(
                display["hidden"].as_bool().unwrap_or(false) == ours.hidden,
                format!("{id}: hidden 不同"),
            );
            let background = display["background"].as_str().unwrap_or("");
            report.check(
                ours.background.as_deref().unwrap_or("") == background,
                format!("{id}: background 不同"),
            );
        }
        let rewards = &line["rewards"];
        report.check(
            rewards["experience"].as_i64().unwrap_or(0) as i32
                == node.advancement.rewards.experience,
            format!("{id}: 经验奖励不同"),
        );
        let mut loot = strings(rewards.get("loot"));
        let mut ours_loot = node.advancement.rewards.loot.clone();
        loot.sort();
        ours_loot.sort();
        report.check(loot == ours_loot, format!("{id}: 战利品奖励不同"));
        let mut recipes = strings(rewards.get("recipes"));
        let mut ours_recipes = node.advancement.rewards.recipes.clone();
        recipes.sort();
        ours_recipes.sort();
        report.check(recipes == ours_recipes, format!("{id}: 配方奖励不同"));
        report.check(
            line["telemetry"].as_bool().unwrap_or(false) == node.advancement.sends_telemetry_event,
            format!("{id}: 遥测标记不同"),
        );
    }
    report.check(
        total == header["total"].as_i64().unwrap_or(0) as usize,
        "官方导出的定义数和 header 不一致",
    );
    report.check(total == tree.nodes.len(), "Bamboo 的进度数和官方不一致");
    report.check(seen.len() == tree.nodes.len(), "Bamboo 有官方没有的进度");
    Ok(())
}

fn layout_digest(lines: &[Value]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for line in lines.iter().filter(|line| line["type"] == "layout-node") {
        let children = line["children"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if children.len() > 1 {
            digest.update(line["id"].as_str().unwrap_or("").as_bytes());
            for child in children {
                digest.update(child.as_str().unwrap_or("").as_bytes());
                digest.update([0xff]);
            }
        }
    }
    format!("{:x}", digest.finalize())[..12].to_owned()
}

/// 把 `layout-node` 行铺成按定义 id 排列的兄弟顺序。官方的排版树只含有显示节点。
fn external_layout_order(lines: &[Value]) -> Result<Vec<Vec<String>>, String> {
    let definitions: Value = serde_json::from_str(mc_bamboo_data::ADVANCEMENTS_JSON)
        .map_err(|error| error.to_string())?;
    let ids: Vec<&str> = definitions["advancements"]
        .as_object()
        .ok_or("内嵌数据不是对象")?
        .keys()
        .map(String::as_str)
        .collect();
    let mut result: Vec<Vec<String>> = (0..ids.len()).map(|_| Vec::new()).collect();
    for line in lines.iter().filter(|line| line["type"] == "layout-node") {
        let id = line["id"].as_str().ok_or("layout-node 没有 id")?;
        let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
            continue;
        };
        result[index] = strings(line.get("children"));
    }
    Ok(result)
}

/// 把官方 `position` 行里的坐标抽出来，供存档和离线重放用。
fn official_positions(lines: &[Value]) -> Vec<ReplayPosition> {
    lines
        .iter()
        .filter(|line| line["type"] == "position")
        .filter_map(|line| {
            Some(ReplayPosition {
                id: line["id"].as_str()?.to_owned(),
                x: line["x"].as_f64()?,
                y: line["y"].as_f64()?,
            })
        })
        .collect()
}

/// 把一次排版对不上的完整兄弟顺序和官方坐标写成一个 JSON 文件。
/// Save one mismatching run's sibling order and official coordinates to a JSON file.
fn save_failure(
    dir: &str,
    run: usize,
    digest: &str,
    child_order: &[Vec<String>],
    lines: &[Value],
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("无法创建 {dir}：{error}"))?;
    let case = ReplayCase {
        digest: digest.to_owned(),
        child_order: child_order.to_vec(),
        positions: official_positions(lines),
    };
    let path = Path::new(dir).join(format!("{run}-{digest}.json"));
    let text = serde_json::to_string_pretty(&case).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| format!("无法写 {path:?}：{error}"))?;
    eprintln!("advancement-oracle: 已把出错顺序存到 {}", path.display());
    Ok(())
}

fn replay_files(path: &str) -> Result<Vec<PathBuf>, String> {
    let target = Path::new(path);
    if target.is_file() {
        return Ok(vec![target.to_path_buf()]);
    }
    if target.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(target)
            .map_err(|error| format!("无法读取 {path}：{error}"))?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|file| {
                file.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        files.sort();
        return Ok(files);
    }
    Err(format!("重放路径不存在：{path}"))
}

/// 读回存档，只跑 Bamboo 一侧的排版，和存下的官方坐标逐项比较，不起 Java 进程。
/// Replay stored cases with the Bamboo layout only, comparing against stored official coordinates.
fn replay(path: &str, show: usize) -> Result<(), String> {
    let files = replay_files(path)?;
    if files.is_empty() {
        return Err(format!("{path} 里没有可重放的 JSON 文件"));
    }
    let mut report = Report {
        show,
        ..Default::default()
    };
    for file in &files {
        let text = fs::read_to_string(file).map_err(|error| format!("无法读 {file:?}：{error}"))?;
        let case: ReplayCase = serde_json::from_str(&text)
            .map_err(|error| format!("{file:?} 不是合法的存档：{error}"))?;
        let tree = AdvancementTree::parse(
            mc_bamboo_data::ADVANCEMENTS_JSON,
            TreeOrder::External,
            Some(&case.child_order),
        )
        .map_err(|error| format!("解析进度树失败：{error}"))?;
        for position in &case.positions {
            let label = format!("{} {}:{}", case.digest, file.display(), position.id);
            let Some(index) = tree.index(&position.id) else {
                report.check(false, format!("{label}: Bamboo 缺这个进度"));
                continue;
            };
            let x = position.x as f32;
            let y = position.y as f32;
            report.check(
                tree.nodes[index].x.to_bits() == x.to_bits(),
                format!("{label}: x 不同"),
            );
            report.check(
                tree.nodes[index].y.to_bits() == y.to_bits(),
                format!(
                    "{label}: y 不同 (ours {}, official {y})",
                    tree.nodes[index].y
                ),
            );
        }
        report.check(
            case.positions.len() == tree.displayed.len(),
            format!("{}: 官方显示节点数不同", case.digest),
        );
    }
    println!("advancement-oracle: 重放了 {} 组兄弟顺序", files.len());
    report.finish()
}

fn compare_layout(
    tree: &AdvancementTree,
    lines: &[Value],
    run: usize,
    digest: &str,
    report: &mut Report,
    break_packet: bool,
) -> Result<(), String> {
    let mut seen = 0;
    for line in lines.iter().filter(|line| line["type"] == "position") {
        let id = line["id"].as_str().ok_or("position 没有 id")?;
        let index = tree
            .index(id)
            .unwrap_or_else(|| panic!("Bamboo 缺进度 {id}"));
        let x = line["x"].as_f64().ok_or("position 没有 x")? as f32;
        let y = line["y"].as_f64().ok_or("position 没有 y")? as f32;
        report.check(
            tree.nodes[index].x.to_bits() == x.to_bits(),
            format!("第 {run} 次（顺序 {digest}）{id}: x 不同"),
        );
        report.check(
            tree.nodes[index].y.to_bits() == y.to_bits(),
            format!(
                "第 {run} 次（顺序 {digest}）{id}: y 不同 (ours {}, official {y})",
                tree.nodes[index].y
            ),
        );
        // 官方单条包的字节：包内容由 Bamboo 的协议层重写一遍。
        let official = base64_decode(line["packet"].as_str().unwrap_or(""))?;
        let ours = single_update_packet(tree, index, x, y);
        let ours = if break_packet && seen == 0 {
            let mut broken = ours;
            if let Some(byte) = broken.first_mut() {
                *byte ^= 1;
            }
            broken
        } else {
            ours
        };
        report.check(
            official == ours,
            format!("第 {run} 次（顺序 {digest}）{id}: 包字节不同"),
        );
        seen += 1;
    }
    report.check(seen == tree.displayed.len(), "官方显示节点数不同");
    Ok(())
}

/// 用 Bamboo 自己的显示编码组成和官方同一条单节点 update 包，坐标用官方给的，
/// 这样只检查显示/包编码；坐标的正确性由上面两条单独检查。
fn single_update_packet(tree: &AdvancementTree, index: usize, x: f32, y: f32) -> Vec<u8> {
    use mc_bamboo_protocol::codec::{write_bool, write_f32, write_string, write_varint};
    let mut out = Vec::new();
    write_bool(&mut out, true); // reset
    write_varint(&mut out, 1); // added
    write_string(&mut out, tree.nodes[index].id);
    tree.nodes[index].advancement.write_network(&mut out);
    write_f32(&mut out, x);
    write_f32(&mut out, y);
    write_varint(&mut out, 0); // removed
    write_varint(&mut out, 0); // progress
    write_bool(&mut out, true); // showAdvancements
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    // 仓库里没有 base64 依赖；实现只服务 oracle 的 ASCII 输出。
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        let value = ALPHABET
            .iter()
            .position(|&candidate| candidate == byte)
            .ok_or_else(|| format!("base64 有非法字符 {byte}"))? as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

fn compare_save(lines: &[Value], report: &mut Report, break_self_test: bool) -> Result<(), String> {
    let official = lines
        .iter()
        .find(|line| line["type"] == "save")
        .and_then(|line| line["json"].as_str())
        .ok_or("官方没有输出存档 JSON")?;
    let official_value: Value =
        serde_json::from_str(official).map_err(|error| format!("官方存档不是 JSON：{error}"))?;
    let file = ProgressFile::decode(official).map_err(|error| error.to_string())?;
    report.check(
        file.data_version == version::DATA_VERSION,
        "存档 DataVersion 不同",
    );
    if break_self_test {
        report.check(false, "改坏确认：故意打乱存档键序");
    }
    // 官方写出的键顺序（Gson 的 LinkedTreeMap）也照抄回来。
    // 官方的 Gson 按插入顺序写；BTreeMap 会重排，这里按官方对象原来的键序写回。
    let mut ours = serde_json::Map::new();
    let official_object = official_value.as_object().ok_or("官方存档不是对象")?;
    for (id, official_entry) in official_object {
        if id == "DataVersion" {
            continue;
        }
        let criteria = official_entry["criteria"]
            .as_object()
            .ok_or("官方 criteria 不是对象")?;
        let mut map = BTreeMap::new();
        for (name, value) in criteria {
            let date = parse_obtained_time(value.as_str().unwrap_or(""))
                .ok_or_else(|| format!("官方时间格式不合法：{value}"))?;
            map.insert(name.clone(), Some(date));
        }
        let mut ours_progress = AdvancementProgress {
            criteria: map,
            requirements: Vec::new(),
        };
        // done: false 时官方读档会补齐 requirements 名，这里只比较写回的 criteria。
        ours_progress.requirements = vec![ours_progress.criteria.keys().cloned().collect()];
        ours.insert(id.clone(), ours_progress.to_json());
    }
    ours.insert(
        "DataVersion".to_owned(),
        serde_json::json!(file.data_version),
    );
    report.check(
        serde_json::to_string(&Value::Object(ours)).map_err(|error| error.to_string())?
            == serde_json::to_string(&official_value).map_err(|error| error.to_string())?,
        "Bamboo 读回官方存档再写出的 JSON 语义和键顺序不同",
    );
    Ok(())
}
