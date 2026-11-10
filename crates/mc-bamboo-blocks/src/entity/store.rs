//! 一个 region 里的全部实体，公开和私有数据分开存。
//!
//! 见 `docs/world-model-design.md` 4.3：并行算的阶段里每个任务只读整个 `public`、
//! 只写自己那一行 `state`，所以两半不能放在同一个结构体里。这里的行槽保持下标稳定，
//! `take` / `put` 都是 O(1)，并行阶段另外把三个平行数组摊开，见 [`EntityStore::snapshot`]。
//!
//! All entities in a region, with public and private halves stored apart. See the world-model
//! design, section 4.3. Row slots keep their indices stable; [`EntityStore::snapshot`] lays
//! out three parallel arrays for the parallel driver.

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::entity_types;
use mc_bamboo_protocol::item::ItemStack;

use crate::{shape::Aabb, state::BlockState, ticks::Busy};

use super::{
    ENTITY_RANDOM_SEED, Entity, EntityKind, FIRST_ENTITY_ID, FallingBlock, GroundItem, JavaRandom,
    Movement, PrimedTnt,
};

/// 发给客户端的实体 ID，也是实体之间互相引用的键。
pub type EntityUid = i32;

/// 空间索引每段有多少格，和区块段一致。
const SECTION_SIZE: i32 = 16;

/// 别人能读到的部分：别的实体的逻辑、方块逻辑、追踪同步会读的字段。
#[derive(Debug, Clone, PartialEq)]
pub struct EntityPublic {
    pub uid: EntityUid,
    pub kind: KindPublic,
    pub position: Vec3,
    pub delta: Vec3,
    pub on_ground: bool,
    pub yaw: f32,
    pub removed: bool,
    pub precise: bool,
}

/// 种类里别人读得到的部分，其余字段在 [`KindState`]。
#[derive(Debug, Clone, PartialEq)]
pub enum KindPublic {
    FallingBlock {
        state: BlockState,
        start_pos: BlockPos,
    },
    Tnt {
        fuse: i32,
    },
    Item {
        stack: ItemStack,
        pickup_delay: i32,
        age: i32,
        health: i32,
        target: Option<[u8; 16]>,
    },
}

impl KindPublic {
    /// 注册表 ID，发 `add_entity` 包时用。
    pub const fn entity_type(&self) -> i32 {
        match self {
            Self::FallingBlock { .. } => mc_bamboo_data::registries::entity_type::FALLING_BLOCK,
            Self::Tnt { .. } => mc_bamboo_data::registries::entity_type::TNT,
            Self::Item { .. } => mc_bamboo_data::registries::entity_type::ITEM,
        }
    }

    /// `EntityDimensions`。
    pub fn dimensions(&self) -> (f32, f32) {
        let facts = &entity_types::FACTS[self.entity_type() as usize];
        (facts.width, facts.height)
    }

    pub const fn is_falling_block(&self) -> bool {
        matches!(self, Self::FallingBlock { .. })
    }
}

/// 只有自己读写、种类特有的部分。
#[derive(Debug, Clone, PartialEq)]
pub enum KindState {
    FallingBlock(FallingBlockState),
    Tnt,
    Item(ItemState),
}

/// `FallingBlockEntity` 里别人不读的字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallingBlockState {
    pub time: i32,
    pub drop_item: bool,
    pub cancel_drop: bool,
    pub hurt_entities: bool,
    pub fall_damage_per_distance: f32,
    pub fall_damage_max: i32,
}

/// `ItemEntity` 里别人不读的字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemState {
    pub bob_offs: f32,
    pub roll_velocity: bool,
}

/// 只有自己读写的部分。
#[derive(Debug, Clone, PartialEq)]
pub struct EntityState {
    pub kind: KindState,
    pub old_position: Vec3,
    pub tick_count: i32,
    pub fall_distance: f64,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    pub on_ground_no_blocks: bool,
    pub main_supporting_block: Option<BlockPos>,
    pub in_water: bool,
    pub first_tick: bool,
    pub remaining_fire_ticks: i32,
    pub stuck_speed_multiplier: Vec3,
    pub movement_this_tick: Vec<Movement>,
    pub final_movements: Vec<Movement>,
    pub random: JavaRandom,
}

/// 一行的两半。取出、放回和命令落地都用这个值。
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRow {
    pub public: EntityPublic,
    pub state: EntityState,
}

impl EntityPublic {
    /// `Entity.getBoundingBox()`。
    pub fn bounding_box(&self) -> Aabb {
        let (width, height) = self.kind.dimensions();
        let half = f64::from(width / 2.0);
        Aabb::new(
            self.position.x - half,
            self.position.y,
            self.position.z - half,
            self.position.x + half,
            self.position.y + f64::from(height),
            self.position.z + half,
        )
    }

    /// `blockPosition()`。
    pub fn block_position(&self) -> BlockPos {
        BlockPos::containing(self.position)
    }

    pub fn chunk_position(&self) -> ChunkPos {
        ChunkPos::containing(self.position.x, self.position.z)
    }
}

impl EntityRow {
    /// 把一行拼成串行阶段用的完整快照。
    pub fn into_entity(self) -> Entity {
        let EntityRow { public, state } = self;
        let kind = match (public.kind, state.kind) {
            (KindPublic::FallingBlock { state, start_pos }, KindState::FallingBlock(private)) => {
                EntityKind::FallingBlock(FallingBlock {
                    state,
                    start_pos,
                    time: private.time,
                    drop_item: private.drop_item,
                    cancel_drop: private.cancel_drop,
                    hurt_entities: private.hurt_entities,
                    fall_damage_per_distance: private.fall_damage_per_distance,
                    fall_damage_max: private.fall_damage_max,
                })
            }
            (KindPublic::Tnt { fuse }, KindState::Tnt) => EntityKind::Tnt(PrimedTnt { fuse }),
            (
                KindPublic::Item {
                    stack,
                    pickup_delay,
                    age,
                    health,
                    target,
                },
                KindState::Item(private),
            ) => EntityKind::Item(GroundItem {
                stack,
                age,
                pickup_delay,
                health,
                bob_offs: private.bob_offs,
                roll_velocity: private.roll_velocity,
                target,
            }),
            // 两半永远成对生成，配对错了是内部错误而不是客户端输入。
            _ => unreachable!("mismatched public and private entity halves"),
        };
        Entity {
            id: public.uid,
            kind,
            position: public.position,
            delta: public.delta,
            old_position: state.old_position,
            on_ground: public.on_ground,
            horizontal_collision: state.horizontal_collision,
            vertical_collision: state.vertical_collision,
            vertical_collision_below: state.vertical_collision_below,
            fall_distance: state.fall_distance,
            tick_count: state.tick_count,
            main_supporting_block: state.main_supporting_block,
            on_ground_no_blocks: state.on_ground_no_blocks,
            in_water: state.in_water,
            removed: public.removed,
            stuck_speed_multiplier: state.stuck_speed_multiplier,
            movement_this_tick: state.movement_this_tick,
            final_movements: state.final_movements,
            requires_precise_position: public.precise,
            random: state.random,
            y_rot: public.yaw,
            first_tick: state.first_tick,
            remaining_fire_ticks: state.remaining_fire_ticks,
        }
    }
}

impl Entity {
    /// 把完整快照拆成一行两半。
    pub fn split(self) -> EntityRow {
        let (public_kind, state_kind) = match self.kind {
            EntityKind::FallingBlock(data) => (
                KindPublic::FallingBlock {
                    state: data.state,
                    start_pos: data.start_pos,
                },
                KindState::FallingBlock(FallingBlockState {
                    time: data.time,
                    drop_item: data.drop_item,
                    cancel_drop: data.cancel_drop,
                    hurt_entities: data.hurt_entities,
                    fall_damage_per_distance: data.fall_damage_per_distance,
                    fall_damage_max: data.fall_damage_max,
                }),
            ),
            EntityKind::Tnt(data) => (KindPublic::Tnt { fuse: data.fuse }, KindState::Tnt),
            EntityKind::Item(data) => (
                KindPublic::Item {
                    stack: data.stack,
                    pickup_delay: data.pickup_delay,
                    age: data.age,
                    health: data.health,
                    target: data.target,
                },
                KindState::Item(ItemState {
                    bob_offs: data.bob_offs,
                    roll_velocity: data.roll_velocity,
                }),
            ),
        };
        EntityRow {
            public: EntityPublic {
                uid: self.id,
                kind: public_kind,
                position: self.position,
                delta: self.delta,
                on_ground: self.on_ground,
                yaw: self.y_rot,
                removed: self.removed,
                precise: self.requires_precise_position,
            },
            state: EntityState {
                kind: state_kind,
                old_position: self.old_position,
                tick_count: self.tick_count,
                fall_distance: self.fall_distance,
                horizontal_collision: self.horizontal_collision,
                vertical_collision: self.vertical_collision,
                vertical_collision_below: self.vertical_collision_below,
                on_ground_no_blocks: self.on_ground_no_blocks,
                main_supporting_block: self.main_supporting_block,
                in_water: self.in_water,
                first_tick: self.first_tick,
                remaining_fire_ticks: self.remaining_fire_ticks,
                stuck_speed_multiplier: self.stuck_speed_multiplier,
                movement_this_tick: self.movement_this_tick,
                final_movements: self.final_movements,
                random: self.random,
            },
        }
    }
}

/// 一个区块里的全部实体，按 uid 排序。字段不公开，只能整体搬走和放回。
#[derive(Debug)]
pub struct ChunkEntities {
    rows: Vec<EntityRow>,
}

/// 并行阶段摊开的三个平行数组：`public` 只读，`next_public` / `state` 每个任务写自己那行。
#[derive(Debug)]
pub struct EntitySnapshot {
    pub uids: Vec<EntityUid>,
    pub public: Vec<EntityPublic>,
    pub next_public: Vec<EntityPublic>,
    pub state: Vec<EntityState>,
    /// `next_public.clone_from(&public)` 那一步的耗时，给基准用。
    pub clone_time: std::time::Duration,
}

/// 一个 region 的实体表。
///
/// 行槽的下标稳定，`take` / `put` O(1)；`index` 是 uid 到行号的映射，`grid` 是 16³ 段的行号索引。
/// tick 期间新生成的实体进 `pending`，下一 tick 才开始 tick，和官方的 `EntityTickList` 一样。
#[derive(Debug)]
pub struct EntityStore {
    slots: Vec<Option<EntityRow>>,
    index: BTreeMap<EntityUid, usize>,
    /// tick 期间被拿出去的行记着原来的槽位，`put` 放回同一个槽位。
    taken: BTreeMap<EntityUid, usize>,
    free: Vec<usize>,
    grid: BTreeMap<(i32, i32, i32), BTreeSet<usize>>,
    next_id: EntityUid,
    pending: Vec<EntityRow>,
    ticking: bool,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            index: BTreeMap::new(),
            taken: BTreeMap::new(),
            free: Vec::new(),
            grid: BTreeMap::new(),
            next_id: FIRST_ENTITY_ID,
            pending: Vec::new(),
            ticking: false,
        }
    }
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从指定的实体 ID 开始分配。oracle 的假世界用这个对齐官方的 `getNextEntityId`。
    pub fn with_next_id(next_id: EntityUid) -> Self {
        Self {
            next_id,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// 按 uid 升序的所有 ID，也就是生成的先后。
    pub fn ids(&self) -> Vec<EntityUid> {
        self.index.keys().copied().collect()
    }

    /// 已经在 tick 列表里的实体，组装成完整快照。正在 tick 的那一个不在里面。
    pub fn get(&self, uid: EntityUid) -> Option<Entity> {
        let row = *self.index.get(&uid)?;
        Some(self.slots[row].as_ref()?.clone().into_entity())
    }

    /// 只读公开数据。正在 tick 的那一个读不到。
    pub fn public_at(&self, uid: EntityUid) -> Option<&EntityPublic> {
        let row = *self.index.get(&uid)?;
        Some(&self.slots[row].as_ref()?.public)
    }

    /// 改一行：拼成完整快照交给回调，再拆回两半。这样 public / state / 索引总是一致。
    pub fn with_mut<R>(&mut self, uid: EntityUid, f: impl FnOnce(&mut Entity) -> R) -> Option<R> {
        let row = *self.index.get(&uid)?;
        let mut entity = self.slots[row].as_ref()?.clone().into_entity();
        let result = f(&mut entity);
        let replaced = entity.split();
        self.replace_row(row, replaced);
        Some(result)
    }

    /// 改一行，包括刚生成还没并进 tick 列表的。
    pub fn with_mut_including_pending<R>(
        &mut self,
        uid: EntityUid,
        f: impl FnOnce(&mut Entity) -> R,
    ) -> Option<R> {
        if self.index.contains_key(&uid) {
            return self.with_mut(uid, f);
        }
        let row = self.pending.iter_mut().find(|row| row.public.uid == uid)?;
        let mut entity = row.clone().into_entity();
        let result = f(&mut entity);
        *row = entity.split();
        Some(result)
    }

    /// 同 [`EntityStore::with_mut`]，但回调是 `dyn`，给 `&mut dyn Level` 用。
    pub fn with_mut_dyn(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool {
        self.with_mut(uid, f).is_some()
    }

    /// 同 [`EntityStore::with_mut_including_pending`]，但回调是 `dyn`。
    pub fn with_mut_including_pending_dyn(
        &mut self,
        uid: EntityUid,
        f: &mut dyn FnMut(&mut Entity),
    ) -> bool {
        self.with_mut_including_pending(uid, f).is_some()
    }

    /// `discard()`：只打标记，实体还留在这一拍的名单里。
    pub fn discard(&mut self, uid: EntityUid) -> bool {
        self.with_mut(uid, |entity| entity.discard()).is_some()
    }

    /// 完整快照的只读遍历，按 uid 升序。
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.index.values().filter_map(|&row| {
            self.slots[row]
                .as_ref()
                .map(|entry| entry.clone().into_entity())
        })
    }

    /// 按 uid 升序遍历公开数据。
    pub fn iter_public(&self) -> impl Iterator<Item = &EntityPublic> + '_ {
        self.index
            .values()
            .filter_map(|&row| self.slots[row].as_ref().map(|entry| &entry.public))
    }

    /// 空间索引：和 `area` 相交的实体公开数据，按 uid 升序。
    pub fn for_each_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        let mut rows = Vec::new();
        let (first, last) = section_range(&area);
        for (_, set) in self.grid.range(first..=last) {
            rows.extend(set.iter().copied());
        }
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            let Some(entry) = &self.slots[row] else {
                continue;
            };
            if entry.public.bounding_box().intersects(area) {
                visit(&entry.public);
            }
        }
    }

    /// `addFreshEntity`：分配 ID 放进世界。tick 期间生成的实体下一 tick 才动。
    pub fn spawn(&mut self, mut entity: Entity) -> EntityUid {
        entity.id = self.next_id;
        self.next_id += 1;
        entity.random = JavaRandom::new(i64::from(entity.id) ^ ENTITY_RANDOM_SEED);
        // 物品的朝向和初速度用刚设好的实体随机源，不能用 `level.random`。
        entity.finish_item_random();
        let uid = entity.id;
        let row = entity.split();
        if self.ticking {
            self.pending.push(row);
        } else {
            self.insert_row(row);
        }
        uid
    }

    /// 从 [`EntityRow`] 直接生成，给命令落地时串行分配 ID 用。
    pub fn spawn_row(&mut self, mut row: EntityRow) -> EntityUid {
        let uid = self.next_id;
        self.next_id += 1;
        row.public.uid = uid;
        row.state.random = JavaRandom::new(i64::from(uid) ^ ENTITY_RANDOM_SEED);
        row.public.removed = false;
        if self.ticking {
            self.pending.push(row);
        } else {
            self.insert_row(row);
        }
        uid
    }

    fn insert_row(&mut self, row: EntityRow) {
        let row_index = self.allocate_slot();
        self.index.insert(row.public.uid, row_index);
        self.grid_insert(row_index, &row.public);
        self.slots[row_index] = Some(row);
    }

    fn allocate_slot(&mut self) -> usize {
        if let Some(free) = self.free.pop() {
            return free;
        }
        self.slots.push(None);
        self.slots.len() - 1
    }

    fn grid_insert(&mut self, row: usize, public: &EntityPublic) {
        for key in sections_of(public.bounding_box()) {
            self.grid.entry(key).or_default().insert(row);
        }
    }

    fn grid_remove(&mut self, row: usize, public: &EntityPublic) {
        for key in sections_of(public.bounding_box()) {
            if let Some(rows) = self.grid.get_mut(&key) {
                rows.remove(&row);
                if rows.is_empty() {
                    self.grid.remove(&key);
                }
            }
        }
    }

    /// 从 tick 列表里拿掉。正在 tick 的实体只标记 `discard`，由 tick 循环丢掉。
    pub fn remove(&mut self, uid: EntityUid) {
        let Some(row) = self.index.remove(&uid) else {
            return;
        };
        self.taken.remove(&uid);
        if let Some(entry) = self.slots[row].take() {
            self.grid_remove(row, &entry.public);
        }
        self.free.push(row);
    }

    /// tick 期间取出一行。取出的行不在表里，别的实体读不到它，和现在的 `take` 一致。
    pub fn take(&mut self, uid: EntityUid) -> Option<Entity> {
        let row = self.index.remove(&uid)?;
        let entry = self.slots[row].take()?;
        self.grid_remove(row, &entry.public);
        self.taken.insert(uid, row);
        Some(entry.into_entity())
    }

    /// 放回 tick 过的行，占回原来的槽位。
    pub fn put(&mut self, entity: Entity) {
        let uid = entity.id;
        let row = self
            .taken
            .remove(&uid)
            .unwrap_or_else(|| self.allocate_slot());
        self.replace_row(row, entity.split());
        self.index.insert(uid, row);
    }

    fn replace_row(&mut self, row: usize, new: EntityRow) {
        let old = self.slots[row].take();
        let changed = old.as_ref().is_none_or(|old| {
            sections_of(old.public.bounding_box()) != sections_of(new.public.bounding_box())
        });
        if changed {
            if let Some(old) = &old {
                self.grid_remove(row, &old.public);
            }
            self.grid_insert(row, &new.public);
        }
        self.slots[row] = Some(new);
    }

    pub fn begin_tick(&mut self) {
        self.ticking = true;
    }

    /// tick 结束时把这次生成的实体并进来，它们下一 tick 才 tick。
    pub fn end_tick(&mut self) {
        self.ticking = false;
        let pending = std::mem::take(&mut self.pending);
        for row in pending {
            self.insert_row(row);
        }
    }

    /// 并行阶段开始：摊出三个平行数组，按 uid 升序。
    pub fn snapshot(&self) -> EntitySnapshot {
        use std::time::Instant;
        let mut uids = Vec::with_capacity(self.index.len());
        let mut public = Vec::with_capacity(self.index.len());
        let mut state = Vec::with_capacity(self.index.len());
        for &row in self.index.values() {
            let entry = self.slots[row].as_ref().expect("indexed row is live");
            uids.push(entry.public.uid);
            public.push(entry.public.clone());
            state.push(entry.state.clone());
        }
        let clone_start = Instant::now();
        let mut next_public = Vec::with_capacity(public.len());
        next_public.clone_from(&public);
        EntitySnapshot {
            uids,
            public,
            next_public,
            state,
            clone_time: clone_start.elapsed(),
        }
    }

    /// 并行阶段结束：把新的公开和私有数据写回表，按 uid 升序重建索引。
    pub fn publish(&mut self, snapshot: EntitySnapshot) {
        let EntitySnapshot {
            uids,
            next_public: public,
            state,
            ..
        } = snapshot;
        self.slots.clear();
        self.index.clear();
        self.taken.clear();
        self.free.clear();
        self.grid.clear();
        for (row, ((uid, public), state)) in uids.into_iter().zip(public).zip(state).enumerate() {
            self.grid_insert(row, &public);
            self.slots.push(Some(EntityRow { public, state }));
            self.index.insert(uid, row);
        }
    }

    /// 把 `chunk` 里的实体装进盒子搬走。只在 tick 边界调用，tick 中途返回 [`Busy`]。
    pub fn take_chunk(&mut self, chunk: ChunkPos) -> Result<ChunkEntities, Busy> {
        if self.ticking || !self.taken.is_empty() {
            return Err(Busy);
        }
        let taken: Vec<EntityUid> = self
            .index
            .iter()
            .filter(|(_, row)| {
                self.slots[**row]
                    .as_ref()
                    .is_some_and(|entry| entry.public.chunk_position() == chunk)
            })
            .map(|(&uid, _)| uid)
            .collect();
        let mut rows = Vec::with_capacity(taken.len());
        for uid in taken {
            let row = self.index.remove(&uid).expect("just indexed");
            self.taken.remove(&uid);
            let entry = self.slots[row].take().expect("indexed row is live");
            self.grid_remove(row, &entry.public);
            self.free.push(row);
            rows.push(entry);
        }
        rows.sort_by_key(|entry| entry.public.uid);
        Ok(ChunkEntities { rows })
    }

    /// 把盒子里的实体放回表里。调用方保证表里已经没有这个区块的实体。
    pub fn insert_chunk(&mut self, entities: ChunkEntities) {
        for row in entities.rows {
            self.insert_row(row);
        }
    }
}

/// 一个盒子可能跨多个 16³ 段，每一段都要登记，范围查询才不会漏。
fn sections_of(bounds: Aabb) -> Vec<(i32, i32, i32)> {
    let (first, last) = section_range(&bounds);
    let mut keys = Vec::new();
    for x in first.0..=last.0 {
        for y in first.1..=last.1 {
            for z in first.2..=last.2 {
                keys.push((x, y, z));
            }
        }
    }
    keys
}

fn section_range(area: &Aabb) -> ((i32, i32, i32), (i32, i32, i32)) {
    (
        (
            (area.min_x.floor() as i32).div_euclid(SECTION_SIZE),
            (area.min_y.floor() as i32).div_euclid(SECTION_SIZE),
            (area.min_z.floor() as i32).div_euclid(SECTION_SIZE),
        ),
        (
            (area.max_x.floor() as i32).div_euclid(SECTION_SIZE),
            (area.max_y.floor() as i32).div_euclid(SECTION_SIZE),
            (area.max_z.floor() as i32).div_euclid(SECTION_SIZE),
        ),
    )
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::state::BlockState;

    #[test]
    fn a_row_survives_a_split_and_join_round_trip() {
        for entity in [
            Entity::falling_block(BlockPos::new(1, 2, 3), BlockState(blocks::STONE)),
            Entity::primed_tnt(Vec3::new(0.5, 64.0, 0.5), 0.25),
            Entity::ground_item(Vec3::new(0.5, 64.0, 0.5), ItemStack::new(7, 3)),
        ] {
            let mut original = entity;
            original.id = 42;
            original.position = Vec3::new(1.25, 70.5, -3.75);
            original.delta = Vec3::new(0.1, -0.2, 0.3);
            original.tick_count = 17;
            original.remaining_fire_ticks = 5;
            assert_eq!(original.clone().split().into_entity(), original);
        }
    }

    #[test]
    fn ids_and_queries_follow_uid_order() {
        let mut store = EntityStore::new();
        let first = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let second = store.spawn(Entity::ground_item(
            Vec3::new(0.6, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        assert_eq!(store.ids(), [first, second]);
        let mut seen = Vec::new();
        store.for_each_in(Aabb::new(0.0, 63.0, 0.0, 2.0, 66.0, 2.0), &mut |public| {
            seen.push(public.uid)
        });
        assert_eq!(seen, [first, second]);
    }

    #[test]
    fn a_query_finds_an_entity_whose_box_spans_a_section_boundary() {
        let mut store = EntityStore::new();
        // 原点在 (15.9, 64, 0.5)，物品盒子跨 x=16 的段界。
        let uid = store.spawn(Entity::ground_item(
            Vec3::new(15.9, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        store.with_mut(uid, |entity| entity.position.x = 16.1);
        let mut seen = Vec::new();
        store.for_each_in(Aabb::new(15.5, 63.0, 0.0, 16.5, 66.0, 2.0), &mut |public| {
            seen.push(public.uid)
        });
        assert_eq!(seen, [uid]);
    }

    #[test]
    fn a_taken_row_is_invisible_until_it_is_put_back() {
        let mut store = EntityStore::new();
        let uid = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let entity = store.take(uid).unwrap();
        assert!(store.public_at(uid).is_none());
        store.put(entity);
        assert_eq!(store.public_at(uid).map(|public| public.uid), Some(uid));
    }

    #[test]
    fn pending_entities_start_ticking_next_tick() {
        let mut store = EntityStore::new();
        store.begin_tick();
        let uid = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        assert!(store.public_at(uid).is_none());
        assert!(store.with_mut_including_pending(uid, |_| {}).is_some());
        store.end_tick();
        assert_eq!(store.public_at(uid).map(|public| public.uid), Some(uid));
    }

    #[test]
    fn a_snapshot_publishes_back_unchanged() {
        let mut store = EntityStore::new();
        let first = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let second = store.spawn(Entity::falling_block(
            BlockPos::new(1, 64, 1),
            BlockState(blocks::SAND),
        ));
        let before: Vec<_> = store.iter().collect();
        let mut snapshot = store.snapshot();
        snapshot.next_public[0].position.x += 1.0;
        store.publish(snapshot);
        let after: Vec<_> = store.iter().collect();
        assert_eq!(store.ids(), [first, second]);
        assert_ne!(before, after);
        assert_eq!(after[0].position.x, 1.5);
    }

    #[test]
    fn a_chunk_moves_out_and_back_at_tick_boundaries() {
        let mut store = EntityStore::new();
        let here = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let elsewhere = store.spawn(Entity::ground_item(
            Vec3::new(40.5, 64.0, 0.5),
            ItemStack::new(2, 1),
        ));
        let boxed = store.take_chunk(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(store.ids(), [elsewhere]);
        store.insert_chunk(boxed);
        assert_eq!(store.ids(), [here, elsewhere]);
        store.begin_tick();
        assert!(store.take_chunk(ChunkPos::new(0, 0)).is_err());
        store.end_tick();
    }

    /// 参照 `BlockEntities::take_chunk`：盒子能跨线程搬，实体按 uid 排序，
    /// tick 中途一律 Busy，别的区块的实体不受影响。
    #[test]
    fn chunk_boxes_move_whole_chunks_at_tick_boundaries() {
        fn assert_send<T: Send>() {}
        assert_send::<ChunkEntities>();

        let mut store = EntityStore::new();
        let first = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let second = store.spawn(Entity::falling_block(
            BlockPos::new(1, 70, 1),
            BlockState(blocks::SAND),
        ));
        let third = store.spawn(Entity::ground_item(
            Vec3::new(40.5, 64.0, 0.5),
            ItemStack::new(2, 1),
        ));
        let fourth = store.spawn(Entity::ground_item(
            Vec3::new(-0.5, 64.0, 0.5),
            ItemStack::new(3, 1),
        ));

        // 同一区块的两件按 uid 升序装盒，别的区块原地不动。
        let boxed = store.take_chunk(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(store.ids(), [third, fourth]);
        let moved = std::thread::spawn(move || boxed).join().unwrap();
        store.insert_chunk(moved);
        assert_eq!(store.ids(), [first, second, third, fourth]);
        assert_eq!(
            store.public_at(second).map(|public| public.position.y),
            Some(70.0)
        );

        // tick 中途和有行被拿出去时都拒绝搬。
        store.begin_tick();
        assert!(store.take_chunk(ChunkPos::new(0, 0)).is_err());
        store.end_tick();
        let taken = store.take(first).unwrap();
        assert!(store.take_chunk(ChunkPos::new(0, 0)).is_err());
        store.put(taken);
        assert!(store.take_chunk(ChunkPos::new(0, 0)).is_ok());
    }

    use mc_bamboo_data::blocks;
}
