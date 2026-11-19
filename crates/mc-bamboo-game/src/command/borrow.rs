//! 一条命令按需借哪几个岛（岛的实施计划 4.5.1）。
//!
//! 规划只读：解析命令行，按命令的参数算出「可能读、写、创建、搬动的任何东西所在的岛」
//! 的集合 `R`。`R` 外的岛只借玩家，所以 `R` 必须是保守超集；算不出来就退回全拼。
//!
//! On-demand island borrowing for a command (island plan 4.5.1). Planning is
//! read-only: parse the line, work out the set `R` of islands that may be read,
//! written, created in or moved into. Islands outside `R` lend only their
//! players, so `R` must be a conservative superset; anything unplannable falls
//! back to joining every island.

use std::collections::BTreeSet;

use mc_bamboo_command::{EntitySelector, ParsedCommand, Value, permission};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::director::RegionId;

use crate::{
    command::{class, commands, select, source},
    inbound::PlayerId,
    island::{Borrow, Islands},
};

/// 方块盒向外扩这么多格：即时方块逻辑每一跳最多 2 格，岛之间隔着 2 个未加载
/// 区块（32 格），所以 2 个区块的外扩既盖住更新链又碰不到别的岛（计划 4.5.1）。
///
/// Grow a block box by this many blocks: each immediate block-logic hop reaches
/// at most 2 blocks, and islands are separated by 2 unloaded chunks (32
/// blocks), so two chunks of margin cover the chain without reaching another
/// island (plan 4.5.1).
const BOX_MARGIN: i32 = 32;

/// 一个盒里最多枚举多少个区块位置；超过就全拼兜底，别让规划变成全表扫描。
///
/// Cap on the chunk positions a box may enumerate; past it fall back to
/// joining every island instead of scanning the world in the planner.
const MAX_BOX_CHUNKS: i64 = 4096;

/// `/execute` 模拟器最多容许多少个分叉来源。
///
/// Cap on the branch sources the `/execute` planner may simulate.
const MAX_BRANCHES: usize = 256;

/// 一次规划的可变状态。
struct Planner<'a> {
    islands: &'a Islands,
    into: RegionId,
    /// 来源（玩家或控制台）。
    source: source::Source,
    /// 来源的权限等级。
    level: u8,
    /// 借进来的岛（恒含发起人的岛）。
    set: BTreeSet<RegionId>,
    /// 算不出来的形态退全拼。
    fallback: bool,
}

/// 这条命令要借多少（计划 4.5.1）。`into` 是发起人的岛，控制台是家岛。
///
/// How much this command borrows (plan 4.5.1). `into` is the sender's island,
/// the home island for the console.
pub(crate) fn plan_borrow(islands: &Islands, into: RegionId, id: PlayerId, line: &str) -> Borrow {
    if !islands.several() {
        return Borrow::All;
    }
    let command = line.strip_prefix('/').unwrap_or(line);
    let Some(home) = islands.get(into) else {
        return Borrow::World;
    };
    let source = source::of(&home.region, id);
    let level = source::level(&home.region, id);
    let parsed = match commands().graph.parse(command, level) {
        Ok(parsed) => parsed,
        // 解析失败只发一条反馈，和方块无关。
        // A failed parse only sends feedback and touches no block.
        Err(_) => return Borrow::All,
    };
    // 权限门（4.5 原文）：不够的解析一定失败，不能让没有权限的玩家随手让全服顿一下。
    // Permission gate (plan 4.5): such a line cannot parse, and a player
    // without permission must not stall the whole server at will.
    if !crate::command::may_use_root(command, level) {
        return Borrow::All;
    }
    if class::scope_of(command) == class::CommandScope::Players {
        return Borrow::All;
    }
    if !islands.all_parked() {
        // 混合驱动下有岛不在主控手里：读不到它的实体/区块，全拼兜底。
        // An island is away: its entities and chunks cannot be read here.
        return Borrow::World;
    }
    let mut planner = Planner {
        islands,
        into,
        source,
        level,
        set: BTreeSet::from([into]),
        fallback: false,
    };
    let root = command.split_whitespace().next().unwrap_or("");
    planner.command(&parsed, root, source, 0);
    if planner.fallback {
        return Borrow::World;
    }
    if planner.set.iter().any(|&id| !islands.contains(id)) {
        return Borrow::World;
    }
    #[cfg(test)]
    let mut set = planner.set;
    #[cfg(test)]
    {
        use crate::island::BorrowBreak;
        match islands.borrow_break {
            BorrowBreak::MissIsland | BorrowBreak::MissSelector | BorrowBreak::MissDestination => {
                // 故意漏借一个非发起人的岛（改坏确认）。
                let victim = set.iter().copied().find(|&id| id != into);
                if let Some(id) = victim {
                    set.remove(&id);
                }
            }
            _ => {}
        }
    }
    #[cfg(not(test))]
    let set = planner.set;
    Borrow::Islands(set)
}

impl Planner<'_> {
    /// 按顶层字面量规划；`depth` 防 `/execute` 链尾递归过深。
    fn command(
        &mut self,
        parsed: &ParsedCommand,
        root: &str,
        source: source::Source,
        depth: usize,
    ) {
        match root {
            "tp" | "teleport" => self.teleport(parsed, source),
            "setblock" | "fill" | "clone" => self.blocks(parsed, root, source),
            "summon" => self.summon(parsed, source),
            "loot" => self.loot(parsed, source),
            "kill" | "damage" => self.hurt(parsed, source),
            "enchant" => self.enchant(parsed, source),
            "gamemode" => self.gamemode(parsed, source),
            "defaultgamemode" => {}
            "data" => self.data(parsed, source),
            "execute" => self.execute(parsed, source, depth),
            _ => self.fallback = true,
        }
    }

    /// 把一个方块盒折算成岛：盒里已加载的区块按归属加进 `set`。
    fn box_islands(&mut self, from: BlockPos, to: BlockPos) {
        let min = BlockPos::new(from.x.min(to.x), from.y.min(to.y), from.z.min(to.z));
        let max = BlockPos::new(from.x.max(to.x), from.y.max(to.y), from.z.max(to.z));
        let lo = ChunkPos::new(
            min.x.saturating_sub(BOX_MARGIN).div_euclid(16),
            min.z.saturating_sub(BOX_MARGIN).div_euclid(16),
        );
        let hi = ChunkPos::new(
            max.x.saturating_add(BOX_MARGIN).div_euclid(16),
            max.z.saturating_add(BOX_MARGIN).div_euclid(16),
        );
        let span_x = i64::from(hi.x) - i64::from(lo.x) + 1;
        let span_z = i64::from(hi.z) - i64::from(lo.z) + 1;
        if span_x <= 0 || span_z <= 0 || span_x.saturating_mul(span_z) > MAX_BOX_CHUNKS {
            self.fallback = true;
            return;
        }
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                if let Some(id) = self.islands.island_of_chunk(ChunkPos::new(x, z)) {
                    self.set.insert(id);
                }
            }
        }
    }

    fn point(&mut self, pos: BlockPos) {
        self.box_islands(pos, pos);
    }

    /// 一串坐标参数按来源解成方块位置；`BlockPos` 和 `Vec3` 都认。
    fn block_at(&self, value: &Value, source: source::Source) -> Option<BlockPos> {
        let coordinates = value.as_block_pos().or_else(|| value.as_vec3())?;
        Some(BlockPos::containing(source::position(coordinates, source)))
    }

    /// 一串 `Vec3` 参数按来源解成世界坐标。
    fn vec_at(&self, value: &Value, source: source::Source) -> Option<Vec3> {
        let coordinates = value.as_vec3()?;
        Some(source::position(coordinates, source))
    }

    /// 选择器候选所在的岛（计划 4.5.1 第 3 节）：逐岛只读筛「有没有非玩家实体
    /// 过全部过滤条件」，把有候选的岛并进 `R`。规划期不排序、不截断、不取随机数。
    fn selector_islands(&mut self, selector: &EntitySelector, origin: Vec3) {
        if !select::may_include_non_players(selector) {
            return;
        }
        if self.level < permission::GAMEMASTERS && selector.uses_selectors {
            self.fallback = true;
            return;
        }
        for island in self.islands.iter() {
            if island.id == self.into {
                continue;
            }
            if select::has_non_player_candidate(&island.region, self.source, selector, origin) {
                self.set.insert(island.id);
            }
        }
    }

    /// 参数表里所有 `Entity` 值按选择器规则折算。
    fn selectors_in<'a>(&mut self, values: impl Iterator<Item = &'a Value>, origin: Vec3) {
        let selectors: Vec<EntitySelector> = values.filter_map(Value::as_entity).cloned().collect();
        for selector in &selectors {
            self.selector_islands(selector, origin);
        }
    }

    /// 把所有非玩家实体所在的岛并进来（`/summon` 的显式 UUID 查重、
    /// `/tp` 的实体目的地、`/execute` 的 `as @e` 分叉）。
    fn all_entity_islands(&mut self) {
        for island in self.islands.iter() {
            if island
                .region
                .entities
                .iter_public()
                .any(|public| !public.removed)
            {
                self.set.insert(island.id);
            }
        }
    }

    /// 所有非玩家实体位置的保守盒。
    fn entity_position_boxes(&mut self) {
        let positions: Vec<Vec3> = self
            .islands
            .iter()
            .flat_map(|island| island.region.entities.iter_public())
            .filter(|public| !public.removed)
            .map(|public| public.position)
            .collect();
        for position in positions {
            self.point(BlockPos::containing(position));
        }
    }

    /// 所有玩家脚下那格的保守盒（`gamemode` / `hurt` 会读地面）。
    fn player_feet_boxes(&mut self) {
        let feet: Vec<BlockPos> = self
            .islands
            .iter()
            .flat_map(|island| island.region.players.values())
            .map(|player| BlockPos::containing(player.position()).below())
            .collect();
        for pos in feet {
            self.point(pos);
        }
    }

    fn teleport(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(
            parsed.arguments.iter().map(|(_, value)| value),
            source.position,
        );
        // 目标或目的地含非玩家实体时 `move_entity` 会改实体位置，`divide` 按实体
        // 当前区块分实体：所有实体岛和它们位置的盒都要借进来（保守超集）。
        // With non-player targets/destinations `move_entity` changes entity
        // positions and `divide` splits by the new chunk: join every entity
        // island and a box around each entity (a conservative superset).
        if has_entity_target(parsed) {
            self.all_entity_islands();
            self.entity_position_boxes();
            if let Some(value) = parsed.get("location")
                && let Some(pos) = self.vec_at(value, source)
            {
                self.point(BlockPos::containing(pos));
            }
        }
    }

    fn blocks(&mut self, parsed: &ParsedCommand, root: &str, source: source::Source) {
        let (first, second) = match root {
            "setblock" => ("pos", None),
            "fill" => ("from", Some("to")),
            "clone" => ("begin", Some("end")),
            _ => {
                self.fallback = true;
                return;
            }
        };
        let Some(from) = parsed
            .get(first)
            .and_then(|value| self.block_at(value, source))
        else {
            self.fallback = true;
            return;
        };
        let to = match second {
            Some(name) => {
                let Some(pos) = parsed
                    .get(name)
                    .and_then(|value| self.block_at(value, source))
                else {
                    self.fallback = true;
                    return;
                };
                pos
            }
            None => from,
        };
        self.box_islands(from, to);
        if root == "clone" {
            let Some(destination) = parsed
                .get("destination")
                .and_then(|value| self.block_at(value, source))
            else {
                self.fallback = true;
                return;
            };
            let span = BlockPos::new(to.x - from.x, to.y - from.y, to.z - from.z);
            self.box_islands(destination, destination.offset(span.x, span.y, span.z));
        }
    }

    fn summon(&mut self, parsed: &ParsedCommand, source: source::Source) {
        // NBT 里的 `Pos` 会覆盖命令的 `pos`（`nbt::load_into` 先读 `Pos`）。
        let pos = parsed
            .get("nbt")
            .and_then(Value::as_nbt)
            .and_then(nbt_position)
            .or_else(|| {
                parsed
                    .get("pos")
                    .and_then(|value| self.vec_at(value, source))
            })
            .unwrap_or(source.position);
        self.point(BlockPos::containing(pos));
        // 显式 UUID 撞车：`summon_cow` 末尾 `has_mob_uuid` 扫全表，规划期只读
        // 解出 UUID，逐岛查有没有实体占用，有就把那个岛借进来（计划 4.5.1）。
        #[cfg(test)]
        if self.islands.borrow_break == crate::island::BorrowBreak::MissUuid {
            return;
        }
        let Some(uuid) = parsed
            .get("nbt")
            .and_then(Value::as_nbt)
            .and_then(explicit_uuid)
        else {
            return;
        };
        for island in self.islands.iter() {
            if island.region.entities.has_mob_uuid(&uuid) {
                self.set.insert(island.id);
            }
        }
    }

    fn loot(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(
            parsed.arguments.iter().map(|(_, value)| value),
            source.position,
        );
        for name in ["pos", "targetPos"] {
            if let Some(value) = parsed.get(name) {
                let pos = self
                    .block_at(value, source)
                    .unwrap_or(BlockPos::containing(source.position));
                self.point(pos);
            }
        }
        // `spawn` 没写 `targetPos` 时掉在来源位置；来源岛本来就在 `set` 里。
        // `give` / `replace entity` 的目标都在 `selectors_in` 里。
    }

    fn hurt(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(
            parsed.arguments.iter().map(|(_, value)| value),
            source.position,
        );
        // `hurt_mob` 会按实体位置读方块、掉物品；`hurt_player` 可能读脚下地面。
        self.entity_position_boxes();
        self.player_feet_boxes();
    }

    fn enchant(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(
            parsed.arguments.iter().map(|(_, value)| value),
            source.position,
        );
    }

    fn gamemode(&mut self, _parsed: &ParsedCommand, _source: source::Source) {
        // `apply_mode` 的 `near_ground` 读目标脚下一格；候选全是玩家，逐个算盒。
        self.player_feet_boxes();
    }

    fn data(&mut self, parsed: &ParsedCommand, source: source::Source) {
        for name in ["target", "source"] {
            if let Some(value) = parsed.get(name)
                && let Some(pos) = self.block_at(value, source)
            {
                self.point(pos);
            }
        }
        self.selectors_in(
            parsed.arguments.iter().map(|(_, value)| value),
            source.position,
        );
    }

    /// `/execute`：只读模拟整条链（计划 4.5.1 第 5 节）。链尾在每个分叉上依次
    /// 执行，前一个分叉改过的世界会被后一个分叉看到；`R` 仍是超集，因为新建
    /// 实体只会落在 `R` 里、被搬的实体本来就是候选、目的地也已经加进 `R`。
    fn execute(&mut self, parsed: &ParsedCommand, base: source::Source, depth: usize) {
        if depth > 0 {
            // 链尾还是 `/execute`：不递归，全拼兜底。
            self.fallback = true;
            return;
        }
        let Some(line) = parsed.run_command.as_deref() else {
            self.fallback = true;
            return;
        };
        let tail = match commands().graph.parse(line, self.level) {
            Ok(parsed) => parsed,
            Err(_) => {
                self.fallback = true;
                return;
            }
        };
        let tail_root = line.split_whitespace().next().unwrap_or("");
        if tail_root == "execute" {
            self.fallback = true;
            return;
        }
        // 分叉来源：链上的 `as` / `at` / `facing` 选择器可能把来源放到任何实体或
        // 玩家的位置。保守地把所有实体/玩家位置都当作分叉来源。
        let mut branches = vec![base];
        let entity_selectors = parsed
            .chain
            .iter()
            .flat_map(|step| step.arguments.iter())
            .filter_map(|(_, value)| Value::as_entity(value))
            .any(select::may_include_non_players);
        if entity_selectors {
            self.all_entity_islands();
            self.entity_position_boxes();
            for island in self.islands.iter() {
                for player in island.region.players.values() {
                    branches.push(source::Source {
                        position: player.position(),
                        ..base
                    });
                }
                for public in island.region.entities.iter_public() {
                    if !public.removed {
                        branches.push(source::Source {
                            position: public.position,
                            ..base
                        });
                    }
                }
            }
        }
        if branches.len() > MAX_BRANCHES {
            self.fallback = true;
            return;
        }
        // 链上每一步的坐标 / 选择器参数，按所有分叉来源都折算一遍（保守超集）。
        // 条件为假、链尾没执行时，条件读到的方块/实体也必须借（4.5.1）。
        for step in parsed.chain.iter().take_while(|step| step.kind != "run") {
            if !known_step(&step.kind) {
                self.fallback = true;
                return;
            }
            for (_, value) in &step.arguments {
                if matches!(value, Value::BlockPos(_) | Value::Vec3(_)) {
                    for branch in &branches {
                        if let Some(pos) = self.block_at(value, *branch) {
                            self.point(pos);
                        }
                    }
                } else if let Some(selector) = value.as_entity() {
                    for branch in &branches {
                        self.selector_islands(selector, branch.position);
                    }
                }
            }
        }
        if class::scope_of(line) == class::CommandScope::World {
            for branch in branches {
                self.command(&tail, tail_root, branch, depth + 1);
            }
        }
    }
}

/// 模拟器认识的 `/execute` 链层；不认识的一律全拼兜底。
fn known_step(kind: &str) -> bool {
    matches!(
        kind,
        "as" | "at"
            | "positioned"
            | "rotated"
            | "facing"
            | "align"
            | "anchored"
            | "in"
            | "if"
            | "unless"
            | "store"
            | "result"
            | "success"
            | "score"
            | "bossbar"
            | "matches"
            | "="
            | "<"
            | "<="
            | ">"
            | ">="
            | "block"
            | "entity"
            | "storage"
            | "int"
            | "short"
            | "long"
            | "float"
            | "double"
            | "byte"
            | "data"
    )
}

/// `/tp` 里有没有可能选中非玩家实体的目标（决定要不要借实体岛和目的地盒）。
fn has_entity_target(parsed: &ParsedCommand) -> bool {
    ["targets", "destination", "facingEntity"]
        .iter()
        .any(|name| {
            parsed
                .get(name)
                .and_then(Value::as_entity)
                .is_some_and(select::may_include_non_players)
        })
}

/// `UUIDUtil.CODEC` 的 `IntArray[4]`：和 `entity::mob::nbt::load_into` 同一份编码。
fn explicit_uuid(tag: &Tag) -> Option<[u8; 16]> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    let (_, Tag::IntArray(words)) = entries.iter().find(|(key, _)| key == "UUID")? else {
        return None;
    };
    if words.len() != 4 {
        return None;
    }
    let mut out = [0u8; 16];
    for (index, word) in words.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
    }
    Some(out)
}

/// NBT 里的 `Pos`（三个 double），和 `nbt::load_into` 读到的位置一致。
fn nbt_position(tag: &Tag) -> Option<Vec3> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    let (_, Tag::List(values)) = entries.iter().find(|(key, _)| key == "Pos")? else {
        return None;
    };
    if values.len() != 3 {
        return None;
    }
    let mut axes = [0.0f64; 3];
    for (index, value) in values.iter().enumerate() {
        match value {
            Tag::Double(number) => axes[index] = *number,
            Tag::Float(number) => axes[index] = f64::from(*number),
            Tag::Int(number) => axes[index] = f64::from(*number),
            _ => return None,
        }
        axes[index] = axes[index].clamp(-3.000_051_2e7, 3.000_051_2e7);
    }
    Some(Vec3::new(axes[0], axes[1], axes[2]))
}
