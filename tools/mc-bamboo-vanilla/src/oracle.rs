//! 用官方代码检查 `mc-bamboo-blocks`。
//!
//! `BlockOracle.java` 在 JVM 里运行官方的 `useItemOn`、`destroyBlock`，有的题之后再走几个游戏刻，
//! 一题一行写到 stdout。这里边读边在同样的场景里重放，比较 `InteractionResult`、世界的变化、
//! 破坏事件和范围内还排着的计划刻。
//! 题目不落盘，20 万题的输出有一百多 MB。
//!
//! 一题的结论是下面四种之一：
//!
//! - `pass`：和官方完全一致。
//! - `fail`：不一致，碰过的方块类都移植了，这是真的 bug。`panic` 永远算这一种。
//! - `blocked`：不一致，但是碰过还没移植的类，现在还判断不了。题目的类自己没移植时这也是待办。
//! - `excluded`：不比较，原因在 [`Rule`] 的表里，运行结束时打印。
//!
//! `fail`，或者题目的类还没移植却和官方不一致，退出码都不是 0。现在全部类都移植了，`blocked` 不会出现，
//! 升级版本多出新类时它才回来。
//!
//! 那时想单独检查一个类可以加 `--pure`：场景里只放已经移植的方块，结果里就没有 `blocked` 了。

mod lines;
mod replay;
mod report;

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{BufRead, BufReader, Read},
    panic::{self, AssertUnwindSafe},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
};

use mc_bamboo_blocks::{behaviour::is_ported, world::MAX_CHAINED_NEIGHBOR_UPDATES};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    Block, BlockAncestor, BlockClass, Item, block_id_of_state, blocks, items, version,
};

use lines::{Case, Header, Kind, Line, Tick};
use replay::{Replayed, Scheduled, replay};
use report::{Mismatch, Report};

/// 同时留着的官方日志行数，只在 JVM 失败时打印。
const LOG_TAIL: usize = 60;
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 3;

const USAGE: &str = "usage: oracle <java> [--cases N] [--seed S] [--only <block or class>] [--pure] [--case I] [--show N] [--show-blocked]";

/// 不比较的内容。每一条都有原因，第 4 章要求排除的内容进最后的报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    AnalogInput,
    RandomOutcome,
    RandomLoop,
    RandomPositional,
    Explosion,
    Worldgen,
    ImpossibleScene,
    ItemUseOn,
    CanDestroyBlock,
    FluidTick,
    RandomDelay,
    FallingTick,
    FireTick,
    CommandTick,
    TimerTick,
    EntityTick,
    Piston,
    EntityTrigger,
    Dispensing,
    DaylightTimer,
}

impl Rule {
    pub const ALL: [Self; 20] = [
        Self::AnalogInput,
        Self::RandomOutcome,
        Self::RandomLoop,
        Self::RandomPositional,
        Self::Explosion,
        Self::Worldgen,
        Self::ImpossibleScene,
        Self::ItemUseOn,
        Self::CanDestroyBlock,
        Self::FluidTick,
        Self::RandomDelay,
        Self::FallingTick,
        Self::FireTick,
        Self::CommandTick,
        Self::TimerTick,
        Self::EntityTick,
        Self::Piston,
        Self::EntityTrigger,
        Self::Dispensing,
        Self::DaylightTimer,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::AnalogInput => "analog input",
            Self::RandomOutcome => "random outcome",
            Self::RandomLoop => "random loop",
            Self::RandomPositional => "positional random",
            Self::Explosion => "explosion",
            Self::Worldgen => "worldgen",
            Self::ImpossibleScene => "impossible scene",
            Self::ItemUseOn => "item useOn",
            Self::CanDestroyBlock => "canDestroyBlock",
            Self::FluidTick => "fluid tick",
            Self::RandomDelay => "random delay",
            Self::FallingTick => "falling tick",
            Self::FireTick => "fire tick",
            Self::CommandTick => "command tick",
            Self::TimerTick => "timer tick",
            Self::EntityTick => "entity tick",
            Self::Piston => "piston",
            Self::EntityTrigger => "entity trigger",
            Self::Dispensing => "dispensing",
            Self::DaylightTimer => "daylight timer",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::AnalogInput => {
                "case: a container changed its contents and a comparator read them, \
                 Bamboo keeps no container contents (chiseled bookshelf excepted)"
            }
            Self::RandomOutcome => {
                "case: the official outcome changes with the random source (chapter 1)"
            }
            Self::RandomLoop => "case: extreme random values made the official code spin",
            Self::RandomPositional => {
                "case: the official code asked for a position derived random source"
            }
            Self::Explosion => {
                "case: explosions are out of scope (respawn anchor in the overworld)"
            }
            Self::Worldgen => "case: world generation is out of scope (bone meal growing a tree)",
            Self::ImpossibleScene => {
                "case: a half of a broken structure made the official code throw"
            }
            Self::ItemUseOn => "case: both sides left the click to a non block item's own useOn",
            Self::CanDestroyBlock => {
                "case: Item.canDestroyBlock refused, the caller checks it before destroy_block"
            }
            Self::FluidTick => "tick entry: fluids do not flow, only recorded (chapter 1)",
            Self::RandomDelay => {
                "tick delay: the official delay is random, position and type are still compared"
            }
            Self::FallingTick => "tick: blocks do not fall (chapter 1)",
            Self::FireTick => "tick: fire does not spread (chapter 1)",
            Self::CommandTick => "tick: command blocks run commands, out of scope",
            Self::TimerTick => "tick: growth and hatching timers are out of scope (chapter 1)",
            Self::EntityTick => "tick: only entities trigger or read this tick, none are simulated",
            Self::Piston => {
                "scope: block events are dropped, pistons never move, pushing is the next step"
            }
            Self::EntityTrigger => {
                "scope: plates, detector rails, tripwires, targets, sculk sensors, lightning rods \
                 and primed TNT need entities, projectiles, vibrations or lightning"
            }
            Self::Dispensing => {
                "scope: dispensers, droppers and crafters are empty, they never dispense or craft"
            }
            Self::DaylightTimer => {
                "scope: daylight detectors follow the sun in a block entity ticker that does not run"
            }
        }
    }

    /// `skip` 行的原因前缀对应的规则。`exception:` 不在这里：那是假世界的 bug，要算失败。
    fn of_skip(reason: &str) -> Option<Self> {
        let (prefix, _) = reason.split_once(':')?;
        match prefix {
            "random" => Some(Self::RandomOutcome),
            "random-loop" => Some(Self::RandomLoop),
            "random-positional" => Some(Self::RandomPositional),
            "explosion" => Some(Self::Explosion),
            "worldgen" => Some(Self::Worldgen),
            "impossible" => Some(Self::ImpossibleScene),
            _ => None,
        }
    }
}

/// 第 1 章第 5 条以外的计划刻：这些方块排的刻不比较，它们的 `tick` 题也不比较，
/// `settle` 里跑过它们的题整道排除。以后做了其中某一种时，把它从表里删掉，比较就打开了。
const OUT_OF_SCOPE_TICKS: &[(BlockAncestor, Rule)] = &[
    (BlockAncestor::FallingBlock, Rule::FallingTick),
    (BlockAncestor::BrushableBlock, Rule::FallingTick),
    (BlockAncestor::FireBlock, Rule::FireTick),
    (BlockAncestor::LiquidBlock, Rule::FluidTick),
    (BlockAncestor::BubbleColumnBlock, Rule::FluidTick),
    (BlockAncestor::CommandBlock, Rule::CommandTick),
    (BlockAncestor::FrogspawnBlock, Rule::TimerTick),
    (BlockAncestor::SnifferEggBlock, Rule::TimerTick),
    (BlockAncestor::DriedGhastBlock, Rule::TimerTick),
    (BlockAncestor::FrostedIceBlock, Rule::TimerTick),
    (BlockAncestor::EyeblossomBlock, Rule::TimerTick),
    (BlockAncestor::CreakingHeartBlock, Rule::TimerTick),
    (BlockAncestor::BasePressurePlateBlock, Rule::EntityTick),
    (BlockAncestor::DetectorRailBlock, Rule::EntityTick),
    (BlockAncestor::TripWireBlock, Rule::EntityTick),
];

/// 这条规则排除了哪些类的计划刻，打印排除表时用。
fn tick_classes(rule: Rule) -> Vec<BlockAncestor> {
    OUT_OF_SCOPE_TICKS
        .iter()
        .filter(|(_, entry)| *entry == rule)
        .map(|&(ancestor, _)| ancestor)
        .collect()
}

/// 这个方块的计划刻不在比较的范围里时，返回原因。
fn tick_exclusion(block: &Block) -> Option<Rule> {
    let ancestors = block.class.ancestors();
    OUT_OF_SCOPE_TICKS
        .iter()
        .find(|(ancestor, _)| ancestors.contains(ancestor))
        .map(|&(_, rule)| rule)
}

/// 命令行参数。`--cases`、`--seed`、`--only`、`--case` 原样交给 `BlockOracle.java`。
#[derive(Debug, PartialEq, Eq)]
struct Options {
    forwarded: Vec<String>,
    pure: bool,
    show: usize,
    show_blocked: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            forwarded: Vec::new(),
            pure: false,
            show: DEFAULT_SHOW,
            show_blocked: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" | "--seed" | "--only" | "--case" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = value.parse().map_err(|_| USAGE)?;
                }
                "--pure" => options.pure = true,
                "--show-blocked" => options.show_blocked = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }

    /// 交给 `BlockOracle.java` 的参数。
    fn java_args(&self) -> Vec<String> {
        let mut args = self.forwarded.clone();
        if self.pure {
            args.extend(["--avoid".to_owned(), unported_classes().join(",")]);
        }
        args
    }

    /// 重现一道题要带的参数：除了题数和题号，别的都要和原来相同。
    fn rerun_args(&self) -> String {
        let mut args = Vec::new();
        for pair in self.forwarded.chunks(2) {
            if pair[0] == "--only" {
                args.extend_from_slice(pair);
            }
        }
        if self.pure {
            args.push("--pure".to_owned());
        }
        args.join(" ")
    }
}

/// 还没移植的类的名字，官方类名就是 `BlockClass` 的变体名。
fn unported_classes() -> Vec<String> {
    let mut names: Vec<String> = blocks::BLOCKS
        .iter()
        .filter(|block| !is_ported(block.class))
        .map(|block| format!("{:?}", block.class))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// 名字到方块和物品的表。这些名字来自另一个进程，生成文件过期时查不到。
pub struct Names {
    blocks: HashMap<&'static str, &'static Block>,
    items: HashMap<&'static str, (i32, &'static Item)>,
}

impl Names {
    fn new() -> Self {
        Self {
            blocks: blocks::BLOCKS
                .iter()
                .map(|block| (block.name, block))
                .collect(),
            items: items::ITEMS
                .iter()
                .zip(0..)
                .map(|(item, id)| (item.name, (id, item)))
                .collect(),
        }
    }

    fn block(&self, name: &str) -> Result<&'static Block, String> {
        self.blocks
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown block {name}, are the generated files stale?"))
    }

    /// 物品的注册表 ID 和物品。
    pub fn item(&self, name: &str) -> Result<(i32, &'static Item), String> {
        self.items
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown item {name}, are the generated files stale?"))
    }
}

thread_local! {
    /// 正在重放一道题。这时的 panic 算这道题失败，别的 panic 是这个工具自己的 bug。
    static REPLAYING: Cell<bool> = const { Cell::new(false) };
    /// 重放时最近一次 panic 的内容，`catch_unwind` 拿到的负载没有位置。
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 重放时的 panic 记下来放进报告，不打印。
fn capture_replay_panics() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if REPLAYING.get() {
            LAST_PANIC.set(info.to_string());
        } else {
            default_hook(info);
        }
    }));
}

/// 启动官方进程，一边读题一边比较。`command` 已经是能运行 `BlockOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    if options.pure {
        println!("--pure: scenes only hold ported classes and water");
    }
    let mut child = command
        .args(options.java_args())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut session = Session::new(options);
    capture_replay_panics();
    let read = session.read_lines(BufReader::new(stdout));
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
        return Err(format!("BlockOracle.java failed: {status}\n{log}"));
    }
    session.finish()
}

/// 官方日志在另一个线程里读，不然管道写满时 JVM 会停住。只留最后几行，JVM 失败时才看。
fn collect_log(child: &mut Child) -> Result<mpsc::Receiver<Vec<String>>, String> {
    let stderr = child.stderr.take().ok_or("java has no stderr")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut tail = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == LOG_TAIL {
                tail.remove(0);
            }
            tail.push(line);
        }
        let _ = sender.send(tail);
    });
    Ok(receiver)
}

/// 一次运行的状态。
struct Session {
    options: Options,
    names: Names,
    header: Option<Header>,
    report: Report,
    ended: bool,
}

impl Session {
    fn new(options: Options) -> Self {
        Self {
            report: Report::new(options.show, options.show_blocked),
            options,
            names: Names::new(),
            header: None,
            ended: false,
        }
    }

    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad oracle line: {error}\n{line}"))?;
            self.accept(parsed)?;
        }
        Ok(())
    }

    /// 打印结果，有失败时返回错误。
    fn finish(&self) -> Result<(), String> {
        if !self.ended {
            return Err("BlockOracle.java stopped before its end line".to_owned());
        }
        self.report.print(&self.options.rerun_args());
        self.report.verdict()
    }

    fn accept(&mut self, line: Line) -> Result<(), String> {
        match line {
            Line::Header(header) => self.accept_header(header),
            Line::Case(case) => self.accept_case(&case),
            Line::Skip(skip) => {
                let block = self.names.block(&skip.block)?;
                match Rule::of_skip(&skip.reason) {
                    Some(rule) => self.report.skipped(block, skip.kind, rule),
                    None => self.report.broken_oracle(block, skip.kind, &skip.reason),
                }
                Ok(())
            }
            Line::End(end) => {
                self.ended = true;
                self.report.check_totals(end.cases, end.skipped)
            }
        }
    }

    fn accept_header(&mut self, header: Header) -> Result<(), String> {
        if header.version != version::GAME_VERSION || header.protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {} (protocol {})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
                header.version,
                header.protocol
            ));
        }
        if header.max_chained_updates != MAX_CHAINED_NEIGHBOR_UPDATES {
            return Err(format!(
                "the fake world allows {} chained updates, mc-bamboo-blocks {MAX_CHAINED_NEIGHBOR_UPDATES}",
                header.max_chained_updates
            ));
        }
        self.report.start(&header);
        self.header = Some(header);
        Ok(())
    }

    fn accept_case(&mut self, case: &Case) -> Result<(), String> {
        let block = self.names.block(&case.block)?;
        let header = self.header.as_ref().ok_or("case before the header")?;
        let kind = case.action.kind;

        if let Some(rule) = case_exclusion(case, block, &self.names)? {
            self.report.excluded(block, kind, rule);
            return Ok(());
        }
        let replayed = match run_guarded(case, header, &self.names)? {
            Ok(replayed) => replayed,
            Err(message) => {
                let mismatch = Mismatch {
                    panic: Some(message),
                    ..Mismatch::default()
                };
                self.report.mismatched(block, case, None, &mismatch, &[]);
                return Ok(());
            }
        };
        if left_to_the_item(case, &replayed) {
            self.report.excluded(block, kind, Rule::ItemUseOn);
            return Ok(());
        }
        let mismatch = compare(case, &replayed, &self.names, &mut self.report)?;
        if mismatch.is_empty() {
            self.report.passed(block, case);
        } else {
            let blockers = blockers(case, &replayed, &self.names)?;
            self.report
                .mismatched(block, case, Some(&replayed), &mismatch, &blockers);
        }
        Ok(())
    }
}

/// 官方的方块没有吃掉这次点击，结果来自非方块物品自己的 `useOn`，那不在这个 crate 里。
/// 我们这边也要什么都没做才算：方块多吃了点击，或者动了世界，要照常比较，这是真的不一致。
fn left_to_the_item(case: &Case, replayed: &Replayed) -> bool {
    case.action.item_use_on
        && case.outcome.item_reached
        && replayed.result == "pass"
        && replayed.changes.is_empty()
        && replayed.destroyed.is_empty()
        && replayed.ticks.is_empty()
}

/// 不用重放就知道不比较的题。
fn case_exclusion(case: &Case, block: &Block, names: &Names) -> Result<Option<Rule>, String> {
    if case.outcome.analog_content {
        return Ok(Some(Rule::AnalogInput));
    }
    let rule = match case.action.kind {
        Kind::Destroy
            if case.outcome.result == "refused"
                && !block
                    .class
                    .ancestors()
                    .contains(&BlockAncestor::GameMasterBlock) =>
        {
            Some(Rule::CanDestroyBlock)
        }
        Kind::Tick => tick_exclusion(block),
        _ => None,
    };
    match rule {
        Some(rule) => Ok(Some(rule)),
        None => settled_out_of_scope(case, names),
    }
}

/// `settle` 里跑过不在范围里的计划刻：官方的世界已经被它改过，这道题比不了。
fn settled_out_of_scope(case: &Case, names: &Names) -> Result<Option<Rule>, String> {
    for name in &case.outcome.ran {
        if let Some(rule) = tick_exclusion(names.block(name)?) {
            return Ok(Some(rule));
        }
    }
    Ok(None)
}

/// 外层的错误是题目本身有问题，内层的错误是 `mc-bamboo-blocks` 的 panic。
fn run_guarded(
    case: &Case,
    header: &Header,
    names: &Names,
) -> Result<Result<Replayed, String>, String> {
    REPLAYING.set(true);
    let replayed = panic::catch_unwind(AssertUnwindSafe(|| replay(case, header, names)));
    REPLAYING.set(false);
    match replayed {
        Ok(result) => result.map(Ok),
        Err(_) => Ok(Err(LAST_PANIC.take())),
    }
}

/// 比较一道题，`Mismatch::is_empty` 表示一致。
fn compare(
    case: &Case,
    replayed: &Replayed,
    names: &Names,
    report: &mut Report,
) -> Result<Mismatch, String> {
    let answer = &case.outcome;
    let official = comparable_ticks(&answer.ticks, names, report)?;
    let ours = in_scope_scheduled(&replayed.ticks, names)?;
    Ok(Mismatch {
        result: answer.result != replayed.result,
        cells: differing_cells(case, replayed),
        destroyed: answer.destroyed != replayed.destroyed,
        ticks: !same_ticks(&official, &ours),
        compared_ticks: (official, ours),
        panic: None,
    })
}

/// 两边结束时状态不同的格子。
fn differing_cells(case: &Case, replayed: &Replayed) -> BTreeSet<BlockPos> {
    let official: BTreeMap<BlockPos, u32> = case
        .outcome
        .changes
        .iter()
        .map(|cell| (cell.pos(), cell.state()))
        .collect();
    let same = |pos: &BlockPos| official.get(pos) == replayed.changes.get(pos);
    official
        .keys()
        .chain(replayed.changes.keys())
        .filter(|pos| !same(pos))
        .copied()
        .collect()
}

/// 官方的计划刻里要比较的部分，按执行顺序排好。不比较的条目计入排除表。
fn comparable_ticks(
    ticks: &[Tick],
    names: &Names,
    report: &mut Report,
) -> Result<Vec<Scheduled>, String> {
    let mut comparable = Vec::with_capacity(ticks.len());
    for tick in ticks {
        if tick.is_fluid() {
            report.excluded_entry(Rule::FluidTick);
            continue;
        }
        let block = names.block(tick.id())?;
        if let Some(rule) = tick_exclusion(block) {
            report.excluded_entry(rule);
            continue;
        }
        if tick.delay() == Tick::RANDOM_DELAY {
            report.excluded_entry(Rule::RandomDelay);
        }
        comparable.push(Scheduled {
            pos: tick.pos(),
            block: block.name,
            delay: tick.delay(),
            priority: tick.priority(),
        });
    }
    comparable.sort_by_key(|tick| (tick.delay, tick.priority));
    Ok(comparable)
}

fn in_scope_scheduled(ticks: &[Scheduled], names: &Names) -> Result<Vec<Scheduled>, String> {
    let mut comparable = Vec::with_capacity(ticks.len());
    for tick in ticks {
        if tick_exclusion(names.block(tick.block)?).is_none() {
            comparable.push(tick.clone());
        }
    }
    Ok(comparable)
}

/// 官方的列表按执行顺序写，所以没有随机延迟时逐项比较。
/// 有随机延迟时执行顺序定不下来，只比较集合。
fn same_ticks(official: &[Scheduled], ours: &[Scheduled]) -> bool {
    if official.len() != ours.len() {
        return false;
    }
    if official.iter().all(|tick| tick.delay != Tick::RANDOM_DELAY) {
        return official == ours;
    }
    let mut remaining: Vec<&Scheduled> = ours.iter().collect();
    official.iter().all(|tick| {
        let found = remaining.iter().position(|candidate| {
            candidate.pos == tick.pos
                && candidate.block == tick.block
                && candidate.priority == tick.priority
                && (tick.delay == Tick::RANDOM_DELAY || candidate.delay == tick.delay)
        });
        found.map(|index| remaining.swap_remove(index)).is_some()
    })
}

/// 这道题碰过的、还没移植的类。手上方块物品放的方块不一定进了世界，也算碰过。
///
/// 水不算：`LiquidBlock` 只有岩浆才写世界，水的逻辑全是流体的计划刻，而这些不比较。
fn blockers(case: &Case, replayed: &Replayed, names: &Names) -> Result<Vec<BlockClass>, String> {
    let water = block_id_of_state(blocks::WATER);
    let (_, held) = names.item(case.action.used_item())?;
    let held_blocks = [held.block, held.wall_block].into_iter().flatten();
    let mut classes: Vec<BlockClass> = replayed
        .touched
        .iter()
        .filter(|&&id| Some(id) != water)
        .map(|&id| blocks::BLOCKS[id].class)
        .chain(held_blocks.map(|id| blocks::BLOCKS[id as usize].class))
        .filter(|&class| !is_ported(class))
        .collect();
    classes.sort_by_key(|class| format!("{class:?}"));
    classes.dedup();
    Ok(classes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduled(x: i32, delay: i32) -> Scheduled {
        Scheduled {
            pos: BlockPos::new(x, 64, 0),
            block: "minecraft:sugar_cane",
            delay,
            priority: 0,
        }
    }

    const HEADER: &str = r#"{"type":"header","version":"26.3","protocol":777,"seed":1,"cases":1,
        "maxChainedUpdates":1000000,"gameTime":100000,"minY":-64,"maxY":319}"#;
    const END: &str = r#"{"type":"end","cases":1,"skipped":0}"#;

    /// 在 (0, 64, 0) 的石头顶面放一块石头，`scene` 和 `outcome` 由测试给。
    fn place_stone(scene: &str, outcome: &str) -> String {
        let action =
            r#"{"kind":"place","main":"minecraft:stone","off":"minecraft:air","hand":"main",
            "itemClass":"BlockItem","itemUseOn":false,"pos":[0,64,0],"face":"up",
            "loc":["3fe0000000000000","4050400000000000","3fe0000000000000"],"inside":false,
            "actor":["4014000000000000","4050400000000000","4014000000000000"],
            "yaw":"00000000","pitch":"00000000","sneak":false,
            "box":["4012666666666666","4050400000000000","4012666666666666",
                   "4015999999999999","4050c00000000000","4015999999999999"],"others":[],"settle":0}"#
                .replace('\n', " ");
        format!(
            r#"{{"type":"case","id":0,"block":"minecraft:stone","class":"Block","scene":{scene},"action":{action},"outcome":{outcome}}}"#
        )
    }

    fn answer(result: &str, changes: &str) -> String {
        answer_after_ticks(result, changes, "")
    }

    /// `ran` 是 `settle` 里跑过计划刻的方块，用逗号隔开的带引号的 ID。
    fn answer_after_ticks(result: &str, changes: &str, ran: &str) -> String {
        format!(
            r#"{{"result":"{result}","changes":{changes},"destroyed":[],"ticks":[],"ran":[{ran}],"analogContent":false,"itemReached":true,"spawned":[],"rng":false}}"#
        )
    }

    fn verdict_of(case: &str) -> Result<(), String> {
        let header = HEADER.replace('\n', " ");
        let text = format!("{header}\n{case}\n{END}\n");
        let mut session = Session::new(Options::parse(&[]).expect("合法"));
        session
            .read_lines(BufReader::new(text.as_bytes()))
            .expect("这些行符合格式");
        session.report.verdict()
    }

    #[test]
    fn the_official_answer_passes() {
        let stone = blocks::STONE;
        let case = place_stone(
            &format!("[[0,64,0,{stone}]]"),
            &answer("success", &format!("[[0,65,0,{stone}]]")),
        );
        assert_eq!(verdict_of(&case), Ok(()));
    }

    #[test]
    fn a_different_result_or_world_fails() {
        let stone = blocks::STONE;
        let scene = format!("[[0,64,0,{stone}]]");
        let wrong_result = place_stone(&scene, &answer("fail", &format!("[[0,65,0,{stone}]]")));
        assert!(verdict_of(&wrong_result).is_err());
        let wrong_world = place_stone(&scene, &answer("success", "[]"));
        assert!(verdict_of(&wrong_world).is_err());
    }

    #[test]
    fn a_case_that_settled_an_out_of_scope_tick_is_not_compared() {
        let stone = blocks::STONE;
        let scene = format!("[[0,64,0,{stone}]]");
        // 官方的沙子落下去了，我们这边不会落，这一题只能排除。
        let fell = answer_after_ticks("success", "[]", r#""minecraft:sand""#);
        assert_eq!(verdict_of(&place_stone(&scene, &fell)), Ok(()));
        let lamp = answer_after_ticks("success", "[]", r#""minecraft:redstone_lamp""#);
        assert!(verdict_of(&place_stone(&scene, &lamp)).is_err());
    }

    #[test]
    fn a_case_that_read_unsupported_container_contents_is_not_compared() {
        let stone = blocks::STONE;
        let scene = format!("[[0,64,0,{stone}]]");
        // 官方把物品塞进了罐子，比较器读到了 1；Bamboo 一律读 0。
        let read =
            answer("fail", "[]").replace("\"analogContent\":false", "\"analogContent\":true");
        assert_eq!(verdict_of(&place_stone(&scene, &read)), Ok(()));
        // 同一道题不标记时照常比较，结果是错的就失败。
        assert!(verdict_of(&place_stone(&scene, &answer("fail", "[]"))).is_err());
    }

    #[test]
    fn a_panic_fails_the_case_and_the_run_goes_on() {
        // 不存在的状态 ID：读到这一格时 mc-bamboo-blocks 会 panic。
        let case = place_stone("[[0,64,0,4000000]]", &answer("success", "[]"));
        let error = verdict_of(&case).expect_err("panic 算失败");
        assert!(error.contains("1 failures"), "{error}");
    }

    #[test]
    fn a_stream_without_its_end_line_is_an_error() {
        let mut session = Session::new(Options::parse(&[]).expect("合法"));
        let header = HEADER.replace('\n', " ");
        session
            .read_lines(BufReader::new(header.as_bytes()))
            .expect("这一行符合格式");
        assert!(session.finish().is_err());
    }

    #[test]
    fn a_bad_line_stops_the_reading() {
        let mut session = Session::new(Options::parse(&[]).expect("合法"));
        assert!(
            session
                .read_lines(BufReader::new("not json\n".as_bytes()))
                .is_err()
        );
    }

    #[test]
    fn options_split_into_forwarded_and_local() {
        let options =
            Options::parse(&["--seed", "2", "--show", "5", "--only", "SlabBlock"]).expect("合法");
        assert_eq!(options.forwarded, ["--seed", "2", "--only", "SlabBlock"]);
        assert_eq!(options.show, 5);
        assert!(!options.show_blocked);
        assert_eq!(options.java_args(), options.forwarded);
        assert_eq!(options.rerun_args(), "--only SlabBlock");
        assert!(Options::parse(&["--seed"]).is_err());
        assert!(Options::parse(&["--show", "many"]).is_err());
        assert!(Options::parse(&["--nonsense"]).is_err());
    }

    #[test]
    fn pure_runs_avoid_what_is_not_ported() {
        let options = Options::parse(&["--pure", "--cases", "10"]).expect("合法");
        let args = options.java_args();
        assert_eq!(args[2], "--avoid");
        assert!(!args[3].split(',').any(|class| class == "SlabBlock"));
        assert_eq!(options.rerun_args(), "--pure");
    }

    #[test]
    fn every_rule_says_why() {
        for rule in Rule::ALL {
            assert!(!rule.name().is_empty());
            assert!(
                rule.reason().contains(':'),
                "{} 的原因要说明范围",
                rule.name()
            );
        }
    }

    #[test]
    fn skip_reasons_map_to_rules_but_exceptions_do_not() {
        assert_eq!(
            Rule::of_skip("random:KelpBlock.getStateForPlacement"),
            Some(Rule::RandomOutcome)
        );
        assert_eq!(Rule::of_skip("explosion:x"), Some(Rule::Explosion));
        assert_eq!(Rule::of_skip("exception:NullPointerException"), None);
    }

    #[test]
    fn out_of_scope_ticks_cover_subclasses() {
        let sand = mc_bamboo_data::block_of_state(blocks::SAND).expect("沙子存在");
        assert_eq!(tick_exclusion(sand), Some(Rule::FallingTick));
        let chain = mc_bamboo_data::block_of_state(blocks::CHAIN_COMMAND_BLOCK).expect("存在");
        assert_eq!(tick_exclusion(chain), Some(Rule::CommandTick));
        let repeater = mc_bamboo_data::block_of_state(blocks::REPEATER).expect("中继器存在");
        assert_eq!(tick_exclusion(repeater), None);
        let cane = mc_bamboo_data::block_of_state(blocks::SUGAR_CANE).expect("甘蔗存在");
        assert_eq!(tick_exclusion(cane), None);
    }

    #[test]
    fn ticks_compare_in_order_when_every_delay_is_known() {
        let first = [scheduled(1, 1), scheduled(2, 1)];
        let swapped = [scheduled(2, 1), scheduled(1, 1)];
        assert!(same_ticks(&first, &first));
        assert!(!same_ticks(&first, &swapped));
        assert!(!same_ticks(&first, &first[..1]));
    }

    #[test]
    fn random_delays_only_compare_position_and_type() {
        let official = [scheduled(1, Tick::RANDOM_DELAY), scheduled(2, 5)];
        assert!(same_ticks(&official, &[scheduled(2, 5), scheduled(1, 80)]));
        assert!(!same_ticks(&official, &[scheduled(2, 6), scheduled(1, 80)]));
        assert!(!same_ticks(&official, &[scheduled(3, 5), scheduled(1, 80)]));
    }
}
