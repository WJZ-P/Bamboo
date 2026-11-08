//! 用官方代码检查 `mc-bamboo-blocks`。
//!
//! `BlockOracle.java` 在 JVM 里运行官方的 `useItemOn`、`useItem`、`destroyBlock`、计划刻和流体的流动，
//! 有的题之后再走几个游戏刻，一题一行写到 stdout。这里边读边在同样的场景里重放，比较
//! `InteractionResult`、世界的变化、破坏事件、物品栏里多出的物品和范围内还排着的计划刻，
//! 方块和流体的计划刻是两条队列，各自按执行顺序比较。
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

mod cauldron_hand;
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

use mc_bamboo_blocks::{
    BlockState, behaviour::is_ported, state::CRAFTING, state::FACING, state::FACING_HOPPER,
    state::HORIZONTAL_FACING, world::MAX_CHAINED_NEIGHBOR_UPDATES,
};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    Block, BlockAncestor, BlockClass, Item, block_id_of_state, blocks, items, version,
};

use lines::{Case, ContainerStack, Header, Kind, Line, Tick};
use replay::{FLUID_NAMES, Replayed, Scheduled, replay};
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
    EntityRandom,
    RandomLoop,
    RandomPositional,
    Explosion,
    Worldgen,
    ImpossibleScene,
    ItemUseOn,
    CanDestroyBlock,
    RandomDelay,
    CommandTick,
    TimerTick,
    EntityTick,
    EntityRandomTick,
    EntityTrigger,
    Dispensing,
    DaylightTimer,
    UnmodeledContainer,
}

impl Rule {
    pub const ALL: [Self; 18] = [
        Self::AnalogInput,
        Self::EntityRandom,
        Self::RandomLoop,
        Self::RandomPositional,
        Self::Explosion,
        Self::Worldgen,
        Self::ImpossibleScene,
        Self::ItemUseOn,
        Self::CanDestroyBlock,
        Self::RandomDelay,
        Self::CommandTick,
        Self::TimerTick,
        Self::EntityTick,
        Self::EntityRandomTick,
        Self::EntityTrigger,
        Self::Dispensing,
        Self::DaylightTimer,
        Self::UnmodeledContainer,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::AnalogInput => "analog input",
            Self::EntityRandom => "entity random",
            Self::RandomLoop => "random loop",
            Self::RandomPositional => "positional random",
            Self::Explosion => "explosion",
            Self::Worldgen => "worldgen",
            Self::ImpossibleScene => "impossible scene",
            Self::ItemUseOn => "item useOn",
            Self::CanDestroyBlock => "canDestroyBlock",
            Self::RandomDelay => "random delay",
            Self::CommandTick => "command tick",
            Self::TimerTick => "timer tick",
            Self::EntityTick => "entity tick",
            Self::EntityRandomTick => "entity random tick",
            Self::EntityTrigger => "entity trigger",
            Self::Dispensing => "dispensing",
            Self::DaylightTimer => "daylight timer",
            Self::UnmodeledContainer => "unmodeled container",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::AnalogInput => {
                "case: a comparator read a block entity Bamboo keeps no contents for \
                 (lectern, furnace and the like), or a clicked comparator re-reading such an input"
            }
            Self::EntityRandom => {
                "case: the outcome changed with the player's own random source, Bamboo has none"
            }
            Self::RandomLoop => {
                "case: extreme values of the player's random source made the official code spin"
            }
            Self::RandomPositional => {
                "case: the official code asked for a position derived random source"
            }
            Self::Explosion => {
                "case: explosions are out of scope (respawn anchor in the overworld)"
            }
            Self::Worldgen => {
                "case: world generation is out of scope (trees, huge mushrooms, bone meal on grass)"
            }
            Self::ImpossibleScene => {
                "case: a half of a broken structure made the official code throw"
            }
            Self::ItemUseOn => {
                "case: the click reached a non block item whose own useOn is not ported"
            }
            Self::CanDestroyBlock => {
                "case: Item.canDestroyBlock refused, the caller checks it before destroy_block"
            }
            Self::RandomDelay => {
                "tick delay: the player's random source picked it, position and type are compared"
            }
            Self::CommandTick => "tick: command blocks run commands, out of scope",
            Self::TimerTick => {
                "tick: growth, hatching and time of day timers are out of scope (eggs, eyeblossoms)"
            }
            Self::EntityTick => "tick: only entities trigger or read this tick, none are simulated",
            Self::EntityRandomTick => {
                "random tick: it spawns entities (turtles hatching, piglins from portals)"
            }
            Self::EntityTrigger => {
                "scope: plates, detector rails, tripwires, targets, sculk sensors, lightning rods \
                 and primed TNT need entities, projectiles, vibrations or lightning"
            }
            Self::Dispensing => {
                "scope: an item dispenser fired, its item behaviours (arrows, water, armor) are out of scope; a crafter that crafted (the crafting flag turned on, or its slots changed while the combined container items changed) is also out of scope. A crafter tick that fails to craft is still compared"
            }
            Self::DaylightTimer => {
                "scope: daylight detectors follow the sun in a block entity ticker that does not run"
            }
            Self::UnmodeledContainer => {
                "case: a hopper or dropper ticked next to a container Bamboo keeps no slot table for \
                 (shelf, furnace, brewing stand), so the transfer cannot be replayed"
            }
        }
    }

    /// `skip` 行的原因前缀对应的规则。`exception:` 不在这里：那是假世界的 bug，要算失败。
    ///
    /// `untick:<类名>` 是 flow 题往后跑时碰到了范围外的计划刻，`unrandom:<类名>` 是 chunk 题碰到了
    /// 范围外的随机刻，都按那个类的规则计数。
    fn of_skip(reason: &str) -> Option<Self> {
        let (prefix, detail) = reason.split_once(':')?;
        match prefix {
            "random-entity" => Some(Self::EntityRandom),
            "random-loop" => Some(Self::RandomLoop),
            "random-positional" => Some(Self::RandomPositional),
            "explosion" => Some(Self::Explosion),
            "worldgen" => Some(Self::Worldgen),
            "impossible" => Some(Self::ImpossibleScene),
            "untick" => rule_of_class(OUT_OF_SCOPE_TICKS, detail),
            "unrandom" => rule_of_class(OUT_OF_SCOPE_RANDOM_TICKS, detail),
            _ => None,
        }
    }
}

/// 表里官方类名是 `name` 的那一项的规则。
fn rule_of_class(table: &[(BlockAncestor, Rule)], name: &str) -> Option<Rule> {
    table
        .iter()
        .find(|(ancestor, _)| format!("{ancestor:?}") == name)
        .map(|&(_, rule)| rule)
}

/// 不在范围里的随机刻：这些类的 random 题不比较，chunk 题要随机刻到它们时整题排除。
const OUT_OF_SCOPE_RANDOM_TICKS: &[(BlockAncestor, Rule)] = &[
    (BlockAncestor::TurtleEggBlock, Rule::EntityRandomTick),
    (BlockAncestor::NetherPortalBlock, Rule::EntityRandomTick),
];

/// 第 1 章第 5 条以外的计划刻：这些方块排的刻不比较，它们的 `tick` 题也不比较，
/// `settle` 或 flow 题往后跑时执行它们的话整题排除。以后做了其中某一种时，把它从表里删掉，比较就打开了。
const OUT_OF_SCOPE_TICKS: &[(BlockAncestor, Rule)] = &[
    (BlockAncestor::CommandBlock, Rule::CommandTick),
    (BlockAncestor::FrogspawnBlock, Rule::TimerTick),
    (BlockAncestor::SnifferEggBlock, Rule::TimerTick),
    (BlockAncestor::DriedGhastBlock, Rule::TimerTick),
    (BlockAncestor::EyeblossomBlock, Rule::TimerTick),
    (BlockAncestor::CreakingHeartBlock, Rule::TimerTick),
    (BlockAncestor::BasePressurePlateBlock, Rule::EntityTick),
    (BlockAncestor::DetectorRailBlock, Rule::EntityTick),
    (BlockAncestor::TripWireBlock, Rule::EntityTick),
];

/// 这条规则排除了哪些类的计划刻或者随机刻，打印排除表时用。
fn tick_classes(rule: Rule) -> Vec<BlockAncestor> {
    OUT_OF_SCOPE_TICKS
        .iter()
        .chain(OUT_OF_SCOPE_RANDOM_TICKS)
        .filter(|(_, entry)| *entry == rule)
        .map(|&(ancestor, _)| ancestor)
        .collect()
}

/// 没有移植 `useOn` 的非方块物品类，官方类名和原因。点击轮到这些物品时不比较。
///
/// 斧子、锄头、锹（类就是 `Item`，靠 `block_transformer` 组件）、打火石、火焰弹、蜜脾、剪刀、末影之眼、
/// 调试棒、刷子和骨粉移植了，照常比较。桶、玻璃瓶和睡莲走 `Item.use`，不经过 `useOn`，所以不在这张表里。
const UNPORTED_ITEM_USE_ON: &[(&str, &str)] = &[
    ("SpawnEggItem", "spawns an entity"),
    ("ArmorStandItem", "spawns an entity"),
    ("CushionItem", "spawns an entity"),
    ("EndCrystalItem", "spawns an entity"),
    ("FireworkRocketItem", "spawns an entity"),
    ("HangingEntityItem", "spawns an entity"),
    ("ItemFrameItem", "spawns an entity"),
    ("LeadItem", "spawns an entity"),
    ("MinecartItem", "spawns an entity"),
    (
        "PotionItem",
        "turning dirt into mud needs the potion_contents component, actors only carry item ids",
    ),
    ("SplashPotionItem", "inherits the useOn of PotionItem"),
    ("LingeringPotionItem", "inherits the useOn of PotionItem"),
    (
        "CompassItem",
        "a lodestone only changes the held item, no block",
    ),
    ("MapItem", "banner markers live in the map data, no block"),
];

/// 物品类不在 [`UNPORTED_ITEM_USE_ON`] 里时是 `None`。
fn unported_item_use_on(class: &str) -> Option<&'static str> {
    UNPORTED_ITEM_USE_ON
        .iter()
        .find(|(name, _)| *name == class)
        .map(|&(_, reason)| reason)
}

/// 这个方块的计划刻不在比较的范围里时，返回原因。
fn tick_exclusion(block: &Block) -> Option<Rule> {
    class_exclusion(OUT_OF_SCOPE_TICKS, block)
}

/// 这个方块的随机刻不在比较的范围里时，返回原因。
fn random_tick_exclusion(block: &Block) -> Option<Rule> {
    class_exclusion(OUT_OF_SCOPE_RANDOM_TICKS, block)
}

/// 方块的类或者它的父类在表里时，返回那一项的规则。
fn class_exclusion(table: &[(BlockAncestor, Rule)], block: &Block) -> Option<Rule> {
    let ancestors = block.class.ancestors();
    table
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
        args.extend(["--untick".to_owned(), class_names(OUT_OF_SCOPE_TICKS)]);
        args.extend([
            "--unrandom".to_owned(),
            class_names(OUT_OF_SCOPE_RANDOM_TICKS),
        ]);
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

/// 表里的类名，逗号隔开。官方类名就是 `BlockAncestor` 的变体名。
fn class_names(table: &[(BlockAncestor, Rule)]) -> String {
    table
        .iter()
        .map(|(ancestor, _)| format!("{ancestor:?}"))
        .collect::<Vec<_>>()
        .join(",")
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
pub(crate) fn collect_log(child: &mut Child) -> Result<mpsc::Receiver<Vec<String>>, String> {
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
                // 本来就不比较的刻里官方代码抛了异常（比如传送门随机刻在假世界里生成僵尸猪灵），
                // 照不比较算，不算假世界坏了。
                let out_of_scope = match skip.kind {
                    Kind::Tick => tick_exclusion(block),
                    Kind::Random => random_tick_exclusion(block),
                    _ => None,
                };
                match Rule::of_skip(&skip.reason).or(out_of_scope) {
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
        let mismatch = compare(case, &replayed, &self.names, &mut self.report)?;
        if left_to_the_item(case, &replayed, &mismatch) {
            self.report.excluded(block, kind, Rule::ItemUseOn);
            return Ok(());
        }
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

/// 官方的方块没有吃掉这次点击，结果来自还没移植的非方块物品的 `useOn`。
/// 这时官方可以多做事（物品自己的效果），我们做不到；但我们做的每一件事官方都得做了、而且做得一样：
/// 方块多吃了点击、改了官方没改的格子、或者改得不一样，照常算失败。两边都有、而且一样的变化
/// （比如场景里的漏斗在之后几刻搬了东西）不妨碍排除。
///
/// The official block passed the click and the result came from a non-block item whose `useOn` is
/// not ported. Vanilla may do more (the item's own effect), which Bamboo cannot; but everything
/// Bamboo did must also have happened in vanilla, the same way. A block that consumed the click, or a
/// cell Bamboo changed that vanilla left alone or changed differently, still counts as a failure.
/// Changes both sides made identically (say, a hopper in the scene moving items afterwards) do not
/// prevent the exclusion.
fn left_to_the_item(case: &Case, replayed: &Replayed, mismatch: &Mismatch) -> bool {
    let changes_also_official = replayed.changes.iter().all(|(&pos, &state)| {
        case.outcome
            .changes
            .iter()
            .any(|cell| cell.pos() == pos && cell.state() == state)
    });
    case.action.item_use_on
        && case.outcome.item_reached
        && unported_item_use_on(&case.action.item_class).is_some()
        && replayed.result == "pass"
        && changes_also_official
        && (replayed.destroyed.is_empty() || !mismatch.destroyed)
        && (replayed.ticks.is_empty() || !mismatch.ticks)
        && mismatch.panic.is_none()
}

/// 不用重放就知道不比较的题。
fn case_exclusion(case: &Case, block: &Block, names: &Names) -> Result<Option<Rule>, String> {
    if case.outcome.analog_content {
        return Ok(Some(Rule::AnalogInput));
    }
    if dispenser_dispenses(case) || crafter_crafts(case) {
        return Ok(Some(Rule::Dispensing));
    }
    if transfers_with_unmodeled_container(case) {
        return Ok(Some(Rule::UnmodeledContainer));
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
        Kind::Place | Kind::Use if clicks_comparator_with_analog_input(case) => {
            Some(Rule::AnalogInput)
        }
        Kind::Place | Kind::Use if bone_meals_a_feature_plant(case) => Some(Rule::Worldgen),
        Kind::Tick => tick_exclusion(block),
        Kind::Random => random_tick_exclusion(block),
        _ => None,
    };
    match rule {
        Some(rule) => Ok(Some(rule)),
        None => settled_out_of_scope(case, names),
    }
}

/// 骨粉在这些方块上能不能用、成不成功都要读树和巨型蘑菇的地物配置。只比类本身，
/// 红树胎生苗是 `SaplingBlock` 的子类，它的骨粉不用地物，照常比较。
const BONE_MEAL_NEEDS_FEATURES: [BlockClass; 3] = [
    BlockClass::SaplingBlock,
    BlockClass::AzaleaBlock,
    BlockClass::MushroomBlock,
];

/// 拿着骨粉点上面这些方块。
fn bone_meals_a_feature_plant(case: &Case) -> bool {
    case.action.item_class == "BoneMealItem"
        && BONE_MEAL_NEEDS_FEATURES.contains(&scene_state(case, case.action.target()).class())
}

/// 题目开始时这一格的状态。
fn scene_state(case: &Case, pos: BlockPos) -> BlockState {
    let cell = case.scene.iter().find(|cell| cell.pos() == pos);
    BlockState(cell.map_or(blocks::AIR, |cell| cell.state()))
}

/// 点中的比较器会重新读输入。内容已经建模的容器两边读到的一样，可以比较；
/// 别的方块实体（讲台、熔炉这类）Bamboo 没有保留内容，这题只能排除。
fn clicks_comparator_with_analog_input(case: &Case) -> bool {
    let state_at = |pos: BlockPos| scene_state(case, pos);
    let pos = case.action.target();
    let clicked = state_at(pos);
    if block_id_of_state(clicked.0) != block_id_of_state(blocks::COMPARATOR) {
        return false;
    }
    let facing = clicked.get(HORIZONTAL_FACING);
    let input_pos = pos.relative(facing);
    let input = state_at(input_pos);
    let behind = state_at(input_pos.relative(facing));
    let modeled = |state: BlockState| is_modeled_container(state);
    if modeled(input) || input.is_redstone_conductor() && modeled(behind) {
        return false;
    }
    input.has_analog_output_signal()
        || input.is_redstone_conductor() && behind.has_analog_output_signal()
}

/// 内容已经在重放里建模的容器方块。末影箱的槽位在玩家身上，不算。
fn is_modeled_container(state: BlockState) -> bool {
    state.class().ancestors().iter().any(|ancestor| {
        matches!(
            ancestor,
            BlockAncestor::ChestBlock
                | BlockAncestor::BarrelBlock
                | BlockAncestor::ShulkerBoxBlock
                | BlockAncestor::HopperBlock
                | BlockAncestor::DispenserBlock
                | BlockAncestor::JukeboxBlock
                | BlockAncestor::DecoratedPotBlock
                | BlockAncestor::ChiseledBookShelfBlock
                | BlockAncestor::CrafterBlock
        )
    })
}

/// Bamboo 没保留内容的容器方块实体。漏斗吸不出来也放不进去，比较器和投掷器碰不到它们。
fn is_unmodeled_container(state: BlockState) -> bool {
    state.class().ancestors().iter().any(|ancestor| {
        matches!(
            ancestor,
            BlockAncestor::ShelfBlock
                | BlockAncestor::AbstractFurnaceBlock
                | BlockAncestor::BrewingStandBlock
        )
    })
}

/// 状态 ID 不在注册表里时返回 `None`：这题照常交给重放，panic 在那边算一道失败。
/// 排除判断比重放先跑，不能自己碰不合法的 ID。
fn state_of(id: u32) -> Option<BlockState> {
    block_id_of_state(id).map(|_| BlockState(id))
}

/// 动作和 settle 走完之后这一格的状态：`changes` 里记录的是和场景不同的格子。
fn final_state(case: &Case, pos: BlockPos) -> Option<BlockState> {
    let cell = case
        .outcome
        .changes
        .iter()
        .find(|cell| cell.pos() == pos)
        .or_else(|| case.scene.iter().find(|cell| cell.pos() == pos));
    match cell {
        Some(cell) => state_of(cell.state()),
        None => Some(BlockState::AIR),
    }
}

/// 漏斗每刻都从上方吸、往出口放，投掷器 tick 时只往出口放，两边都要读容器的槽位表。
/// 碰到 Bamboo 没建模的容器时，搬物品的结果没法重放，这题只能排除。
fn transfers_with_unmodeled_container(case: &Case) -> bool {
    let scene = case
        .scene
        .iter()
        .filter_map(|cell| Some((cell.pos(), state_of(cell.state())?)));
    let changed = case
        .outcome
        .changes
        .iter()
        .filter_map(|cell| Some((cell.pos(), state_of(cell.state())?)));
    // 漏斗从上方吸，出口在 `facing` 那一格。
    let hopper_hits = scene
        .clone()
        .chain(changed.clone())
        .filter(|(_, state)| state.is_a(BlockAncestor::HopperBlock))
        .any(|(pos, state)| {
            final_state(case, pos.above()).is_some_and(is_unmodeled_container)
                || final_state(case, pos.relative(state.get(FACING_HOPPER)))
                    .is_some_and(is_unmodeled_container)
        });
    if hopper_hits {
        return true;
    }
    // 投掷器没有 ticker，只有动作那一下（或者场景里被红石排上刻）才会动手。
    let target = case.action.target();
    let dropper_ran = case.outcome.ran.iter().any(|id| id == "minecraft:dropper");
    if !(dropper_ran || case.action.kind == Kind::Tick) {
        return false;
    }
    let Some(state) = final_state(case, target) else {
        return false;
    };
    state.is_a(BlockAncestor::DropperBlock)
        && final_state(case, target.relative(state.get(FACING))).is_some_and(is_unmodeled_container)
}

/// 合成成功才整题排除。没有配方时官方只发失败事件 1050，方块和格子都不变，那题照常比较。
///
/// 成功的痕迹：`crafting` 被设上，或者合成器的格子变了并且容器里的物品种类或数量变了。
/// 漏斗只是把同一件物品搬到另一格，两边的物品合计不变，不算合成。
/// `CrafterBlock.dispenseFrom` 和 `serverTick` 不在这次范围里。
///
/// Only a successful craft excludes the case. With no recipe, vanilla only plays fail event
/// 1050, the block and the slots stay put, and that case is still compared.
///
/// Success is the `crafting` flag turning on, or the crafter's slots changing while the
/// combined container items change. A hopper only moves the same items, so the totals stay
/// equal and that is not a craft. `CrafterBlock.dispenseFrom` and `serverTick` are out of scope.
fn crafter_crafts(case: &Case) -> bool {
    // `crafting` 只在合成成功时才会被设上，不管这一刻是题目本身还是之后几刻里跑的。
    // `crafting` only turns on after a successful craft, whether the crafter ticked as the
    // action itself or during the ticks that follow it.
    crafting_turned_on(case)
        || (crafter_ran(case) && crafter_slots_changed(case) && container_items_changed(case))
}

/// 动作本身是合成器的计划刻，或者 `settle` 里跑过它。
///
/// The action is the crafter's scheduled tick, or `settle` ran one.
fn crafter_ran(case: &Case) -> bool {
    case.outcome.ran.iter().any(|id| id == "minecraft:crafter")
        || case.action.kind == Kind::Tick && case.block == "minecraft:crafter"
}

/// 结果里某一格的 `crafting` 从关变成开。
///
/// Some cell's `crafting` flag turned on.
fn crafting_turned_on(case: &Case) -> bool {
    case.outcome.changes.iter().any(|cell| {
        let Some(state) = state_of(cell.state()) else {
            return false;
        };
        crafting_is_on(state) && !crafting_is_on(scene_state(case, cell.pos()))
    })
}

fn crafting_is_on(state: BlockState) -> bool {
    state.is_a(BlockAncestor::CrafterBlock) && state.get(CRAFTING)
}

/// 合成器上的非空格子和开始时不一样。
///
/// The non-empty slots on crafters differ from the start of the case.
fn crafter_slots_changed(case: &Case) -> bool {
    let at_crafter = |stack: &ContainerStack| {
        let pos = stack.pos();
        scene_state(case, pos).is_a(BlockAncestor::CrafterBlock)
            || final_state(case, pos).is_some_and(|state| state.is_a(BlockAncestor::CrafterBlock))
    };
    let before: Vec<_> = case
        .contents
        .iter()
        .filter(|stack| at_crafter(stack))
        .collect();
    let after: Vec<_> = case
        .outcome
        .contents
        .iter()
        .filter(|stack| at_crafter(stack))
        .collect();
    before != after
}

/// 全部已建模容器里的物品合计变了。漏斗搬家不会变。
///
/// The combined items in every modelled container changed. A hopper move does not.
fn container_items_changed(case: &Case) -> bool {
    item_totals(&case.contents) != item_totals(&case.outcome.contents)
}

fn item_totals(stacks: &[ContainerStack]) -> BTreeMap<&str, i32> {
    let mut totals = BTreeMap::new();
    for stack in stacks {
        *totals.entry(stack.item()).or_default() += stack.count();
    }
    totals
}

/// 发射器的物品行为（箭、水、盔甲等）不在范围里，Bamboo 的发射器 tick 什么也不做。
/// 场景里的发射器有东西又真的 tick 了，官方那边已经发出去，这题只能排除。投掷器自己实现了，照常比较。
fn dispenser_dispenses(case: &Case) -> bool {
    let is_dispenser = |pos: BlockPos| {
        let state = scene_state(case, pos);
        state.is_a(BlockAncestor::DispenserBlock) && !state.is_a(BlockAncestor::DropperBlock)
    };
    let ticked = case
        .outcome
        .ran
        .iter()
        .any(|id| id == "minecraft:dispenser")
        || case.action.kind == Kind::Tick && case.block == "minecraft:dispenser";
    if ticked {
        return case.contents.iter().any(|stack| is_dispenser(stack.pos()));
    }
    // 流动题里被红石排上的发射器刻由官方假世界反射调用 `ServerLevel.tickBlock`，
    // 不经过 FakeLevel 记 `ran` 的那一层，所以上面看不出来。官方发出去之后槽位里的东西变少，
    // 用前后的数量判断。上面有漏斗的格子排除：那是漏斗正常吸走一件，Bamboo 能重放。
    if case.action.kind != Kind::Flow {
        return false;
    }
    let mut before: BTreeMap<BlockPos, i32> = BTreeMap::new();
    for stack in &case.contents {
        *before.entry(stack.pos()).or_default() += stack.count();
    }
    let mut after: BTreeMap<BlockPos, i32> = BTreeMap::new();
    for stack in &case.outcome.contents {
        *after.entry(stack.pos()).or_default() += stack.count();
    }
    before.iter().any(|(&pos, &count)| {
        let hopper_above = final_state(case, pos.above())
            .is_some_and(|state| state.is_a(BlockAncestor::HopperBlock));
        after.get(&pos).copied().unwrap_or(0) < count && is_dispenser(pos) && !hopper_above
    })
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
        contents: answer.contents != replayed.contents,
        ticks: !same_queues(&official, &ours),
        compared_ticks: (official, ours),
        gained: answer.gained != replayed.gained,
        panic: None,
    })
}

/// 方块和流体是两条队列，各自比较。
fn same_queues(official: &[Scheduled], ours: &[Scheduled]) -> bool {
    let queue = |ticks: &[Scheduled], fluid: bool| -> Vec<Scheduled> {
        ticks
            .iter()
            .filter(|tick| tick.fluid == fluid)
            .cloned()
            .collect()
    };
    [false, true]
        .into_iter()
        .all(|fluid| same_ticks(&queue(official, fluid), &queue(ours, fluid)))
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

/// 官方的计划刻里要比较的部分，每条队列按执行顺序排好。不比较的条目计入排除表。
fn comparable_ticks(
    ticks: &[Tick],
    names: &Names,
    report: &mut Report,
) -> Result<Vec<Scheduled>, String> {
    let mut comparable = Vec::with_capacity(ticks.len());
    for tick in ticks {
        let id = if tick.is_fluid() {
            fluid_name(tick.id())?
        } else {
            let block = names.block(tick.id())?;
            if let Some(rule) = tick_exclusion(block) {
                report.excluded_entry(rule);
                continue;
            }
            block.name
        };
        if tick.delay() == Tick::RANDOM_DELAY {
            report.excluded_entry(Rule::RandomDelay);
        }
        comparable.push(Scheduled {
            pos: tick.pos(),
            fluid: tick.is_fluid(),
            id,
            delay: tick.delay(),
            priority: tick.priority(),
        });
    }
    comparable.sort_by_key(|tick| (tick.fluid, tick.delay, tick.priority));
    Ok(comparable)
}

/// 官方的流体名，也就是 [`FLUID_NAMES`] 里的那一个。
fn fluid_name(name: &str) -> Result<&'static str, String> {
    FLUID_NAMES
        .iter()
        .find(|&&known| known == name)
        .copied()
        .ok_or_else(|| format!("unknown fluid {name}, are the generated files stale?"))
}

fn in_scope_scheduled(ticks: &[Scheduled], names: &Names) -> Result<Vec<Scheduled>, String> {
    let mut comparable = Vec::with_capacity(ticks.len());
    for tick in ticks {
        if tick.fluid || tick_exclusion(names.block(tick.id)?).is_none() {
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
                && candidate.id == tick.id
                && candidate.priority == tick.priority
                && (tick.delay == Tick::RANDOM_DELAY || candidate.delay == tick.delay)
        });
        found.map(|index| remaining.swap_remove(index)).is_some()
    })
}

/// 这道题碰过的、还没移植的类。手上方块物品放的方块不一定进了世界，也算碰过。
fn blockers(case: &Case, replayed: &Replayed, names: &Names) -> Result<Vec<BlockClass>, String> {
    let (_, held) = names.item(case.action.used_item())?;
    let held_blocks = [held.block, held.wall_block].into_iter().flatten();
    let mut classes: Vec<BlockClass> = replayed
        .touched
        .iter()
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
    use mc_bamboo_blocks::state::HAS_RECORD;

    use super::*;

    fn scheduled(x: i32, delay: i32) -> Scheduled {
        Scheduled {
            pos: BlockPos::new(x, 64, 0),
            fluid: false,
            id: "minecraft:sugar_cane",
            delay,
            priority: 0,
        }
    }

    fn fluid_tick(x: i32, delay: i32) -> Scheduled {
        Scheduled {
            fluid: true,
            id: "minecraft:water",
            ..scheduled(x, delay)
        }
    }

    const HEADER: &str = r#"{"type":"header","version":"26.3","protocol":777,"seed":1,"cases":1,
        "maxChainedUpdates":1000000,"gameTime":100000,"minY":-64,"maxY":319}"#;
    const END: &str = r#"{"type":"end","cases":1,"skipped":0}"#;

    /// 在 (0, 64, 0) 的石头顶面放一块石头，`scene` 和 `outcome` 由测试给。
    fn place_stone(scene: &str, outcome: &str) -> String {
        click_top("minecraft:stone", "BlockItem", scene, outcome)
    }

    /// 主手拿着 `item` 点 (0, 64, 0) 的顶面。
    fn click_top(item: &str, class: &str, scene: &str, outcome: &str) -> String {
        let use_on = class != "BlockItem";
        let action = format!(
            r#"{{"kind":"use","main":"{item}","off":"minecraft:air","hand":"main",
            "itemClass":"{class}","itemUseOn":{use_on},"pos":[0,64,0],"face":"up",
            "loc":["3fe0000000000000","4050400000000000","3fe0000000000000"],"inside":false,
            "actor":["4014000000000000","4050400000000000","4014000000000000"],
            "yaw":"00000000","pitch":"00000000","sneak":false,
            "box":["4012666666666666","4050400000000000","4012666666666666",
                   "4015999999999999","4050c00000000000","4015999999999999"],"others":[],"settle":0}}"#
        )
        .replace('\n', " ");
        format!(
            r#"{{"type":"case","id":0,"seed":31,"block":"minecraft:stone","class":"Block","scene":{scene},"action":{action},"outcome":{outcome}}}"#
        )
    }

    fn answer(result: &str, changes: &str) -> String {
        answer_with_ticks(result, changes, "[]")
    }

    fn answer_with_ticks(result: &str, changes: &str, ticks: &str) -> String {
        format!(
            r#"{{"result":"{result}","changes":{changes},"destroyed":[],"ticks":{ticks},"ran":[],"analogContent":false,"itemReached":true,"spawned":[],"rng":false}}"#
        )
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
        // 官方的命令方块在 settle 里跑了计划刻，我们不跑，这一题只能排除。
        // 沙子以前也走这条排除，下落已经做了，不再用它当例子。
        //
        // A command block that ticked during settle is still out of scope, so the case is excluded.
        // Sand used to take this path; falling is implemented, so it is no longer the example.
        let command = answer_after_ticks("success", "[]", r#""minecraft:command_block""#);
        assert_eq!(verdict_of(&place_stone(&scene, &command)), Ok(()));
        let lamp = answer_after_ticks("success", "[]", r#""minecraft:redstone_lamp""#);
        assert!(verdict_of(&place_stone(&scene, &lamp)).is_err());
    }

    #[test]
    fn a_case_that_read_unsupported_container_contents_is_not_compared() {
        let stone = blocks::STONE;
        let scene = format!("[[0,64,0,{stone}]]");
        // 官方标记了 analogContent：比较器读到了还没建模的容器内容。
        let read =
            answer("fail", "[]").replace("\"analogContent\":false", "\"analogContent\":true");
        assert_eq!(verdict_of(&place_stone(&scene, &read)), Ok(()));
        // 同一道题不标记时照常比较，结果是错的就失败。
        assert!(verdict_of(&place_stone(&scene, &answer("fail", "[]"))).is_err());
    }

    #[test]
    fn a_ported_item_use_on_is_compared() {
        let scene = format!("[[0,64,0,{}]]", blocks::STONE);
        let fire = format!("[[0,65,0,{}]]", blocks::FIRE);
        let flint = "minecraft:flint_and_steel";
        // 点着的火在自己身上排了下一个刻，延迟来自世界随机源，写成官方的“随机延迟”。
        let fire_tick = r#"[[0,65,0,"block","minecraft:fire",-1,0]]"#;
        let lit = click_top(
            flint,
            "FlintAndSteelItem",
            &scene,
            &answer_with_ticks("success", &fire, fire_tick),
        );
        assert_eq!(verdict_of(&lit), Ok(()));
        let left_alone = click_top(flint, "FlintAndSteelItem", &scene, &answer("pass", "[]"));
        assert!(verdict_of(&left_alone).is_err());
    }

    #[test]
    fn an_unported_item_use_on_is_not_compared() {
        let scene = format!("[[0,64,0,{}]]", blocks::STONE);
        let hung = format!("[[0,64,1,{}]]", blocks::STONE);
        let case = click_top(
            "minecraft:painting",
            "HangingEntityItem",
            &scene,
            &answer("success", &hung),
        );
        assert_eq!(verdict_of(&case), Ok(()));
        assert!(unported_item_use_on("BoneMealItem").is_none());
    }

    #[test]
    fn a_comparator_reading_an_analog_output_is_not_compared() {
        let scene = format!(
            "[[0,64,0,{}],[0,64,-1,{}]]",
            blocks::COMPARATOR,
            blocks::LECTERN
        );
        let case = place_stone(&scene, &answer("fail", "[]"));
        assert_eq!(verdict_of(&case), Ok(()));
    }

    #[test]
    fn a_comparator_reading_a_scene_jukebox_is_compared() {
        let record = BlockState(blocks::JUKEBOX).with(HAS_RECORD, true);
        let scene = format!("[[0,64,0,{}],[0,64,-1,{}]]", blocks::COMPARATOR, record.0);
        // 假世界补的是 `music_disc_13`，比较器读到 1。错的答案不再被整题跳过。
        //
        // The fake world fills `music_disc_13` and the comparator reads 1. A wrong answer is
        // no longer skipped for the whole case.
        let case = place_stone(&scene, &answer("fail", "[]"));
        assert!(verdict_of(&case).is_err());
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
        let java_args = options.java_args();
        assert_eq!(java_args[..4], options.forwarded);
        assert_eq!(java_args[4], "--untick");
        assert!(java_args[5].split(',').any(|class| class == "CommandBlock"));
        assert!(!java_args[5].split(',').any(|class| class == "FallingBlock"));
        assert_eq!(java_args[6], "--unrandom");
        assert!(
            java_args[7]
                .split(',')
                .any(|class| class == "TurtleEggBlock")
        );
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
            Rule::of_skip("random-entity:Player.eat"),
            Some(Rule::EntityRandom)
        );
        // 世界的随机源和官方逐位相同，不再有这种排除。
        assert_eq!(Rule::of_skip("random:KelpBlock.getStateForPlacement"), None);
        assert_eq!(Rule::of_skip("explosion:x"), Some(Rule::Explosion));
        assert_eq!(Rule::of_skip("untick:FallingBlock"), None);
        assert_eq!(
            Rule::of_skip("untick:CommandBlock"),
            Some(Rule::CommandTick)
        );
        assert_eq!(Rule::of_skip("untick:StairBlock"), None);
        assert_eq!(
            Rule::of_skip("unrandom:TurtleEggBlock"),
            Some(Rule::EntityRandomTick)
        );
        assert_eq!(Rule::of_skip("exception:NullPointerException"), None);
    }

    #[test]
    fn block_and_fluid_queues_are_compared_separately() {
        let official = [scheduled(1, 5), fluid_tick(2, 5)];
        assert!(same_queues(&official, &[fluid_tick(2, 5), scheduled(1, 5)]));
        assert!(!same_queues(&official, &[scheduled(1, 5)]));
        assert!(!same_queues(
            &official,
            &[scheduled(1, 5), fluid_tick(2, 30)]
        ));
    }

    #[test]
    fn liquids_tick_inside_the_comparison() {
        let water = mc_bamboo_data::block_of_state(blocks::WATER).expect("水存在");
        assert_eq!(tick_exclusion(water), None);
        assert_eq!(
            fluid_name("minecraft:flowing_lava"),
            Ok("minecraft:flowing_lava")
        );
        assert!(fluid_name("minecraft:honey").is_err());
    }

    #[test]
    fn out_of_scope_ticks_cover_subclasses() {
        let sand = mc_bamboo_data::block_of_state(blocks::SAND).expect("沙子存在");
        assert_eq!(tick_exclusion(sand), None);
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
