//! 用官方的菜单代码检查 `mc-bamboo-game` 的容器点击逻辑。
//!
//! `MenuOracle.java` 在 JVM 里给每种容器菜单出题：随机的初始内容、随机的创造模式开关和随机的一串点击。
//! 讲台走 `LecternMenu` 的按钮，不走槽位点击。每一步之后它报告菜单的全部槽位、光标、`state_id`，
//! 讲台再加页码。箱子菜单用 [`Replay`] 重放，讲台用 [`mc_bamboo_game::LecternReplay`]。
//!
//! 随机题是“客户端从不预测”：点击自带的 `changed_slots` 是空的，光标永远是空物品。
//! 最后一道带组件的题改成官方 `MultiPlayerGameMode.handleContainerInput` 的预测，
//! 回传变过的槽和光标的哈希，再比服务端补发了哪些槽。
//!
//! Random cases never predict: `changed_slots` is empty and the carried hash is
//! empty. The last case predicts like `MultiPlayerGameMode.handleContainerInput`
//! and compares which slots the server resends.
//!
//! 题目不落盘，`--case` 可以单独重跑一道。

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_data::version;
use mc_bamboo_game::{LecternReplay, LecternSnapshot, Replay, ReplayShape, ReplayWorld, Snapshot};
use mc_bamboo_protocol::{
    components::{ComponentPatch, cursor::Cursor},
    item::ItemStack,
    packet::play::ContainerInput,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: menu <java> [--cases N] [--seed S] [--case I] [--show N]";
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header(Header),
    Case(Box<Case>),
    End(End),
}

#[derive(Debug, Deserialize)]
struct Header {
    version: String,
    protocol: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: u32,
    shape: String,
    creative: bool,
    /// 讲台才有。箱子题不写这两项。
    ///
    /// Present on lectern cases. Chest cases omit both.
    #[serde(default)]
    page_count: i32,
    #[serde(default)]
    may_build: bool,
    container: Vec<[i32; 2]>,
    inventory: Vec<[i32; 2]>,
    #[serde(default)]
    data: Vec<i32>,
    /// 附魔台和铁砧题里的 `enchantmentSeed`，也用来固定铁砧的实体随机源。
    #[serde(default)]
    seed: i32,
    /// 假世界里的地图比例、锁定状态和书架数。
    /// 固定题的假世界会执行 `access.execute`；随机题仍是 NULL access。
    #[serde(default)]
    fixed_world: bool,
    #[serde(default)]
    map_scale: i32,
    #[serde(default)]
    map_locked: bool,
    #[serde(default)]
    bookshelves: i32,
    /// 附魔成功后官方从玩家随机源换出的新种子。
    #[serde(default)]
    next_seed: Option<i32>,
    open: Outcome,
    steps: Vec<Step>,
    /// 这道题的客户端会按官方预测回传哈希。
    ///
    /// This case's client sends hashes the way the official client predicts.
    #[serde(default)]
    predict: bool,
    /// 打开前盖到容器格上的组件补丁，网络格式。
    ///
    /// Component patches, in the network format, applied before the menu opens.
    #[serde(default)]
    patches: Vec<StoredPatch>,
}

#[derive(Debug, Deserialize)]
struct StoredPatch {
    index: usize,
    bytes: String,
}

#[derive(Debug, Deserialize)]
struct Step {
    slot: i16,
    button: i8,
    input: u8,
    /// 切石机的 `container_button_click`。别的题不写。
    ///
    /// Stonecutter `container_button_click`. Other cases omit it.
    #[serde(default, rename = "menuButton")]
    menu_button: bool,
    /// 铁砧改名包。别的题不写。
    #[serde(default)]
    rename: Option<String>,
    /// 讲台的 `container_button_click`。箱子题不写，默认 0。
    ///
    /// Lectern `container_button_click`. Chest cases omit it and it defaults to 0.
    #[serde(rename = "buttonId", default)]
    button_id: i32,
    outcome: Outcome,
    /// 这次点击之后服务端补发的槽位。只有预测题才有。
    ///
    /// Slots the server resent after this click. Only predicting cases set it.
    #[serde(default)]
    resent: Vec<i16>,
    /// 光标有没有被补发。
    ///
    /// Whether the carried item was resent.
    #[serde(default)]
    carried_resent: bool,
}

/// 菜单在某一刻的样子。槽位和光标是 `[物品 ID, 数量, 组件补丁的十六进制]`，空槽是 `[0, 0, ""]`。
/// `data` 是进度条。箱子没有，缺省是空的。
#[derive(Debug, Deserialize)]
struct Outcome {
    state: i32,
    carried: (i32, i32, String),
    slots: Vec<(i32, i32, String)>,
    #[serde(default)]
    data: Vec<i32>,
    /// 讲台的当前页。箱子题不写。
    ///
    /// Lectern page. Chest cases omit it.
    #[serde(default)]
    page: i32,
}

#[derive(Debug, Deserialize)]
struct End {
    cases: u32,
}

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" | "--seed" | "--case" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE)?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Report {
    cases: u32,
    steps: usize,
    slots: usize,
    checks: usize,
    mismatches: Vec<String>,
    show: usize,
    ended: bool,
}

/// 启动官方进程，一边读一边比较。`command` 已经是能运行 `MenuOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        show: options.show.unwrap_or(DEFAULT_SHOW),
        ..Report::default()
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        // 这边不再读了，JVM 会一直等着管道。
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("MenuOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad menu oracle line: {error}"))?;
            match parsed {
                Line::Header(header) => self.check_header(&header)?,
                Line::Case(case) => self.replay(&case)?,
                Line::End(end) => {
                    self.ended = true;
                    if end.cases != self.cases {
                        return Err(format!(
                            "java wrote {} cases, {} arrived",
                            end.cases, self.cases
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_header(&mut self, header: &Header) -> Result<(), String> {
        if header.version != version::GAME_VERSION || header.protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {} (protocol {})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
                header.version,
                header.protocol,
            ));
        }
        Ok(())
    }

    /// 重放一道题：打开菜单，然后一步一步点。
    fn replay(&mut self, case: &Case) -> Result<(), String> {
        if case.shape == "lectern" {
            return self.replay_lectern(case);
        }
        let shape = shape_of(&case.shape)
            .ok_or_else(|| format!("case {}: unknown menu shape {:?}", case.id, case.shape))?;
        self.cases += 1;
        let mut replay = if shape.is_workstation() {
            let mut replay = Replay::workstation(
                shape,
                &pairs(&case.container),
                &pairs(&case.inventory),
                &case.data,
                case.creative,
                ReplayWorld {
                    seed: case.seed,
                    fixed: case.fixed_world,
                    map_scale: case.map_scale,
                    map_locked: case.map_locked,
                    bookshelves: case.bookshelves,
                    next_seed: case.next_seed,
                },
            );
            // 材料先按物品和数量放进去，再盖上组件。下标是格子序号，合成格要加一。
            // Ingredients land as item and count first, then the component patch. The index is the
            // grid slot; crafting and the inventory grid sit one slot later.
            for patch in &case.patches {
                let Some(&[item, count]) = case.container.get(patch.index) else {
                    return Err(format!(
                        "case {}: patch slot {} is outside the grid",
                        case.id, patch.index
                    ));
                };
                let slot = work_slot(shape, patch.index).ok_or_else(|| {
                    format!(
                        "case {}: shape {} has no grid slot {}",
                        case.id, case.shape, patch.index
                    )
                })?;
                replay.reseed(slot, stack_with_patch(item, count, &patch.bytes)?);
            }
            replay
        } else {
            let mut replay = Replay::new(shape, &pairs(&case.container), &pairs(&case.inventory));
            replay.set_creative(case.creative);
            if case.predict {
                for patch in &case.patches {
                    let Some(&[item, count]) = case.container.get(patch.index) else {
                        return Err(format!(
                            "case {}: patch slot {} is outside the container",
                            case.id, patch.index
                        ));
                    };
                    replay.set_container(patch.index, stack_with_patch(item, count, &patch.bytes)?);
                }
            }
            replay
        };
        let snapshot = replay.open();
        self.compare(&format!("case {} open", case.id), &snapshot, &case.open);

        for (index, step) in case.steps.iter().enumerate() {
            let input = ContainerInput::from_id(i32::from(step.input));
            let snapshot = if let Some(name) = &step.rename {
                replay.rename(name)
            } else if step.menu_button {
                replay.button(step.button)
            } else if case.predict {
                let (snapshot, resent, carried_resent) =
                    replay.click_predicting(step.slot, step.button, input);
                if resent != step.resent || carried_resent != step.carried_resent {
                    self.mismatch(format!(
                        "case {} step {index}: resent slots official {:?} carried {}, bamboo {resent:?} carried {carried_resent}",
                        case.id, step.resent, step.carried_resent
                    ));
                }
                snapshot
            } else {
                replay.click(step.slot, step.button, input)
            };
            self.steps += 1;
            self.compare(
                &format!("case {} step {index}", case.id),
                &snapshot,
                &step.outcome,
            );
        }
        Ok(())
    }

    /// `LecternMenu`：按钮改页码或取书，比对槽位、光标、`state_id` 和页码。
    ///
    /// Replays lectern buttons and compares slots, the cursor, `state_id`, and the page.
    fn replay_lectern(&mut self, case: &Case) -> Result<(), String> {
        self.cases += 1;
        let [item, count] = case.container.first().copied().unwrap_or([0, 0]);
        let (mut replay, snapshot) =
            LecternReplay::open(item, count, case.page_count, case.may_build);
        self.compare_lectern(&format!("case {} open", case.id), &snapshot, &case.open);
        for (index, step) in case.steps.iter().enumerate() {
            let snapshot = replay.press(step.button_id);
            self.steps += 1;
            self.compare_lectern(
                &format!("case {} step {index}", case.id),
                &snapshot,
                &step.outcome,
            );
        }
        Ok(())
    }

    fn compare(&mut self, place: &str, ours: &Snapshot, official: &Outcome) {
        self.checks += 1;
        if ours.state_id != official.state {
            self.mismatch(format!(
                "{place}: state id: official {}, bamboo {}",
                official.state, ours.state_id
            ));
        }
        if triple(&ours.carried) != official.carried {
            self.mismatch(format!(
                "{place}: carried: official {:?}, bamboo {}",
                official.carried,
                describe(&ours.carried)
            ));
        }
        if ours.data != official.data {
            self.mismatch(format!(
                "{place}: data: official {:?}, bamboo {:?}",
                official.data, ours.data
            ));
        }
        if ours.slots.len() != official.slots.len() {
            self.mismatch(format!(
                "{place}: menu has {} slots, official {}",
                ours.slots.len(),
                official.slots.len()
            ));
            return;
        }
        self.slots += ours.slots.len();
        for (index, (ours, official)) in ours.slots.iter().zip(&official.slots).enumerate() {
            if triple(ours) != *official {
                self.mismatch(format!(
                    "{place}: slot {index}: official {:?}, bamboo {}",
                    official,
                    describe(ours)
                ));
            }
        }
    }

    fn compare_lectern(&mut self, place: &str, ours: &LecternSnapshot, official: &Outcome) {
        self.checks += 1;
        if ours.state_id != official.state {
            self.mismatch(format!(
                "{place}: state id: official {}, bamboo {}",
                official.state, ours.state_id
            ));
        }
        if ours.page != official.page {
            self.mismatch(format!(
                "{place}: page: official {}, bamboo {}",
                official.page, ours.page
            ));
        }
        // 讲台重放只放物品和数量。页文本在 `page` 里比，不比组件补丁。
        // The lectern replay stores the item and count. Page text is the `page` field, not the patch.
        if id_count(&triple(&ours.carried)) != id_count(&official.carried) {
            self.mismatch(format!(
                "{place}: carried: official {:?}, bamboo {}",
                official.carried,
                describe(&ours.carried)
            ));
        }
        if ours.slots.len() != official.slots.len() {
            self.mismatch(format!(
                "{place}: menu has {} slots, official {}",
                ours.slots.len(),
                official.slots.len()
            ));
            return;
        }
        self.slots += ours.slots.len();
        for (index, (ours, official_slot)) in ours.slots.iter().zip(&official.slots).enumerate() {
            if id_count(&triple(ours)) != id_count(official_slot) {
                self.mismatch(format!(
                    "{place}: slot {index}: official {:?}, bamboo {}",
                    official_slot,
                    describe(ours)
                ));
            }
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show {
            println!("MISMATCH {message}");
        }
        self.mismatches.push(message);
    }

    fn finish(&self) -> Result<(), String> {
        if !self.ended {
            return Err("MenuOracle.java stopped before its end line".to_owned());
        }
        println!(
            "menu: {} cases, {} clicks; compared {} states ({} slots)",
            self.cases, self.steps, self.checks, self.slots
        );
        println!("MENU RESULT: {} mismatches", self.mismatches.len());
        if self.mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!("{} menu mismatches", self.mismatches.len()))
        }
    }
}

fn shape_of(name: &str) -> Option<ReplayShape> {
    match name {
        "chest3" => Some(ReplayShape::Chest(3)),
        "chest6" => Some(ReplayShape::Chest(6)),
        "hopper" => Some(ReplayShape::Hopper),
        "dispenser" => Some(ReplayShape::Dispenser),
        "shulker" => Some(ReplayShape::ShulkerBox),
        "inventory" => Some(ReplayShape::Inventory),
        "crafting" => Some(ReplayShape::Crafting),
        "crafter" => Some(ReplayShape::Crafter),
        "stonecutter" => Some(ReplayShape::Stonecutter),
        "smithing" => Some(ReplayShape::Smithing),
        "furnace" => Some(ReplayShape::Furnace),
        "blast_furnace" => Some(ReplayShape::BlastFurnace),
        "smoker" => Some(ReplayShape::Smoker),
        "brewing" => Some(ReplayShape::Brewing),
        "loom" => Some(ReplayShape::Loom),
        "grindstone" => Some(ReplayShape::Grindstone),
        "anvil" => Some(ReplayShape::Anvil),
        "cartography" => Some(ReplayShape::Cartography),
        "enchantment" => Some(ReplayShape::Enchantment),
        _ => None,
    }
}

fn stack_with_patch(item: i32, count: i32, hex: &str) -> Result<ItemStack, String> {
    let bytes = unhex(hex)?;
    let mut cursor = Cursor::new(&bytes);
    let patch = ComponentPatch::read(&mut cursor).map_err(|error| error.to_string())?;
    cursor.finish().map_err(|error| error.to_string())?;
    let (added, removed) = patch.to_opaque();
    Ok(ItemStack {
        item,
        count,
        added,
        removed,
    })
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

/// 合成格和工作台材料放进菜单时的槽位。和 `ReplayShape::seed_slot` 一样。
///
/// Where a grid index lands in the menu. Matches `ReplayShape::seed_slot`.
fn work_slot(shape: ReplayShape, index: usize) -> Option<usize> {
    match shape {
        ReplayShape::Inventory | ReplayShape::Crafting => Some(index + 1),
        ReplayShape::Stonecutter
        | ReplayShape::Smithing
        | ReplayShape::Furnace
        | ReplayShape::BlastFurnace
        | ReplayShape::Smoker
        | ReplayShape::Brewing
        | ReplayShape::Loom
        | ReplayShape::Grindstone
        | ReplayShape::Anvil
        | ReplayShape::Cartography
        | ReplayShape::Enchantment
        | ReplayShape::Crafter => Some(index),
        _ => None,
    }
}

fn pairs(entries: &[[i32; 2]]) -> Vec<(i32, i32)> {
    entries.iter().map(|&[item, count]| (item, count)).collect()
}

/// 出错时打印成 `(物品, 数量, 补丁)`，空槽写成 `empty`。
fn id_count(triple: &(i32, i32, String)) -> [i32; 2] {
    [triple.0, triple.1]
}

fn describe(stack: &ItemStack) -> String {
    let (item, count, patch) = triple(stack);
    if item == 0 {
        "empty".to_owned()
    } else if patch.is_empty() {
        format!("({item}, {count})")
    } else {
        format!("({item}, {count}, {patch})")
    }
}

/// 官方写出的 `[物品, 数量, 补丁十六进制]`。空物品没有补丁。
fn triple(stack: &ItemStack) -> (i32, i32, String) {
    if stack.is_empty() {
        return (0, 0, String::new());
    }
    let mut bytes = Vec::new();
    stack.patch().write(&mut bytes);
    let patch = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    (stack.item, stack.count, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_forward_the_case_selection() {
        let options =
            Options::parse(&["--case", "7", "--seed", "3", "--show", "2"]).expect("valid options");
        assert_eq!(options.forwarded, ["--case", "7", "--seed", "3"]);
        assert_eq!(options.show, Some(2));
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    #[test]
    fn every_shape_has_a_name() {
        for (name, shape) in [
            ("chest3", ReplayShape::Chest(3)),
            ("chest6", ReplayShape::Chest(6)),
            ("hopper", ReplayShape::Hopper),
            ("dispenser", ReplayShape::Dispenser),
            ("shulker", ReplayShape::ShulkerBox),
            ("inventory", ReplayShape::Inventory),
            ("crafting", ReplayShape::Crafting),
            ("crafter", ReplayShape::Crafter),
            ("stonecutter", ReplayShape::Stonecutter),
            ("smithing", ReplayShape::Smithing),
            ("furnace", ReplayShape::Furnace),
            ("blast_furnace", ReplayShape::BlastFurnace),
            ("smoker", ReplayShape::Smoker),
            ("brewing", ReplayShape::Brewing),
            ("loom", ReplayShape::Loom),
            ("grindstone", ReplayShape::Grindstone),
            ("anvil", ReplayShape::Anvil),
            ("cartography", ReplayShape::Cartography),
            ("enchantment", ReplayShape::Enchantment),
        ] {
            assert_eq!(shape_of(name), Some(shape));
        }
        assert_eq!(shape_of("unknown_menu"), None);
    }
}
