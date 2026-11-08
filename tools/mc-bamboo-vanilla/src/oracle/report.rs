//! 比较结果的汇总：按类和题型计数，留下前几个失败的完整场景。

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write,
};

use mc_bamboo_blocks::behaviour::{Family, family_of, is_ported};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{Block, BlockClass, block_of_state, blocks};

use super::{
    Rule, UNPORTED_ITEM_USE_ON,
    lines::{Case, Cell, ContainerStack, Destroyed, Header, Kind},
    replay::{Replayed, Scheduled},
    tick_classes,
};

/// 打印前多少个挡路的类。
const TOP_BLOCKERS: usize = 25;

/// 和官方不一致的地方。
#[derive(Debug, Default)]
pub struct Mismatch {
    pub result: bool,
    /// 结果不同的格子。
    pub cells: BTreeSet<BlockPos>,
    pub destroyed: bool,
    /// 容器内容不同。
    pub contents: bool,
    pub ticks: bool,
    /// 官方和我们范围内的计划刻。
    pub compared_ticks: (Vec<Scheduled>, Vec<Scheduled>),
    /// 物品栏里新出现的物品不同。
    pub gained: bool,
    pub panic: Option<String>,
}

impl Mismatch {
    pub fn is_empty(&self) -> bool {
        !self.result
            && self.cells.is_empty()
            && !self.destroyed
            && !self.contents
            && !self.ticks
            && !self.gained
            && self.panic.is_none()
    }
}

/// 一组题的计数。
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    cases: u64,
    passed: u64,
    failed: u64,
    blocked: u64,
    excluded: u64,
    /// 比较过的题里官方改变了世界的题数。太少说明这一行的通过没有多少分量。
    changed: u64,
}

impl Tally {
    fn add(&mut self, other: Self) {
        self.cases += other.cases;
        self.passed += other.passed;
        self.failed += other.failed;
        self.blocked += other.blocked;
        self.excluded += other.excluded;
        self.changed += other.changed;
    }

    /// 比较过的题里通过的比例。
    fn pass_rate(self) -> f64 {
        let compared = self.cases - self.excluded;
        if compared == 0 {
            return 100.0;
        }
        self.passed as f64 * 100.0 / compared as f64
    }
}

/// 一次运行的全部结果。
pub struct Report {
    show: usize,
    show_blocked: bool,
    seed: i64,
    rows: HashMap<(BlockClass, Kind), Tally>,
    rules: BTreeMap<Rule, u64>,
    blockers: HashMap<BlockClass, u64>,
    /// 每个类和题型已经留下了几个失败。
    shown: HashMap<(BlockClass, Kind), usize>,
    /// 题号和失败的全部内容。
    details: Vec<(u64, String)>,
    broken: Vec<String>,
    /// `case` 和 `skip` 行的数量，和 `end` 行对账。
    lines: (u64, u64),
}

impl Report {
    pub fn new(show: usize, show_blocked: bool) -> Self {
        Self {
            show,
            show_blocked,
            seed: 0,
            rows: HashMap::new(),
            rules: BTreeMap::new(),
            blockers: HashMap::new(),
            shown: HashMap::new(),
            details: Vec::new(),
            broken: Vec::new(),
            lines: (0, 0),
        }
    }

    pub fn start(&mut self, header: &Header) {
        self.seed = header.seed;
        println!(
            "oracle: Minecraft {} (protocol {}), seed {}",
            header.version, header.protocol, header.seed
        );
    }

    fn row(&mut self, block: &Block, kind: Kind) -> &mut Tally {
        let row = self.rows.entry((block.class, kind)).or_default();
        row.cases += 1;
        row
    }

    pub fn passed(&mut self, block: &Block, case: &Case) {
        self.lines.0 += 1;
        let changed = u64::from(!case.outcome.changes.is_empty());
        let row = self.row(block, case.action.kind);
        row.passed += 1;
        row.changed += changed;
    }

    /// 官方有答案，但是这道题不比较。
    pub fn excluded(&mut self, block: &Block, kind: Kind, rule: Rule) {
        self.lines.0 += 1;
        self.count_excluded(block, kind, rule);
    }

    /// 官方给不出唯一的答案。
    pub fn skipped(&mut self, block: &Block, kind: Kind, rule: Rule) {
        self.lines.1 += 1;
        self.count_excluded(block, kind, rule);
    }

    fn count_excluded(&mut self, block: &Block, kind: Kind, rule: Rule) {
        self.row(block, kind).excluded += 1;
        *self.rules.entry(rule).or_default() += 1;
    }

    /// 一题里有一项不比较。
    pub fn excluded_entry(&mut self, rule: Rule) {
        *self.rules.entry(rule).or_default() += 1;
    }

    /// 官方代码在假世界里抛了异常，这是假世界的 bug，不算排除。
    pub fn broken_oracle(&mut self, block: &Block, kind: Kind, reason: &str) {
        self.lines.1 += 1;
        self.row(block, kind).failed += 1;
        self.broken
            .push(format!("{} {}: {reason}", kind.name(), block.name));
    }

    /// 结果和官方不同。`blockers` 是这道题碰过的、还没移植的类，panic 时它是空的。
    pub fn mismatched(
        &mut self,
        block: &Block,
        case: &Case,
        replayed: Option<&Replayed>,
        mismatch: &Mismatch,
        blockers: &[BlockClass],
    ) {
        self.lines.0 += 1;
        let kind = case.action.kind;
        let changed = u64::from(!case.outcome.changes.is_empty());
        let row = self.row(block, kind);
        row.changed += changed;
        if blockers.is_empty() {
            row.failed += 1;
        } else {
            row.blocked += 1;
        }
        for &class in blockers {
            *self.blockers.entry(class).or_default() += 1;
        }
        if !blockers.is_empty() && !self.show_blocked {
            return;
        }
        let shown = self.shown.entry((block.class, kind)).or_default();
        if *shown < self.show {
            *shown += 1;
            let text = render_case(case, block, replayed, mismatch, blockers);
            self.details.push((case.id, text));
        }
    }

    /// 生存挖掘题。算进 `end` 行的 case 数。
    ///
    /// A survival-mining case. It counts toward the `end` line's case total.
    pub fn mine_checked(&mut self, block: &Block, ok: bool, detail: &str) {
        self.lines.0 += 1;
        let row = self.row(block, Kind::Destroy);
        if ok {
            row.passed += 1;
        } else {
            row.failed += 1;
            self.details.push((0, detail.to_owned()));
        }
    }

    /// 和 `end` 行对账，少了行说明读漏了。
    pub fn check_totals(&self, cases: u64, skipped: u64) -> Result<(), String> {
        if self.lines == (cases, skipped) {
            return Ok(());
        }
        Err(format!(
            "the end line says {cases} cases and {skipped} skips, {} and {} were read",
            self.lines.0, self.lines.1
        ))
    }

    /// 打印结果，最重要的放在最后，终端里不用往回翻。
    pub fn print(&self, rerun_args: &str) {
        self.print_details(rerun_args);
        self.print_rows();
        self.print_blockers();
        self.print_rules();
        self.print_families();
        self.print_total();
    }

    fn print_details(&self, rerun_args: &str) {
        for (id, text) in &self.details {
            let rerun = format!(
                "oracle <java> --seed {} --case {id} {rerun_args}",
                self.seed
            );
            println!("\n{text}\n  rerun  {}", rerun.trim_end());
        }
        for error in &self.broken {
            println!("\n[broken oracle] {error}");
        }
    }

    fn print_rows(&self) {
        println!(
            "\n{:<34} {:<8} {:>7} {:>7} {:>6} {:>7} {:>8} {:>7}",
            "class", "kind", "cases", "pass", "fail", "blocked", "excluded", "changed"
        );
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by_key(|((class, kind), _)| (format!("{class:?}"), *kind));
        for ((class, kind), tally) in rows {
            let unported = if is_ported(*class) { "" } else { " (unported)" };
            println!(
                "{:<34} {:<8} {:>7} {:>7} {:>6} {:>7} {:>8} {:>7}{unported}",
                format!("{class:?}"),
                kind.name(),
                tally.cases,
                tally.passed,
                tally.failed,
                tally.blocked,
                tally.excluded,
                tally.changed,
            );
        }
    }

    fn print_blockers(&self) {
        if self.blockers.is_empty() {
            return;
        }
        println!("\nunported classes behind the blocked cases (a case can name several):");
        let mut blockers: Vec<_> = self.blockers.iter().collect();
        blockers.sort_by_key(|(class, count)| (std::cmp::Reverse(**count), format!("{class:?}")));
        for (class, count) in blockers.iter().take(TOP_BLOCKERS) {
            println!("  {count:>7} {class:?}");
        }
        if blockers.len() > TOP_BLOCKERS {
            println!("  ... and {} more", blockers.len() - TOP_BLOCKERS);
        }
    }

    fn print_rules(&self) {
        println!("\nnot compared:");
        for rule in Rule::ALL {
            let hits = self.rules.get(&rule).copied().unwrap_or(0);
            println!("  {hits:>7} {:<18} {}", rule.name(), rule.reason());
            let classes = tick_classes(rule);
            if !classes.is_empty() {
                println!("          {:<18} {classes:?}", "");
            }
            if rule == Rule::ItemUseOn {
                for (class, reason) in UNPORTED_ITEM_USE_ON {
                    println!("          {:<18} {class}: {reason}", "");
                }
            }
        }
        println!(
            "  {:>7} {:<18} outcome fields that only record",
            "-", "spawned, rng"
        );
    }

    fn family_tallies(&self) -> BTreeMap<Family, Tally> {
        let mut tallies: BTreeMap<Family, Tally> = BTreeMap::new();
        for ((class, _), tally) in &self.rows {
            tallies.entry(family_of(*class)).or_default().add(*tally);
        }
        tallies
    }

    fn print_families(&self) {
        println!(
            "\n{:<9} {:>15} {:>8} {:>8} {:>6} {:>8} {:>8} {:>6}",
            "family", "classes ported", "cases", "pass", "fail", "blocked", "excluded", "pass%"
        );
        let tallies = self.family_tallies();
        for family in Family::ALL {
            let tally = tallies.get(&family).copied().unwrap_or_default();
            let (ported, classes) = ported_classes(family);
            println!(
                "{:<9} {:>15} {:>8} {:>8} {:>6} {:>8} {:>8} {:>6.1}",
                family.name(),
                format!("{ported}/{classes}"),
                tally.cases,
                tally.passed,
                tally.failed,
                tally.blocked,
                tally.excluded,
                tally.pass_rate(),
            );
        }
    }

    fn total(&self) -> Tally {
        let mut total = Tally::default();
        for tally in self.rows.values() {
            total.add(*tally);
        }
        total
    }

    /// 题目的类还没移植、结果又和官方不同的题数。这些题是待办，不是挡路。
    fn unported_mismatches(&self) -> u64 {
        self.rows
            .iter()
            .filter(|((class, _), _)| !is_ported(*class))
            .map(|(_, tally)| tally.blocked)
            .sum()
    }

    fn print_total(&self) {
        let total = self.total();
        let unported = self.unported_mismatches();
        println!(
            "\ntotal: {} cases, {} pass, {} fail, {} blocked ({} of unported classes, {} by unported neighbours), {} excluded",
            total.cases,
            total.passed,
            total.failed,
            total.blocked,
            unported,
            total.blocked - unported,
            total.excluded,
        );
    }

    /// 真的 bug、假世界的异常、没移植的类和官方不同，都算失败。
    pub fn verdict(&self) -> Result<(), String> {
        let failed = self.total().failed;
        let unported = self.unported_mismatches();
        if failed == 0 && unported == 0 {
            return Ok(());
        }
        Err(format!(
            "oracle failed: {failed} failures, {unported} differences in unported classes"
        ))
    }
}

/// 这个家族有多少类已经移植，总共多少类。
fn ported_classes(family: Family) -> (usize, usize) {
    let classes: BTreeSet<String> = blocks::BLOCKS
        .iter()
        .filter(|block| family_of(block.class) == family)
        .map(|block| format!("{:?}", block.class))
        .collect();
    let ported: BTreeSet<String> = blocks::BLOCKS
        .iter()
        .filter(|block| family_of(block.class) == family && is_ported(block.class))
        .map(|block| format!("{:?}", block.class))
        .collect();
    (ported.len(), classes.len())
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// 状态的文字形式，比如 `oak_log[axis=y]`。
pub fn describe_state(id: u32) -> String {
    let Some(block) = block_of_state(id) else {
        return format!("<state {id}>");
    };
    let name = short(block.name);
    if block.properties.is_empty() {
        return name.to_owned();
    }
    // 状态 ID 是属性值的笛卡尔积，最后一个属性变化最快。
    let mut offset = id - block.first_state;
    let mut values = Vec::with_capacity(block.properties.len());
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        let value = property.values[(offset % count) as usize];
        values.push(format!("{}={value}", property.name));
        offset /= count;
    }
    values.reverse();
    format!("{name}[{}]", values.join(","))
}

fn pos_text(pos: BlockPos) -> String {
    format!("({}, {}, {})", pos.x, pos.y, pos.z)
}

fn cell_lines(text: &mut String, cells: &[(BlockPos, u32)], differing: &BTreeSet<BlockPos>) {
    for &(pos, state) in cells {
        let mark = if differing.contains(&pos) { "*" } else { " " };
        let _ = writeln!(
            text,
            "  {mark}    {} {}",
            pos_text(pos),
            describe_state(state)
        );
    }
}

fn destroyed_lines(text: &mut String, events: &[Destroyed], differ: bool) {
    let mark = if differ { "*" } else { " " };
    for &Destroyed(x, y, z, state, excluded) in events {
        let who = if excluded { " (skips the player)" } else { "" };
        let _ = writeln!(
            text,
            "  {mark}    destroyed {} {}{who}",
            pos_text(BlockPos::new(x, y, z)),
            describe_state(state)
        );
    }
}

fn tick_lines(text: &mut String, ticks: &[Scheduled], differ: bool) {
    let mark = if differ { "*" } else { " " };
    for tick in ticks {
        let queue = if tick.fluid { "fluid tick" } else { "tick" };
        let _ = writeln!(
            text,
            "  {mark}    {queue} {} {} delay {} priority {}",
            pos_text(tick.pos),
            short(tick.id),
            tick.delay,
            tick.priority
        );
    }
}

fn gained_lines(text: &mut String, gained: &[String], differ: bool) {
    let mark = if differ { "*" } else { " " };
    for item in gained {
        let _ = writeln!(text, "  {mark}    gained {}", short(item));
    }
}

fn contents_lines(text: &mut String, contents: &[ContainerStack], differ: bool) {
    let mark = if differ { "*" } else { " " };
    for stack in contents {
        let _ = writeln!(
            text,
            "  {mark}    contents {} slot {} {} x{}",
            pos_text(stack.pos()),
            stack.slot(),
            short(stack.item()),
            stack.count()
        );
    }
}

fn click_line(case: &Case) -> String {
    let action = &case.action;
    match (action.face, action.loc, action.inside) {
        (Some(face), Some([x, y, z]), Some(inside)) => format!(
            "click  {} face {} at ({}, {}, {}) inside {inside}",
            pos_text(action.target()),
            face.name(),
            x.0,
            y.0,
            z.0
        ),
        _ if action.kind == Kind::Flow => format!(
            "target {} then {} ticks",
            pos_text(action.target()),
            action.steps
        ),
        _ if action.kind == Kind::Chunk => format!(
            "chunk of {} at speed {}",
            pos_text(action.target()),
            action.speed
        ),
        _ => format!("target {}", pos_text(action.target())),
    }
}

/// 一个失败的全部内容，移植的人读完就知道场景和两边的结果。
fn render_case(
    case: &Case,
    block: &Block,
    replayed: Option<&Replayed>,
    mismatch: &Mismatch,
    blockers: &[BlockClass],
) -> String {
    let action = &case.action;
    let mut text = String::new();
    let label = if blockers.is_empty() {
        "fail"
    } else {
        "blocked"
    };
    let _ = writeln!(
        text,
        "[{label}] #{} {} {} ({:?})",
        case.id,
        action.kind.name(),
        short(block.name),
        block.class
    );
    let _ = writeln!(text, "  {}", click_line(case));
    let [x, y, z] = action.actor.map(|value| value.0);
    let _ = writeln!(
        text,
        "  player ({x}, {y}, {z}) yaw {} pitch {} sneak {}",
        action.yaw.0, action.pitch.0, action.sneak
    );
    let _ = writeln!(
        text,
        "  hands  main {}, off {}, using {:?}",
        short(&action.main),
        short(&action.off),
        action.hand
    );
    let _ = writeln!(text, "  scene");
    let scene: Vec<_> = case.scene.iter().map(|c| (c.pos(), c.state())).collect();
    cell_lines(&mut text, &scene, &BTreeSet::new());

    render_official(&mut text, case, mismatch);
    render_bamboo(&mut text, replayed, mismatch);
    if !blockers.is_empty() {
        let names: Vec<String> = blockers.iter().map(|class| format!("{class:?}")).collect();
        let _ = writeln!(text, "  unported {}", names.join(", "));
    }
    text.truncate(text.trim_end().len());
    text
}

fn render_official(text: &mut String, case: &Case, mismatch: &Mismatch) {
    let answer = &case.outcome;
    let mark = if mismatch.result { "*" } else { " " };
    let _ = writeln!(text, "  official {mark}{}", answer.result);
    let cells: Vec<_> = answer.changes.iter().map(|&c| to_pair(c)).collect();
    cell_lines(text, &cells, &mismatch.cells);
    destroyed_lines(text, &answer.destroyed, mismatch.destroyed);
    contents_lines(text, &answer.contents, mismatch.contents);
    tick_lines(text, &mismatch.compared_ticks.0, mismatch.ticks);
    gained_lines(text, &answer.gained, mismatch.gained);
}

fn render_bamboo(text: &mut String, replayed: Option<&Replayed>, mismatch: &Mismatch) {
    let Some(replayed) = replayed else {
        let message = mismatch.panic.as_deref().unwrap_or("no result");
        let _ = writeln!(text, "  bamboo   panicked: {message}");
        return;
    };
    let mark = if mismatch.result { "*" } else { " " };
    let _ = writeln!(text, "  bamboo   {mark}{}", replayed.result);
    let cells: Vec<_> = replayed
        .changes
        .iter()
        .map(|(&pos, &state)| (pos, state))
        .collect();
    cell_lines(text, &cells, &mismatch.cells);
    destroyed_lines(text, &replayed.destroyed, mismatch.destroyed);
    contents_lines(text, &replayed.contents, mismatch.contents);
    tick_lines(text, &mismatch.compared_ticks.1, mismatch.ticks);
    gained_lines(text, &replayed.gained, mismatch.gained);
}

const fn to_pair(cell: Cell) -> (BlockPos, u32) {
    (cell.pos(), cell.state())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_print_like_the_game() {
        assert_eq!(describe_state(blocks::STONE), "stone");
        assert_eq!(describe_state(blocks::OAK_LOG), "oak_log[axis=y]");
        assert_eq!(
            describe_state(blocks::OAK_SLAB),
            "oak_slab[type=bottom,waterlogged=false]"
        );
        assert_eq!(describe_state(u32::MAX), format!("<state {}>", u32::MAX));
    }

    #[test]
    fn pass_rate_ignores_excluded_cases() {
        let tally = Tally {
            cases: 10,
            passed: 4,
            failed: 4,
            blocked: 0,
            excluded: 2,
            changed: 0,
        };
        assert!((tally.pass_rate() - 50.0).abs() < 1e-9);
        assert!((Tally::default().pass_rate() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn families_count_their_classes() {
        let (ported, classes) = ported_classes(Family::Building);
        assert!(ported >= 3);
        assert!(classes >= ported);
    }
}
