//! `SculkCatalystBlockEntity` / `CatalystListener` 与活着的 `SculkSpreader`。
//!
//! 官方的催发体每刻跑一次 `listener.getSculkSpreader().updateCursors(level, pos,
//! level.random, true)`；`ENTITY_DIE` 到达时把死者的基础经验转成电荷，并播放一次
//! `bloom`。世界生成用的 `SculkSpreader` 在 `mc-bamboo-worldgen` 里，那里配置不
//! 同、也不合并游标；这里只实现 `createLevelSpreader()` 那一套。
//!
//! The live `SculkCatalystBlockEntity`: one `updateCursors` per tick and an
//! `ENTITY_DIE` listener that turns the dying mob's experience into charge and
//! blooms. World generation has its own spreader with different constants.

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{blocks, registries::particle_type, sounds::sound_event, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    BlockEntity, BlockState,
    archive::{
        Error,
        tag::{self, put},
    },
    game_event::{GameEvent, dispatch::DeliveryMode, registry::ListenerRef},
    state::{BLOOM, DOWN, EAST, NORTH, SOUTH, UP, WEST},
    world::World,
};

/// `createLevelSpreader()` 的四个数：`growthSpawnCost 10`、`noGrowthRadius 4`、
/// `chargeDecayRate 10`、`additionalDecayRate 5`；替换表是 `SCULK_REPLACEABLE`。
const GROWTH_SPAWN_COST: i32 = 10;
const NO_GROWTH_RADIUS: i32 = 4;
const CHARGE_DECAY_RATE: i32 = 10;
const ADDITIONAL_DECAY_RATE: i32 = 5;
/// `SculkSpreader.MAX_CURSORS` / `MAX_CHARGE`。
const MAX_CURSORS: usize = 32;
const MAX_CHARGE: i32 = 1000;
/// `CatalystListener.getListenerRadius()`。
const LISTENER_RADIUS: i32 = 8;

/// `SculkSpreader.ChargeCursor`：`facings == None` 对应官方字段的 `null`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeCursor {
    pub pos: BlockPos,
    pub charge: i32,
    pub update_delay: i32,
    pub decay_delay: i32,
    pub facings: Option<Vec<Direction>>,
}

impl ChargeCursor {
    /// `new ChargeCursor(pos, charge)`：`decayDelay = 1, updateDelay = 0, facings = null`。
    pub fn new(pos: BlockPos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            update_delay: 0,
            decay_delay: 1,
            facings: None,
        }
    }

    /// `isPosUnreasonable(originPos)`：切比雪夫距离超过 1024。
    fn is_pos_unreasonable(&self, origin: BlockPos) -> bool {
        (self.pos.x - origin.x)
            .abs()
            .max((self.pos.y - origin.y).abs())
            .max((self.pos.z - origin.z).abs())
            > 1024
    }

    /// `ChargeCursor.update`：返回这一步之后要不要发 3006 的「耗尽」事件。
    fn update(&mut self, world: &mut World<'_>, origin: BlockPos, force: bool) {
        if self.charge <= 0 || (!force && !world.chunk_ticks_blocks(self.pos.chunk())) {
            return;
        }
        if self.update_delay > 0 {
            self.update_delay -= 1;
            return;
        }
        let mut state = world.block_state(self.pos);
        let mut behaviour = SculkBehaviour::of(state);
        if force
            && attempt_spread_vein(behaviour, world, self.pos, state, self.facings.as_deref())
            && behaviour.can_change_block_state_on_spread()
        {
            // 官方在 `force` 铺完脉络后重读方块状态和 `SculkBehaviour`，后续的
            // `attemptUseCharge` / `onDischarged` / `updateDecayDelay` 都用重读后的值
            // （脉络会被 `onDischarged` 拆掉贴住幽匿块的面）。这里照同一顺序重读。
            // The official re-reads the block state and `SculkBehaviour` after the forced
            // vein spread; `attemptUseCharge` / `onDischarged` / `updateDecayDelay` then
            // use the re-read values (so `onDischarged` prunes faces backed by sculk).
            state = world.block_state(self.pos);
            behaviour = SculkBehaviour::of(state);
        }
        self.charge = attempt_use_charge(behaviour, self, world, origin, force, state);
        if self.charge <= 0 {
            on_discharged(behaviour, world, state, self.pos);
            return;
        }
        let target = valid_movement_pos(world, self.pos, origin);
        on_discharged(behaviour, world, state, self.pos);
        match target {
            Some(pos) => {
                self.pos = pos;
                let fresh = world.block_state(pos);
                if SculkBehaviour::of(fresh) != SculkBehaviour::Default {
                    self.facings = Some(available_faces(fresh));
                }
            }
            None => {
                // 非世界生成时找不到落点也不清零，继续留在原地等下一刻。
                self.facings = None;
            }
        }
        self.decay_delay = behaviour.update_decay_delay(self.decay_delay);
        self.update_delay = behaviour.spread_delay();
    }

    /// `mergeWith`：合并电荷和较小的 `updateDelay`，被合并的游标电荷归零。
    fn merge_with(&mut self, other: &mut Self) {
        self.charge += other.charge;
        other.charge = 0;
        self.update_delay = self.update_delay.min(other.update_delay);
    }
}

/// 方块实现的 `SculkBehaviour`。26.3 只有 `SculkBlock` 和 `SculkVeinBlock`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SculkBehaviour {
    /// `SculkBehaviour.DEFAULT`：非 `SculkBehaviour` 方块走这个匿名对象。
    Default,
    Sculk,
    Vein,
}

impl SculkBehaviour {
    fn of(state: BlockState) -> Self {
        if state.is(blocks::SCULK) {
            Self::Sculk
        } else if state.is(blocks::SCULK_VEIN) {
            Self::Vein
        } else {
            Self::Default
        }
    }

    fn can_change_block_state_on_spread(self) -> bool {
        self != Self::Sculk
    }

    fn update_decay_delay(self, age: i32) -> i32 {
        if self == Self::Default {
            (age - 1).max(0)
        } else {
            1
        }
    }

    /// `getSculkSpreadDelay()`：三个实现都用接口默认的 1。
    fn spread_delay(self) -> i32 {
        1
    }
}

/// `SculkSpreader`：`createLevelSpreader()` 的一份运行时状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SculkSpreader {
    pub cursors: Vec<ChargeCursor>,
}

impl SculkSpreader {
    /// `addCursors(pos, charge)`：一次最多 1000 电荷，超出拆成多个游标，超过 32 个丢弃。
    pub fn add_cursors(&mut self, pos: BlockPos, mut charge: i32) {
        while charge > 0 {
            let current = charge.min(MAX_CHARGE);
            if self.cursors.len() < MAX_CURSORS {
                self.cursors.push(ChargeCursor::new(pos, current));
            }
            charge -= current;
        }
    }

    /// `updateCursors(level, origin, random, force)`：逐游标更新、发 3006、合并，
    /// 最后按位置累计电荷再发一次 3006（带 `log1p` 编码的粒子数据）。
    pub fn update_cursors(&mut self, world: &mut World<'_>, origin: BlockPos, force: bool) {
        if self.cursors.is_empty() {
            return;
        }
        let mut processed: Vec<ChargeCursor> = Vec::new();
        let mut merged: Vec<(BlockPos, usize)> = Vec::new();
        for mut cursor in std::mem::take(&mut self.cursors) {
            if cursor.is_pos_unreasonable(origin) {
                continue;
            }
            cursor.update(world, origin, force);
            if cursor.charge <= 0 {
                world.level_event(3006, cursor.pos, 0);
                continue;
            }
            if let Some((_, index)) = merged.iter().find(|(pos, _)| *pos == cursor.pos) {
                let first = &mut processed[*index];
                if first.charge + cursor.charge <= MAX_CHARGE {
                    first.merge_with(&mut cursor);
                    continue;
                }
            } else {
                merged.push((cursor.pos, processed.len()));
            }
            processed.push(cursor);
        }
        for (pos, index) in merged {
            let total: i32 = processed
                .iter()
                .filter(|cursor| cursor.pos == pos && cursor.charge > 0)
                .map(|cursor| cursor.charge)
                .sum();
            if total <= 0 {
                continue;
            }
            let faces = processed[index]
                .facings
                .as_deref()
                .map(pack_faces)
                .unwrap_or(0);
            let band = ((total as f64).ln_1p() / 2.299_999_952_316_284) as i32 + 1;
            world.level_event(3006, pos, (band << 6) | faces);
        }
        self.cursors = processed;
    }

    /// `SculkSpreader.save`：只有 `cursors` 一个键。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        vec![(
            "cursors".to_owned(),
            Tag::List(write_cursors(&self.cursors)),
        )]
    }

    /// `SculkSpreader.load`：读不进的列表回到空表，最多 32 条。
    pub fn read(tag: &Tag) -> Self {
        Self {
            cursors: read_cursors(tag::field(tag, "cursors")),
        }
    }
}

/// 活着的 `SculkCatalystBlockEntity`。
#[derive(Debug)]
pub struct SculkCatalystEntity {
    pos: BlockPos,
    pub spreader: SculkSpreader,
}

impl SculkCatalystEntity {
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            spreader: SculkSpreader::default(),
        }
    }

    pub fn read(pos: BlockPos, tag: &Tag) -> Self {
        Self {
            pos,
            spreader: SculkSpreader::read(tag),
        }
    }

    /// `CatalystListener.bloom`：`PULSE=true`、8 刻复位、两枚 `sculk_soul` 粒子、一声 bloom。
    fn bloom(&self, world: &mut World<'_>) {
        let state = world.block_state(self.pos);
        if state.has(BLOOM) {
            world.set_block_and_update(self.pos, state.with(BLOOM, true));
        }
        world.schedule_tick(self.pos, state.block(), 8);
        // `ServerLevel.sendParticles` 在方块 oracle 里只按类型 / 坐标 / 数量记账；
        // 这里用 3006 之外的普通粒子效果表达，正式同步层按 `Effect::Particles` 发包。
        world.push_effect(crate::Effect::Particles {
            particle: particle_type::SCULK_SOUL,
            position: Vec3::new(
                f64::from(self.pos.x) + 0.5,
                f64::from(self.pos.y) + 1.15,
                f64::from(self.pos.z) + 0.5,
            ),
            offset: Vec3::new(0.2, 0.0, 0.2),
            speed: 0.0,
            count: 2,
        });
        let pitch = world.random().next_float() * 0.4 + 0.6;
        world.play_block_sound(
            self.pos,
            sound_event("minecraft:block.sculk_catalyst.bloom"),
            2.0,
            pitch,
        );
    }

    /// `CatalystListener.handleGameEvent`：`ENTITY_DIE` 且经验没被消耗时转充能 + bloom。
    fn on_entity_die(
        &mut self,
        world: &mut World<'_>,
        source: Vec3,
        experience: Option<i32>,
    ) -> bool {
        let Some(experience) = experience else {
            return false;
        };
        if experience > 0 {
            let above = BlockPos::containing(Vec3::new(source.x, source.y + 0.5, source.z));
            self.spreader.add_cursors(above, experience);
        }
        self.bloom(world);
        true
    }
}

impl BlockEntity for SculkCatalystEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    /// `SculkCatalystBlockEntity.serverTick`：每刻推进一次电荷游标。
    fn tick(&mut self, world: &mut World) {
        self.spreader.update_cursors(world, self.pos, true);
    }

    fn listener_radius(&self) -> Option<i32> {
        Some(LISTENER_RADIUS)
    }

    fn delivery_mode(&self) -> Option<DeliveryMode> {
        Some(DeliveryMode::ByDistance)
    }

    fn receive_game_event(
        &mut self,
        world: &mut World,
        delivery: &crate::game_event::dispatch::Delivery,
    ) -> bool {
        if delivery.event != GameEvent::EntityDie || delivery.source_entity.is_none() {
            return false;
        }
        self.on_entity_die(world, delivery.source, delivery.experience)
    }

    fn pre_remove_side_effects(&mut self, world: &mut World, pos: BlockPos, _state: BlockState) {
        if let Some(table) = world.level_mut().game_event_listeners_mut() {
            table.unregister(pos.chunk(), pos.y >> 4, ListenerRef::BlockEntity(pos));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, Error> {
        Ok(self.spreader.additional())
    }
}

/// 新建催发体方块实体，供 `on_place`、读档和存档重存共用。
pub fn create_block_entity(pos: BlockPos) -> Box<dyn BlockEntity> {
    Box::new(SculkCatalystEntity::new(pos))
}

/// 重存走「读 → 写」同一条路径，和 `BlockEntity::write_additional` 共用格式。
pub fn additional_from_tag(tag: &Tag) -> Vec<(String, Tag)> {
    SculkSpreader::read(tag).additional()
}

/// `ChargeCursor.CODEC` 的写入：`pos` 必填，其余字段按默认值省略。
fn write_cursors(cursors: &[ChargeCursor]) -> Vec<Tag> {
    cursors
        .iter()
        .map(|cursor| {
            let mut one = Vec::new();
            put(
                &mut one,
                "pos",
                Tag::IntArray(vec![cursor.pos.x, cursor.pos.y, cursor.pos.z]),
            );
            if cursor.charge != 0 {
                put(&mut one, "charge", Tag::Int(cursor.charge));
            }
            if cursor.decay_delay != 1 {
                put(&mut one, "decay_delay", Tag::Int(cursor.decay_delay));
            }
            if cursor.update_delay != 0 {
                put(&mut one, "update_delay", Tag::Int(cursor.update_delay));
            }
            if let Some(faces) = &cursor.facings
                && !faces.is_empty()
            {
                put(
                    &mut one,
                    "facings",
                    Tag::List(
                        faces
                            .iter()
                            .map(|face| Tag::String(face_name(*face).to_owned()))
                            .collect(),
                    ),
                );
            }
            Tag::Compound(one)
        })
        .collect()
}

/// `ChargeCursor.CODEC` 的读入：坏一条就整表回空，不 panic。
fn read_cursors(input: Option<&Tag>) -> Vec<ChargeCursor> {
    let Some(Tag::List(values)) = input else {
        return Vec::new();
    };
    if values.len() > MAX_CURSORS {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Tag::Compound(_) = value else {
            return Vec::new();
        };
        let Some(Tag::IntArray(pos)) = tag::field(value, "pos") else {
            return Vec::new();
        };
        if pos.len() != 3 {
            return Vec::new();
        }
        let charge = tag::int_or(value, "charge", 0);
        let decay = tag::int_or(value, "decay_delay", 1);
        let update = tag::int_or(value, "update_delay", 0);
        if !(0..=MAX_CHARGE).contains(&charge) || !(0..=1).contains(&decay) || update < 0 {
            return Vec::new();
        }
        let facings = match tag::field(value, "facings") {
            None => None,
            Some(Tag::List(names)) => {
                let mut faces = Vec::new();
                for name in names {
                    let Tag::String(text) = name else {
                        return Vec::new();
                    };
                    let Some(face) = Direction::ALL.into_iter().find(|f| face_name(*f) == text)
                    else {
                        return Vec::new();
                    };
                    faces.push(face);
                }
                Some(faces)
            }
            Some(_) => return Vec::new(),
        };
        out.push(ChargeCursor {
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            charge,
            update_delay: update,
            decay_delay: decay,
            facings,
        });
    }
    out
}

fn face_name(face: Direction) -> &'static str {
    match face {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

/// `MultifaceBlock.pack`：每个面一位，按官方 `Direction` 的 2 的幂编码。
fn pack_faces(faces: &[Direction]) -> i32 {
    faces.iter().fold(0, |bits, &face| match face {
        Direction::Down => bits | 1,
        Direction::Up => bits | 2,
        Direction::North => bits | 4,
        Direction::South => bits | 8,
        Direction::West => bits | 16,
        Direction::East => bits | 32,
    })
}

/// `MultifaceBlock.availableFaces`：非 multiface 方块返回空集合。
fn available_faces(state: BlockState) -> Vec<Direction> {
    Direction::ALL
        .into_iter()
        .filter(|&face| has_face(state, face))
        .collect()
}

fn face_property(face: Direction) -> crate::state::Property<bool> {
    match face {
        Direction::Down => DOWN,
        Direction::Up => UP,
        Direction::North => NORTH,
        Direction::South => SOUTH,
        Direction::West => WEST,
        Direction::East => EAST,
    }
}

fn has_face(state: BlockState, face: Direction) -> bool {
    state.try_get(face_property(face)).unwrap_or(false)
}

/// `SculkBehaviour.attemptSpreadVein`：`SculkBlock` / `SculkVeinBlock` 用接口默认，
/// `DEFAULT` 对象按 `facings` 分三支。
fn attempt_spread_vein(
    behaviour: SculkBehaviour,
    world: &mut World<'_>,
    pos: BlockPos,
    state: BlockState,
    facings: Option<&[Direction]>,
) -> bool {
    match behaviour {
        SculkBehaviour::Sculk | SculkBehaviour::Vein => spread_all(world, pos, state) > 0,
        SculkBehaviour::Default => match facings {
            None => spread_all_with(world, pos, world.block_state(pos), true) > 0,
            Some(faces) if !faces.is_empty() => {
                if !state.is_air() && !world.fluid_state(pos).is_water() {
                    return false;
                }
                regrow(world, pos, state, faces)
            }
            Some(_) => spread_all(world, pos, state) > 0,
        },
    }
}

/// `SculkBehaviour.DEFAULT.attemptUseCharge` 与两个方块实现的合并版本。
fn attempt_use_charge(
    behaviour: SculkBehaviour,
    cursor: &ChargeCursor,
    world: &mut World<'_>,
    origin: BlockPos,
    force: bool,
    state: BlockState,
) -> i32 {
    match behaviour {
        SculkBehaviour::Default => {
            if cursor.decay_delay > 0 {
                cursor.charge
            } else {
                0
            }
        }
        SculkBehaviour::Sculk => sculk_use_charge(cursor, world, origin),
        SculkBehaviour::Vein => {
            if force && attempt_place_sculk(world, cursor.pos, state) {
                return cursor.charge - 1;
            }
            let roll = world.random().next_int(CHARGE_DECAY_RATE);
            if roll == 0 {
                (cursor.charge as f32 * 0.5).floor() as i32
            } else {
                cursor.charge
            }
        }
    }
}

/// `SculkBlock.attemptUseCharge`。
fn sculk_use_charge(cursor: &ChargeCursor, world: &mut World<'_>, origin: BlockPos) -> i32 {
    let charge = cursor.charge;
    if charge == 0 || world.random().next_int(CHARGE_DECAY_RATE) != 0 {
        return charge;
    }
    let pos = cursor.pos;
    let close = closer_than(pos, origin, NO_GROWTH_RADIUS);
    if !close && can_place_growth(world, pos) {
        let roll = world.random().next_int(GROWTH_SPAWN_COST);
        if roll < charge {
            let above = pos.above();
            let state = random_growth_state(world, above);
            world.set_block_and_update(above, state);
            world.play_block_sound(above, sound_event("minecraft:block.sculk.spread"), 1.0, 1.0);
        }
        return (charge - GROWTH_SPAWN_COST).max(0);
    }
    if world.random().next_int(ADDITIONAL_DECAY_RATE) != 0 {
        return charge;
    }
    let penalty = if close {
        1
    } else {
        decay_penalty(pos, origin, charge)
    };
    charge - penalty
}

/// `BlockPos.closerThan(pos, distance)`：`distSqr < distance * distance`。
fn closer_than(a: BlockPos, b: BlockPos, distance: i32) -> bool {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    let dz = f64::from(a.z - b.z);
    dx * dx + dy * dy + dz * dz < f64::from(distance) * f64::from(distance)
}

/// `SculkBlock.getDecayPenalty`。
fn decay_penalty(pos: BlockPos, origin: BlockPos, charge: i32) -> i32 {
    let dx = f64::from(pos.x - origin.x);
    let dy = f64::from(pos.y - origin.y);
    let dz = f64::from(pos.z - origin.z);
    let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
    let offset = dist - NO_GROWTH_RADIUS as f32;
    let outer = offset * offset;
    let max_reach = ((24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS)) as f32;
    let factor = (outer / max_reach).min(1.0);
    (((charge as f32) * factor * 0.5) as i32).max(1)
}

/// `SculkBlock.canPlaceGrowth`：上方是空气 / 水，且 9×3×9 盒子里抑制物少于三个。
fn can_place_growth(world: &World<'_>, pos: BlockPos) -> bool {
    let above = world.block_state(pos.above());
    if !(above.is_air() || world.fluid_state(pos.above()).is_water()) {
        return false;
    }
    let mut count = 0;
    for x in pos.x - 4..=pos.x + 4 {
        for z in pos.z - 4..=pos.z + 4 {
            for y in pos.y..=pos.y + 2 {
                if world
                    .block_state(BlockPos::new(x, y, z))
                    .is_tagged(block_tags::SCULK_GROWTH_INHIBITORS)
                {
                    count += 1;
                    if count >= 3 {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `SculkBlock.getRandomGrowthState`：1/11 尖啸体，其余感测体。
fn random_growth_state(world: &World<'_>, pos: BlockPos) -> BlockState {
    let mut state = if world.random().next_int(11) == 0 {
        BlockState(blocks::SCULK_SHRIEKER)
    } else {
        BlockState(blocks::SCULK_SENSOR)
    };
    state = state.with(BLOOM, false);
    if world.fluid_state(pos).is_water() {
        state = state.try_with_text("waterlogged", "true").unwrap_or(state);
    }
    state
}

/// `SculkVeinBlock.attemptPlaceSculk`：洗好的六方向找一个能贴的 `sculk_replaceable`
/// 支撑格，换成幽匿方块并铺一轮脉络。
fn attempt_place_sculk(world: &mut World<'_>, pos: BlockPos, state: BlockState) -> bool {
    let directions = all_shuffled(world.random());
    for support in directions {
        if !has_face(state, support) {
            continue;
        }
        let support_pos = pos.relative(support);
        if !world
            .block_state(support_pos)
            .is_tagged(block_tags::SCULK_REPLACEABLE)
        {
            continue;
        }
        let sculk = BlockState(blocks::SCULK);
        world.set_block_and_update(support_pos, sculk);
        world.play_block_sound(
            support_pos,
            sound_event("minecraft:block.sculk.spread"),
            1.0,
            1.0,
        );
        let _ = spread_all(world, support_pos, sculk);
        let skip = support.opposite();
        for direction in Direction::ALL {
            if direction == skip {
                continue;
            }
            let neighbor = support_pos.relative(direction);
            let vein = world.block_state(neighbor);
            if vein.is(blocks::SCULK_VEIN) {
                discharged_vein(world, neighbor, vein);
            }
        }
        return true;
    }
    false
}

/// `SculkBehaviour.onDischarged`：只有 `SculkVeinBlock` 覆写了它。
fn on_discharged(
    behaviour: SculkBehaviour,
    world: &mut World<'_>,
    state: BlockState,
    pos: BlockPos,
) {
    if behaviour == SculkBehaviour::Vein {
        discharged_vein(world, pos, state);
    }
}

fn discharged_vein(world: &mut World<'_>, pos: BlockPos, state: BlockState) {
    if !state.is(blocks::SCULK_VEIN) {
        return;
    }
    let mut next = state;
    for face in Direction::ALL {
        if !has_face(next, face) {
            continue;
        }
        if world.block_state(pos.relative(face)).is(blocks::SCULK) {
            next = next.with(face_property(face), false);
        }
    }
    if !Direction::ALL.into_iter().any(|face| has_face(next, face)) {
        next = if world.fluid_state(pos).is_water() {
            BlockState::WATER
        } else {
            BlockState::AIR
        };
    }
    world.set_block_and_update(pos, next);
}

/// `ChargeCursor.getValidMovementPos`：洗好的 18 个非角邻居里找第一个
/// 「是 `SculkBehaviour` 且路径不被挡住」的格子，遇到有基质的提前停。
fn valid_movement_pos(world: &World<'_>, pos: BlockPos, origin: BlockPos) -> Option<BlockPos> {
    let mut result = None;
    for neighbour in non_corner_neighbours(world.random()) {
        let candidate = pos.offset(neighbour.0, neighbour.1, neighbour.2);
        let dx = origin.x - candidate.x;
        let dz = origin.z - candidate.z;
        if dx * dx + dz * dz > 144 {
            continue;
        }
        let state = world.block_state(candidate);
        if SculkBehaviour::of(state) == SculkBehaviour::Default
            || !is_movement_unobstructed(world, pos, candidate)
        {
            continue;
        }
        result = Some(candidate);
        if has_substrate_access(world, state, candidate) {
            break;
        }
    }
    result
}

/// `ChargeCursor.NON_CORNER_NEIGHBOURS`：x 内层、y 中层、z 外层的 18 项，再洗牌。
fn non_corner_neighbours(random: &crate::random::LegacyRandom) -> Vec<(i32, i32, i32)> {
    let mut list = Vec::with_capacity(18);
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                if (x == 0 || y == 0 || z == 0) && !(x == 0 && y == 0 && z == 0) {
                    list.push((x, y, z));
                }
            }
        }
    }
    shuffle(&mut list, random);
    list
}

fn all_shuffled(random: &crate::random::LegacyRandom) -> [Direction; 6] {
    let mut list = Direction::ALL;
    shuffle(&mut list, random);
    list
}

/// `Util.shuffledCopy`：Fisher-Yates，`i` 从 size 递减到 2，每次 `nextInt(i)`。
fn shuffle<T>(list: &mut [T], random: &crate::random::LegacyRandom) {
    for i in (2..=list.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        list.swap(i - 1, swap_to);
    }
}

/// `ChargeCursor.isMovementUnobstructed`。
fn is_movement_unobstructed(world: &World<'_>, from: BlockPos, to: BlockPos) -> bool {
    let delta = (to.x - from.x, to.y - from.y, to.z - from.z);
    if delta.0.abs() + delta.1.abs() + delta.2.abs() == 1 {
        return true;
    }
    let x = if delta.0 < 0 {
        Direction::West
    } else {
        Direction::East
    };
    let y = if delta.1 < 0 {
        Direction::Down
    } else {
        Direction::Up
    };
    let z = if delta.2 < 0 {
        Direction::North
    } else {
        Direction::South
    };
    if delta.0 == 0 {
        is_unobstructed(world, from, y) || is_unobstructed(world, from, z)
    } else if delta.1 == 0 {
        is_unobstructed(world, from, x) || is_unobstructed(world, from, z)
    } else {
        is_unobstructed(world, from, x) || is_unobstructed(world, from, y)
    }
}

fn is_unobstructed(world: &World<'_>, from: BlockPos, direction: Direction) -> bool {
    let pos = from.relative(direction);
    !world
        .block_state(pos)
        .is_face_sturdy(direction.opposite(), crate::state::SupportType::Full)
}

/// `SculkVeinBlock.hasSubstrateAccess`：某个打开的面旁边是 `sculk_replaceable`。
fn has_substrate_access(world: &World<'_>, state: BlockState, pos: BlockPos) -> bool {
    if !state.is(blocks::SCULK_VEIN) {
        return false;
    }
    Direction::ALL.into_iter().any(|face| {
        has_face(state, face)
            && world
                .block_state(pos.relative(face))
                .is_tagged(block_tags::SCULK_REPLACEABLE)
    })
}

/// `SculkVeinBlock.regrow`：按保留的面重新放一层脉络。
fn regrow(world: &mut World<'_>, pos: BlockPos, state: BlockState, faces: &[Direction]) -> bool {
    let mut vein = BlockState(blocks::SCULK_VEIN);
    let mut changed = false;
    for face in faces {
        if world
            .block_state(pos.relative(*face))
            .is_face_sturdy(face.opposite(), crate::state::SupportType::Full)
        {
            vein = vein.with(face_property(*face), true);
            changed = true;
        }
    }
    if !changed {
        return false;
    }
    if !world.fluid_state(pos).is_empty() {
        vein = vein.try_with_text("waterlogged", "true").unwrap_or(vein);
    }
    let _ = state;
    world.set_block_and_update(pos, vein);
    true
}

/// `MultifaceSpreader.spreadAll` 的已建模部分。世界生成版在 `mc-bamboo-worldgen`。
/// 这里按官方 `SculkVeinSpreaderConfig` 的顺序铺同格 / 同平面 / 绕角脉络，返回铺下
/// 的位置数。随机数不参与。
fn spread_all(world: &mut World<'_>, pos: BlockPos, state: BlockState) -> i64 {
    spread_all_with(world, pos, state, false)
}

fn spread_all_with(
    world: &mut World<'_>,
    pos: BlockPos,
    state: BlockState,
    same_position_only: bool,
) -> i64 {
    let other_valid = !state.is(blocks::SCULK_VEIN);
    let mut total = 0_i64;
    for from_face in Direction::ALL {
        if !(other_valid || has_face(state, from_face)) {
            continue;
        }
        for spread in Direction::ALL {
            if spread.axis() == from_face.axis() {
                continue;
            }
            if !(other_valid || (has_face(state, from_face) && !has_face(state, spread))) {
                continue;
            }
            let kinds: &[SpreadKind] = if same_position_only {
                &[SpreadKind::SamePosition]
            } else {
                &[
                    SpreadKind::SamePosition,
                    SpreadKind::SamePlane,
                    SpreadKind::WrapAround,
                ]
            };
            for kind in kinds {
                let (target, face) = kind.spread_pos(pos, spread, from_face);
                if place_vein_face(world, target, face) {
                    total += 1;
                }
            }
        }
    }
    total
}

#[derive(Clone, Copy)]
enum SpreadKind {
    SamePosition,
    SamePlane,
    WrapAround,
}

impl SpreadKind {
    fn spread_pos(
        self,
        pos: BlockPos,
        spread: Direction,
        from_face: Direction,
    ) -> (BlockPos, Direction) {
        match self {
            Self::SamePosition => (pos, spread),
            Self::SamePlane => (pos.relative(spread), from_face),
            Self::WrapAround => (pos.relative(spread).relative(from_face), spread.opposite()),
        }
    }
}

/// `MultifaceSpreader.SpreadPos.place`：目标格是空气 / 水或已有脉络，且贴得住。
/// 官方 `SculkVeinSpreaderConfig.stateCanBeReplaced` 额外拒绝把脉络贴在幽匿块 /
/// 幽匿催发体 / 移动中的活塞背面，这里照做。
fn place_vein_face(world: &mut World<'_>, pos: BlockPos, face: Direction) -> bool {
    let state = world.block_state(pos);
    if !state.is(blocks::SCULK_VEIN) && !state.is_air() && !world.fluid_state(pos).is_water() {
        return false;
    }
    let support = world.block_state(pos.relative(face));
    if support.is(blocks::SCULK)
        || support.is(blocks::SCULK_CATALYST)
        || support.is(blocks::MOVING_PISTON)
    {
        return false;
    }
    if !support.is_face_sturdy(face.opposite(), crate::state::SupportType::Full) {
        return false;
    }
    let mut next = if state.is(blocks::SCULK_VEIN) {
        state
    } else {
        BlockState(blocks::SCULK_VEIN)
    };
    if next.try_get(face_property(face)) == Some(true) {
        return false;
    }
    next = next.with(face_property(face), true);
    if world.fluid_state(pos).is_water() {
        next = next.try_with_text("waterlogged", "true").unwrap_or(next);
    }
    world.set_block_and_update(pos, next);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_split_at_a_thousand_charge() {
        let mut spreader = SculkSpreader::default();
        spreader.add_cursors(BlockPos::new(1, 64, 2), 2500);
        assert_eq!(spreader.cursors.len(), 3);
        assert_eq!(
            spreader
                .cursors
                .iter()
                .map(|c| c.charge)
                .collect::<Vec<_>>(),
            [1000, 1000, 500]
        );
    }

    #[test]
    fn codec_round_trip_keeps_every_cursor_field() {
        let mut spreader = SculkSpreader::default();
        spreader.cursors.push(ChargeCursor {
            pos: BlockPos::new(-3, 70, 8),
            charge: 42,
            update_delay: 1,
            decay_delay: 0,
            facings: Some(vec![Direction::Up, Direction::North]),
        });
        let tag = Tag::Compound(spreader.additional());
        assert_eq!(SculkSpreader::read(&tag).cursors, spreader.cursors);
    }
}
