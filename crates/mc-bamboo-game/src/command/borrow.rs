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

use mc_bamboo_command::{Coordinates, EntitySelector, ParsedCommand, Value, permission};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::director::RegionId;

use crate::{
    command::{class, commands, select, source},
    inbound::PlayerId,
    island::{Borrow, BorrowClass, Islands},
};

#[cfg(test)]
use crate::island::BorrowBreak;

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

/// 一个候选：它所在的岛、它是谁、它此刻的位置。
///
/// One candidate: the island it lives on, who it is, and its current position.
struct Candidate {
    island: RegionId,
    target: select::Target,
    position: Vec3,
}

/// 一次规划的可变状态。
struct Planner<'a> {
    islands: &'a Islands,
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
    let root = command.split_whitespace().next().unwrap_or("");
    // `into` 一定停放着（混合驱动下玩家自己的岛不在时请求排队），只查停放着的岛，
    // 免得 `iter()` / `get()` 碰到不在主控手里的槽位。
    // `into` is always parked (a request waits when its own island is away);
    // look only at parked islands so no away slot is dereferenced.
    let Some(home) = islands.parked().find(|island| island.id == into) else {
        return Borrow::World(BorrowClass::Other);
    };
    let source = source::of(&home.region, id);
    let level = source::level(&home.region, id);
    // 权限门（4.5 原文）：不够的解析一定失败，不能让没有权限的玩家随手让全服顿一下。
    // Permission gate (plan 4.5): such a line cannot parse, and a player
    // without permission must not stall the whole server at will.
    if !crate::command::may_use_root(command, level) {
        return Borrow::All;
    }
    // 只碰玩家的命令和解析失败的命令都不借岛：它们本来就看全部玩家，
    // 混合驱动下也照旧走 `gather` 的延后投递。
    // Player-only commands and failed parses do not borrow islands: they
    // already see every player and keep the hybrid drive's deferred delivery.
    if class::scope_of(command) == class::CommandScope::Players {
        return Borrow::All;
    }
    let parsed = match commands().graph.parse(command, level) {
        Ok(parsed) => parsed,
        Err(_) => return Borrow::All,
    };
    // 命令类别只用来给剖析指标分组。
    // The command class only groups the profiler samples.
    let class = class_of(root);
    if !islands.all_parked() {
        // 规划要读全部岛的实体和区块；有岛不在主控手里就全拼兜底（和改前一样）。
        // Planning reads every island's entities and chunks; with an island
        // away fall back to a full join, as before.
        return Borrow::World(class);
    }
    let mut planner = Planner {
        islands,
        level,
        set: BTreeSet::from([into]),
        fallback: false,
    };
    planner.command(&parsed, root, source, 0);
    if planner.fallback {
        return Borrow::World(class);
    }
    if planner.set.iter().any(|&id| !islands.contains(id)) {
        return Borrow::World(class);
    }
    #[cfg(test)]
    let mut set = planner.set;
    #[cfg(test)]
    {
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
    Borrow::Islands(set, class)
}

/// 剖析样本按顶层字面量归类（`/execute` 不论链尾是什么都算一类）。
///
/// Profiler sample class by root literal (`/execute` is one class whatever
/// its tail).
fn class_of(root: &str) -> BorrowClass {
    match root {
        "gamemode" => BorrowClass::Gamemode,
        "kill" | "damage" => BorrowClass::Hurt,
        "effect" => BorrowClass::Effect,
        "execute" => BorrowClass::Execute,
        _ => BorrowClass::Other,
    }
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
            "effect" => self.effect(parsed, source),
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

    /// 规划期权限门：选择器语法要求 GM 等级；等级不够时执行期这一步就报错，
    /// 链尾不会跑，也就不用借。
    ///
    /// Planning-time permission gate: selector syntax needs gamemasters; a
    /// lower level makes execution fail at this step, so nothing is borrowed.
    fn selector_denied(&self, selector: &EntitySelector) -> bool {
        selector.uses_selectors && self.level < permission::GAMEMASTERS
    }

    /// 逐岛只读求候选（计划 4.5.1 第 3 节）：和执行期共用 `select::candidates`，
    /// 不排序、不截断、不取随机数。`source` 是这一步的执行上下文（`/execute`
    /// 的分叉），`@s` 和距离都按它算。
    ///
    /// Read-only candidates island by island (plan 4.5.1, section 3), sharing
    /// `select::candidates` with execution. `source` is this step's context
    /// (an `/execute` branch); `@s` and distances follow it.
    fn candidates(&self, source: source::Source, selector: &EntitySelector) -> Vec<Candidate> {
        if self.selector_denied(selector) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for island in self.islands.iter() {
            for target in select::candidates(&island.region, source, selector, source.position) {
                let position = match target {
                    select::Target::Player(id) => island
                        .region
                        .players
                        .get(&id)
                        .map(|player| player.position()),
                    select::Target::World(uid) => island
                        .region
                        .entities
                        .public_at(uid)
                        .map(|public| public.position),
                };
                let Some(position) = position else {
                    continue;
                };
                // 改坏确认：候选筛查漏掉目标玩家（来源自己除外）。
                // Break check: drop the target players (except the source itself).
                #[cfg(test)]
                if self.islands.borrow_break == BorrowBreak::MissCandidatePlayer
                    && matches!(target, select::Target::Player(_))
                    && Some(target) != source.entity
                {
                    continue;
                }
                out.push(Candidate {
                    island: island.id,
                    target,
                    position,
                });
            }
        }
        out
    }

    /// 求候选并把非玩家候选所在的岛并进 `R`：实体表在各岛的 `Region` 里，不借就选不到；
    /// 玩家不用，别的岛的玩家本来就借过来了。
    ///
    /// Collect candidates and join the islands of the non-player ones: entity
    /// tables live in each island's `Region` and are invisible otherwise;
    /// players are lent from every island anyway.
    fn note_candidates(
        &mut self,
        source: source::Source,
        selector: &EntitySelector,
    ) -> Vec<Candidate> {
        let candidates = self.candidates(source, selector);
        for candidate in &candidates {
            if matches!(candidate.target, select::Target::World(_)) {
                self.set.insert(candidate.island);
            }
        }
        candidates
    }

    /// 参数表里所有 `Entity` 值按选择器规则折算。
    fn selectors_in<'a>(
        &mut self,
        values: impl Iterator<Item = &'a Value>,
        source: source::Source,
    ) {
        let selectors: Vec<EntitySelector> = values.filter_map(Value::as_entity).cloned().collect();
        for selector in &selectors {
            self.note_candidates(source, selector);
        }
    }

    /// 一个执行者实体此刻的位置（`/execute as` 可能把来源换成别的岛上的玩家或实体）。
    ///
    /// The current position of a source entity (`/execute as` may have swapped
    /// in a player or an entity on another island).
    fn entity_position(&self, entity: Option<select::Target>) -> Option<Vec3> {
        match entity? {
            select::Target::Player(id) => self.islands.iter().find_map(|island| {
                island
                    .region
                    .players
                    .get(&id)
                    .map(|player| player.position())
            }),
            select::Target::World(uid) => self.islands.iter().find_map(|island| {
                island
                    .region
                    .entities
                    .public_at(uid)
                    .map(|public| public.position)
            }),
        }
    }
    /// 把所有非玩家实体所在的岛并进来（`/tp` 的实体目的地）。
    ///
    /// Join every island holding a non-player entity (`/tp` destinations).
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

    fn teleport(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(parsed.arguments.iter().map(|(_, value)| value), source);
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
        self.selectors_in(parsed.arguments.iter().map(|(_, value)| value), source);
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
        // 目标（`kill` 的 `targets`、`damage` 的 `target`）：候选所在位置的盒。
        // `hurt_mob` 在目标处读方块、掉物品，玩家死亡（`player_death::die`）在他的
        // 位置生成掉落物，`divide` 按位置把新实体分回去，所以候选玩家也要算盒。
        // Targets: a box at each candidate. `hurt_mob` reads blocks and drops
        // loot there and a dying player (`player_death::die`) spawns drops at
        // their position, which `divide` splits by position; players count too.
        let mut targeted = false;
        for name in ["targets", "target"] {
            if let Some(selector) = parsed.get(name).and_then(Value::as_entity) {
                targeted = true;
                let positions: Vec<Vec3> = self
                    .note_candidates(source, selector)
                    .into_iter()
                    .map(|candidate| candidate.position)
                    .collect();
                for position in positions {
                    self.point(BlockPos::containing(position));
                }
            }
        }
        // `damage ... by <entity> [from <cause>]`：只取第一个结果的 ID 和位置填进伤害
        // 来源，不在那里读写方块；非玩家候选的岛要借进来才选得到。
        // `by` / `from` only feed the damage source (id and position) and touch
        // no blocks there; non-player candidates need their islands to be seen.
        for name in ["entity", "cause"] {
            if let Some(selector) = parsed.get(name).and_then(Value::as_entity) {
                self.note_candidates(source, selector);
            }
        }
        if !targeted {
            // 无参 `/kill` 就是来源实体（`/execute as` 可能把它换成别的岛的玩家或实体）。
            // A bare `/kill` uses the source entity, which `/execute as` may have
            // swapped for a player or entity on another island.
            if let Some(position) = self.entity_position(source.entity) {
                self.point(BlockPos::containing(position));
            }
        }
    }

    fn effect(&mut self, parsed: &ParsedCommand, source: source::Source) {
        // 按 `command/effect.rs` 逐条核过：玩家走 `player_effects::give/clear`，只改
        // 这名玩家的效果表、血量上限、吸收值和元数据脏标记，包只发给他自己，瞬间效果
        // 不当场结算（`addEffect`，下一刻 `tickEffects`）；非玩家走
        // `add_mob_effect` / `clear_mob_effects`，在实体表里取出放回，不读方块；反馈
        // 发给发起人和管理员，都是玩家。所以玩家目标一个岛都不借，非玩家候选借它的岛。
        // `clear` 不带目标时是发起人自己。
        // Checked against `command/effect.rs`: players only change their own
        // effect table, health cap, absorption and metadata flag, packets go
        // to that player, instantaneous effects settle next tick; mobs are
        // taken out of and put back into the entity table, no blocks read;
        // feedback goes to players. Player targets borrow nothing, non-player
        // candidates borrow their island.
        #[cfg(test)]
        if self.islands.borrow_break == BorrowBreak::MissEffect {
            return;
        }
        if let Some(selector) = parsed.get("targets").and_then(Value::as_entity) {
            self.note_candidates(source, selector);
        }
    }

    fn enchant(&mut self, parsed: &ParsedCommand, source: source::Source) {
        self.selectors_in(parsed.arguments.iter().map(|(_, value)| value), source);
    }

    fn gamemode(&mut self, parsed: &ParsedCommand, source: source::Source) {
        // `apply_mode` 的 `near_ground` 读目标脚下一格；只对候选玩家算盒。
        // `near_ground` reads the block under each target: feet boxes only.
        if let Some(selector) = parsed.get("target").and_then(Value::as_entity) {
            let feet: Vec<BlockPos> = self
                .candidates(source, selector)
                .into_iter()
                .filter(|candidate| matches!(candidate.target, select::Target::Player(_)))
                .map(|candidate| BlockPos::containing(candidate.position).below())
                .collect();
            for pos in feet {
                self.point(pos);
            }
        } else if let Some(position) = self.entity_position(source.entity) {
            // 没有目标时 `players::gamemode` 改的是发起人自己（在 `into` 上，恒在 `R`
            // 里）；`/execute as` 换过的执行者也算一盒，保守超集。
            // Without a target `players::gamemode` changes the sender, whose
            // island `into` is always in `R`; the `/execute as` executor gets a
            // box too, as a conservative superset.
            self.point(BlockPos::containing(position).below());
        }
    }

    fn data(&mut self, parsed: &ParsedCommand, source: source::Source) {
        for name in ["target", "source"] {
            if let Some(value) = parsed.get(name)
                && let Some(pos) = self.block_at(value, source)
            {
                self.point(pos);
            }
        }
        self.selectors_in(parsed.arguments.iter().map(|(_, value)| value), source);
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
        let steps: Vec<_> = parsed
            .chain
            .iter()
            .take_while(|step| step.kind != "run")
            .collect();
        // 局部坐标（`^ ^ ^`）按执行上下文的位置和朝向解；规划器不模拟朝向，
        // 链上又有一层会换朝向（`at` / `rotated` / `facing`）时就退回全拼。
        // Local coordinates use the context's rotation; the planner does not
        // model rotation, so a rotation-changing step plus local coordinates
        // falls back to a full join.
        let turns = steps
            .iter()
            .any(|step| matches!(step.kind.as_str(), "at" | "rotated" | "facing"));
        let local = has_local_coords(&tail)
            || steps
                .iter()
                .any(|step| step.arguments.iter().any(|(_, value)| is_local(value)));
        if turns && local {
            self.fallback = true;
            return;
        }
        // `store ... block|entity|storage` 的目标在全部分叉跑完以后才写（`apply_store`），
        // 用的是位置 (0, 0, 0)、没有执行者的零上下文，和分叉无关：按零上下文另算一遍。
        // The `store ... block|entity|storage` target is written after every
        // branch (`apply_store`) from a zero context at (0, 0, 0) with no
        // executor, independent of the branches: plan it from that context too.
        if steps
            .iter()
            .any(|step| matches!(step.kind.as_str(), "result" | "success"))
        {
            let zero = source::Source {
                position: Vec3::default(),
                yaw: 0.0,
                pitch: 0.0,
                entity: None,
            };
            for step in steps
                .iter()
                .filter(|step| matches!(step.kind.as_str(), "block" | "entity" | "storage"))
            {
                for (_, value) in &step.arguments {
                    if let Some(pos) = self.block_at(value, zero) {
                        self.point(pos);
                    } else if let Some(selector) = value.as_entity() {
                        self.note_candidates(zero, selector);
                    }
                }
            }
        }
        // 按 `apply_step` 的语义只读模拟链：`as` 换执行者，`at` / `positioned as`
        // 换位置，`positioned <pos>` / `align` 改位置，分叉逐个候选展开。条件为假、
        // 链尾没执行时，条件读到的方块 / 实体也必须借（4.5.1）。
        // Simulate the chain read-only like `apply_step`: `as` swaps the
        // executor, `at` / `positioned as` move to each candidate, `positioned
        // <pos>` / `align` move the position, and each candidate becomes a
        // branch. Conditions are planned even when they turn out false.
        let mut branches = vec![base];
        for step in steps {
            if !known_step(&step.kind) {
                self.fallback = true;
                return;
            }
            // 坐标参数按当前分叉折算（条件、`positioned` 的落点、`facing` 的点）。
            // Coordinates resolve against each current branch.
            for (_, value) in &step.arguments {
                if matches!(value, Value::BlockPos(_) | Value::Vec3(_)) {
                    for branch in &branches {
                        if let Some(pos) = self.block_at(value, *branch) {
                            self.point(pos);
                        }
                    }
                }
            }
            let targets = step
                .arguments
                .iter()
                .find(|(name, _)| name == "targets")
                .and_then(|(_, value)| value.as_entity());
            let changes_entity = step.kind == "as";
            let changes_position =
                step.kind == "at" || (step.kind == "positioned" && targets.is_some());
            if !changes_entity && !changes_position {
                // 条件、`store`、`facing entity` 等只读选择器：候选实体所在的岛要借。
                // Conditions, `store`, `facing entity`: join the candidates' islands.
                for branch in &branches {
                    for (_, value) in &step.arguments {
                        if let Some(selector) = value.as_entity() {
                            self.note_candidates(*branch, selector);
                        }
                    }
                }
                match step.kind.as_str() {
                    "positioned" => {
                        // `positioned <pos>`：位置换成按当前分叉解出的落点。
                        // `positioned <pos>` moves each branch to the resolved point.
                        if let Some(value) = parsed.step_value(step, "pos") {
                            for branch in &mut branches {
                                if let Some(position) = self.vec_at(value, *branch) {
                                    branch.position = position;
                                }
                            }
                        }
                    }
                    "align" => {
                        if let Some(axes) = parsed.step_value(step, "axes").and_then(Value::as_str)
                        {
                            for branch in &mut branches {
                                for axis in axes.chars() {
                                    let coordinate = match axis {
                                        'x' => &mut branch.position.x,
                                        'y' => &mut branch.position.y,
                                        _ => &mut branch.position.z,
                                    };
                                    *coordinate = coordinate.floor();
                                }
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let Some(selector) = targets else {
                self.fallback = true;
                return;
            };
            let mut next = Vec::new();
            for branch in &branches {
                for candidate in self.note_candidates(*branch, selector) {
                    if changes_entity {
                        next.push(source::Source {
                            entity: Some(candidate.target),
                            ..*branch
                        });
                    } else {
                        // 只换位置；链尾和后面的条件按新位置自己算盒。
                        // Only the position changes; the tail and later
                        // conditions box their own coordinates from it.
                        next.push(source::Source {
                            position: candidate.position,
                            ..*branch
                        });
                    }
                }
            }
            branches = next;
            if branches.len() > MAX_BRANCHES {
                self.fallback = true;
                return;
            }
            if branches.is_empty() {
                // 选择器没有候选：执行期链尾不会跑，前面的条件已经算过。
                // No candidates: the tail never runs; earlier steps are planned.
                return;
            }
        }
        if class::scope_of(line) == class::CommandScope::World {
            for branch in branches {
                self.command(&tail, tail_root, branch, depth + 1);
            }
        }
    }
}

/// 命令参数里有没有局部坐标（`^ ^ ^`）。
///
/// Whether a command's arguments contain local coordinates (`^ ^ ^`).
fn has_local_coords(parsed: &ParsedCommand) -> bool {
    parsed.arguments.iter().any(|(_, value)| is_local(value))
}

/// 一个参数值是不是局部坐标。
fn is_local(value: &Value) -> bool {
    match value {
        Value::BlockPos(coords) | Value::Vec3(coords) => {
            matches!(coords, Coordinates::Local { .. })
        }
        _ => false,
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
