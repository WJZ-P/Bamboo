//! 一个 region 里的全部实体，公开和私有数据分开存。
//!
//! 见 `docs/world-model-design.md` 4.3：并行算的阶段里每个任务只读整个 `public`、
//! 只写自己那一行 `state`，所以两半不能放在同一个结构体里。这里的行槽保持下标稳定，
//! `take` / `put` 都是 O(1)，并行阶段另外把三个平行数组摊开，见 [`EntityStore::snapshot`]。
//!
//! All entities in a region, with public and private halves stored apart. See the world-model
//! design, section 4.3. Row slots keep their indices stable; [`EntityStore::snapshot`] lays
//! out three parallel arrays for the parallel driver.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
};

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::entity_types;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use crate::{shape::Aabb, state::BlockState, ticks::Busy};

use super::{
    ENTITY_RANDOM_SEED, Entity, EntityKind, FIRST_ENTITY_ID, FallingBlock, GroundItem, JavaRandom,
    Movement, PrimedTnt,
};

/// 发给客户端的实体 ID，也是实体之间互相引用的键。
pub type EntityUid = i32;

/// 非玩家实体的网络 ID 分配器，所有拿着同一个句柄的实体表共用一个计数器。
///
/// 每个世界一个（`docs/island-implementation-plan.md` 第 6 节第 2 步）：以后一个世界
/// 拆成多个岛、每个岛一张 [`EntityStore`]，号也不会撞。不用进程级静态量，免得同一进程里
/// 并行跑的测试互相影响。克隆得到的是同一个计数器的另一个句柄。
/// 只有一张表在分配时，号的序列和原来表内自带的计数器逐个相同：从 [`FIRST_ENTITY_ID`]
/// 起，每次加一。
///
/// Network-id allocator for non-player entities; every store holding the same
/// handle shares one counter. One per world, so that islands with separate
/// stores never collide. Not a process-wide static, so tests running in
/// parallel in one process stay independent. Cloning yields another handle to
/// the same counter. With a single store allocating, the sequence is exactly
/// the old per-store counter: from [`FIRST_ENTITY_ID`], one at a time.
#[derive(Debug, Clone)]
pub struct EntityIds(Arc<AtomicI32>);

impl Default for EntityIds {
    fn default() -> Self {
        Self::starting_at(FIRST_ENTITY_ID)
    }
}

impl EntityIds {
    /// 从 [`FIRST_ENTITY_ID`] 开始的新计数器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 `next` 开始的新计数器。oracle 的假世界用它对齐官方的 `getNextEntityId`。
    ///
    /// A fresh counter starting at `next`; oracle worlds use it to line up with
    /// the official `getNextEntityId`.
    pub fn starting_at(next: EntityUid) -> Self {
        Self(Arc::new(AtomicI32::new(next)))
    }

    /// 取一个号。分配的先后由调用方的顺序决定（现在都在串行路径上）。
    ///
    /// Take one id. The order is the callers' order (all serial today).
    pub fn allocate(&self) -> EntityUid {
        self.0.fetch_add(1, Ordering::Relaxed)
    }

    /// 下一个会分配出去的号，不消耗。
    ///
    /// The id the next allocation will return, without taking it.
    pub fn peek(&self) -> EntityUid {
        self.0.load(Ordering::Relaxed)
    }

    /// 两个句柄是不是同一个计数器。
    ///
    /// Whether two handles share one counter.
    pub fn same_counter(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

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
    /// `Entity.vehicle`：这一刻开始时骑着的载具；别的实体只看得到这份快照。
    ///
    /// `Entity.vehicle` at the start of this tick; other entities only see this
    /// snapshot of the riding relation.
    pub vehicle: Option<EntityUid>,
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
    /// 经验球：同步只需要 `Value`。
    ExperienceOrb {
        value: i32,
    },
    /// 闪电：客户端要 `seed`、`life` 和闪光次数。
    ///
    /// A lightning bolt: the client reads its seed, life and remaining flashes.
    LightningBolt(super::lightning::LightningBolt),
    /// 生物：血量、朝向、年龄这些别的实体和同步要读的。
    Mob(Box<super::mob::MobPublic>),
    Arrow(super::arrow::ArrowPublic),
    /// 羊驼的口水：同步要的 UUID、主人和俯仰角。
    ///
    /// Llama spit: the UUID, owner and pitch the sync layer needs.
    LlamaSpit(super::spit::LlamaSpitPublic),
    /// 扔出的鸡蛋：同步要它的物品、主人和俯仰角。
    ///
    /// A thrown egg: the sync layer reads its item, owner and pitch.
    Egg(Box<super::thrown_egg::EggPublic>),
    /// 扔出的喷溅药水。
    ///
    /// A thrown splash potion.
    SplashPotion(Box<super::splash_potion::SplashPotionPublic>),
}

impl KindPublic {
    /// 注册表 ID，发 `add_entity` 包时用。
    pub const fn entity_type(&self) -> i32 {
        match self {
            Self::FallingBlock { .. } => mc_bamboo_data::registries::entity_type::FALLING_BLOCK,
            Self::Tnt { .. } => mc_bamboo_data::registries::entity_type::TNT,
            Self::Item { .. } => mc_bamboo_data::registries::entity_type::ITEM,
            Self::ExperienceOrb { .. } => mc_bamboo_data::registries::entity_type::EXPERIENCE_ORB,
            Self::Arrow(arrow) => arrow.variant.entity_type(),
            Self::LlamaSpit(_) => mc_bamboo_data::registries::entity_type::LLAMA_SPIT,
            Self::Egg(egg) => egg.kind.entity_type(),
            Self::SplashPotion(_) => mc_bamboo_data::registries::entity_type::SPLASH_POTION,
            Self::LightningBolt(_) => mc_bamboo_data::registries::entity_type::LIGHTNING_BOLT,
            Self::Mob(mob) => mob.mob_type.entity_type(),
        }
    }

    /// `EntityDimensions`。
    pub fn dimensions(&self) -> (f32, f32) {
        if let Self::Mob(mob) = self {
            return mob.dimensions();
        }
        let facts = &entity_types::FACTS[self.entity_type() as usize];
        (facts.width, facts.height)
    }

    pub const fn is_falling_block(&self) -> bool {
        matches!(self, Self::FallingBlock { .. })
    }

    /// 这个公开种类的生物类型；不是生物时是 `None`。
    pub const fn mob_type(&self) -> Option<super::mob::MobType> {
        match self {
            Self::Mob(mob) => Some(mob.mob_type),
            _ => None,
        }
    }

    /// 按类型的子索引用这个 ID；和注册表 ID 一致，以后加生物不用改索引结构。
    ///
    /// Key for the per-type sub-index; matches the registry id so future mobs
    /// need no structural change.
    pub const fn type_index(&self) -> usize {
        self.entity_type() as usize
    }
}

/// 官方会互相硬碰撞的实体（船、矿车、潜影贝等）。现在三种实体都不是，
/// 所以这个索引恒为空：只留接口和「以后怎么登记」的判定，不给还不存在的实体写逻辑。
///
/// Entities that have hard collisions in vanilla (boats, minecarts, shulkers).
/// None of the three current kinds is one, so this index stays empty: only
/// the interface and the future registration rule exist, with no mob logic.
pub fn is_hard_colliding(public: &EntityPublic) -> bool {
    // 三种现有实体都不是硬碰撞体；以后按新种类扩充这个判定。
    // None of the three current kinds is hard-colliding; extend with new kinds.
    let _ = public;
    false
}

/// 只有自己读写、种类特有的部分。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum KindState {
    FallingBlock(FallingBlockState),
    #[default]
    Tnt,
    Item(ItemState),
    /// 经验球自己的字段（年龄、健康、数量、跟随的玩家）。
    ExperienceOrb(super::ExperienceOrb),
    /// 闪电的全部状态，公开的那一半就是它本身。
    LightningBolt(super::lightning::LightningBolt),
    /// 生物的全部状态。公开的那一半在 [`KindPublic::Mob`] 里另有一份拷贝。
    Mob(Box<super::mob::Mob>),
    Arrow(Box<super::arrow::Arrow>),
    LlamaSpit(Box<super::spit::LlamaSpit>),
    Egg(Box<super::thrown_egg::ThrownEgg>),
    SplashPotion(Box<super::splash_potion::SplashPotion>),
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
#[derive(Debug, Clone, PartialEq, Default)]
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
    /// `Entity.boardingCooldown`，每刻 `baseTick` 减一；不存档。
    ///
    /// `Entity.boardingCooldown`, decremented in `baseTick` every tick; not saved.
    pub boarding_cooldown: i32,
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
            (KindPublic::ExperienceOrb { value }, KindState::ExperienceOrb(mut orb)) => {
                orb.value = value;
                EntityKind::ExperienceOrb(orb)
            }
            (KindPublic::Arrow(_), KindState::Arrow(arrow)) => EntityKind::Arrow(arrow),
            (KindPublic::LlamaSpit(_), KindState::LlamaSpit(spit)) => EntityKind::LlamaSpit(spit),
            (KindPublic::Egg(_), KindState::Egg(egg)) => EntityKind::Egg(egg),
            (KindPublic::SplashPotion(_), KindState::SplashPotion(potion)) => {
                EntityKind::SplashPotion(potion)
            }
            (KindPublic::LightningBolt(_), KindState::LightningBolt(bolt)) => {
                EntityKind::LightningBolt(Box::new(bolt))
            }
            (KindPublic::Mob(_), KindState::Mob(mob)) => EntityKind::Mob(mob),
            // 两半永远成对生成，配对错了是内部错误而不是客户端输入。
            _ => unreachable!("mismatched public and private entity halves"),
        };
        let dimensions = kind.dimensions();
        Entity {
            id: public.uid,
            kind,
            dimensions,
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
            vehicle: public.vehicle,
            boarding_cooldown: state.boarding_cooldown,
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
            EntityKind::ExperienceOrb(orb) => (
                KindPublic::ExperienceOrb { value: orb.value },
                KindState::ExperienceOrb(orb),
            ),
            EntityKind::Arrow(arrow) => (
                KindPublic::Arrow(arrow.public(self.remaining_fire_ticks)),
                KindState::Arrow(arrow),
            ),
            EntityKind::LlamaSpit(spit) => (
                KindPublic::LlamaSpit(spit.public()),
                KindState::LlamaSpit(spit),
            ),
            EntityKind::Egg(egg) => (
                KindPublic::Egg(Box::new(egg.public(self.remaining_fire_ticks))),
                KindState::Egg(egg),
            ),
            EntityKind::SplashPotion(potion) => (
                KindPublic::SplashPotion(Box::new(potion.public(self.remaining_fire_ticks))),
                KindState::SplashPotion(potion),
            ),
            EntityKind::LightningBolt(bolt) => (
                KindPublic::LightningBolt((*bolt).clone()),
                KindState::LightningBolt(*bolt),
            ),
            EntityKind::Mob(mob) => (
                KindPublic::Mob(Box::new(super::mob::MobPublic::of(&mob, self.in_water))),
                KindState::Mob(mob),
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
                vehicle: self.vehicle,
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
                boarding_cooldown: self.boarding_cooldown,
            },
        }
    }
}

/// 一个区块里的全部实体，按 uid 排序。字段不公开，只能整体搬走和放回。
#[derive(Debug, Clone, Default)]
pub struct ChunkEntities {
    rows: Vec<EntityRow>,
    /// 读盘时不认识的实体类型（官方世界里的生物等），整条原样保留、写回。
    /// 它们不进 `EntityStore`，不参与逻辑，也不发给客户端。
    preserved: Vec<Tag>,
    /// 刚从存档解出来：uid 只是占位、随机源还没有种子。放进表时按行的先后重新取号、
    /// 从这个岛的种子源取种子。在岛之间整盒搬运的盒子没有这个标记，原样放回。
    ///
    /// Freshly decoded from disk: uids are placeholders and the random sources are
    /// unseeded. Inserting takes new ids and seeds in row order. Boxes moved live
    /// between islands lack the flag and go back unchanged.
    from_disk: bool,
}

impl ChunkEntities {
    /// 盒子里有没有实体。`is_pristine` 用它判断盒子能不能丢掉。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.preserved.is_empty()
    }

    /// 建模的三种实体，按 uid 排序。
    pub fn rows(&self) -> &[EntityRow] {
        &self.rows
    }

    /// 原样保留的实体条目。
    pub fn preserved(&self) -> &[Tag] {
        &self.preserved
    }

    /// 从实体行和保留条目建盒子，存档解码时用：放进表时重新取号、取种子。
    ///
    /// Build a box from decoded rows and preserved entries. Inserting it assigns
    /// fresh ids and seeds.
    pub fn from_parts(rows: Vec<EntityRow>, preserved: Vec<Tag>) -> Self {
        Self {
            rows,
            preserved,
            from_disk: true,
        }
    }
}

/// 一个 region 的实体表。
///
/// 行槽的下标稳定，`take` / `put` O(1)；`index` 是 uid 到行号的映射，`grid` 是 16³ 段的行号索引。
/// tick 期间新生成的实体进 `pending`，下一 tick 才开始 tick，和官方的 `EntityTickList` 一样。
#[derive(Debug, Clone)]
pub struct EntityStore {
    slots: Vec<Option<EntityRow>>,
    index: BTreeMap<EntityUid, usize>,
    /// tick 期间被拿出去的行记着原来的槽位，`put` 放回同一个槽位。
    taken: BTreeMap<EntityUid, usize>,
    free: Vec<usize>,
    grid: BTreeMap<(i32, i32, i32), BTreeSet<usize>>,
    /// 按所在区块（`EntityPublic::chunk_position`）的子索引：区块 → uid 升序的集合，和
    /// `index` 一起增删。岛拆分、按区块装盒和存盘快照只走要的那几个区块，不扫整张表。
    ///
    /// Sub-index by chunk (`EntityPublic::chunk_position`): chunk → uids in
    /// ascending order, kept in step with `index`. Island splits, chunk boxing
    /// and save snapshots visit only the chunks they need instead of the table.
    by_chunk: HashMap<ChunkPos, BTreeSet<EntityUid>>,
    /// 按注册表实体类型的子索引：类型 ID → uid 升序的行号集合。
    by_type: BTreeMap<usize, BTreeSet<EntityUid>>,
    /// 硬碰撞实体的 uid 集合，现在恒为空。
    hard_colliding: BTreeSet<EntityUid>,
    /// 共享的 ID 分配器句柄；表里不再有自己的计数器。
    /// Handle to the shared id allocator; the store has no counter of its own.
    ids: EntityIds,
    /// 这个岛的实体种子源：每个实体生成（或读盘放进来）时从这里取一个 `nextLong` 当
    /// 自己随机源的种子，见 [`Self::next_entity_seed`]。官方实体的随机源不带种子，
    /// 所以种子从哪来不影响和官方一致；放在实体表上，拆合时跟着表走，取过的种子
    /// 留在实体身上。和网络 ID 无关：多个岛交替取号也不改变谁拿到哪个种子。
    ///
    /// The island's entity seed source. Every entity spawned (or loaded from disk)
    /// takes one `nextLong` from it as the seed of its own random. Vanilla entity
    /// randoms are unseeded, so the source does not affect vanilla parity. It lives
    /// on the table and splits and merges with it; a drawn seed stays on its entity.
    /// Independent of network ids, so islands interleaving id allocation do not
    /// change which entity gets which seed.
    seeds: JavaRandom,
    pending: Vec<EntityRow>,
    ticking: bool,
    /// 读盘时不认识的实体类型，按区块原样保留。
    preserved: Vec<Tag>,
    /// 睡着的方块实体关注的 16³ 段。空表时实体写入完全不额外做事。
    watched_sections: HashSet<(i32, i32, i32)>,
    /// 精确驱动 tick 期间被拿出去、所在段有人关注的行，记着旧位置和物品堆。
    taken_snapshots: BTreeMap<EntityUid, (Aabb, Option<ItemStack>)>,
    /// 位置或堆变了的实体的新包围盒，交给方块实体表精确判定。
    wake_events: Vec<Aabb>,
    /// 刚进世界的实体的公开数据，供同步层按官方 `ChunkMap.addEntity` 的时机建
    /// `ServerEntity`。`sync` 每刻取走一次。
    ///
    /// Public data for entities that just joined the world, so the sync layer
    /// can create its tracking state at vanilla `ChunkMap.addEntity` timing.
    spawn_snapshots: BTreeMap<EntityUid, EntityPublic>,
    /// 这一刻被丢弃的生物身上还没发出去的事件（例如死亡动画结束的 `Poof`）。
    /// 实体行已经离表，`take_mob_events` 从这里补上，顺序按 uid。每条记着生物离场时
    /// 所在的区块：行已经不在了，岛拆分时只能靠它知道这条事件该跟哪一边走。
    ///
    /// Events left on mobs discarded this tick (e.g. the `Poof` at the end of
    /// the death animation). Their rows are gone; `take_mob_events` appends
    /// these in uid order. Each entry keeps the chunk the mob was in when it
    /// left: with the row gone, an island split has nothing else to tell
    /// which side the events belong to.
    departed_mob_events: BTreeMap<EntityUid, (ChunkPos, Vec<super::mob::MobEvent>)>,
    /// 三档降频的限量唤醒令牌桶（每岛一份）。缺省按刻号 0 的满桶起。
    wake_tokens: super::mob::throttle::WakeTokens,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            index: BTreeMap::new(),
            taken: BTreeMap::new(),
            free: Vec::new(),
            grid: BTreeMap::new(),
            by_chunk: HashMap::new(),
            by_type: BTreeMap::new(),
            hard_colliding: BTreeSet::new(),
            ids: EntityIds::default(),
            seeds: JavaRandom::new(ENTITY_RANDOM_SEED),
            pending: Vec::new(),
            ticking: false,
            preserved: Vec::new(),
            watched_sections: HashSet::new(),
            taken_snapshots: BTreeMap::new(),
            wake_events: Vec::new(),
            spawn_snapshots: BTreeMap::new(),
            departed_mob_events: BTreeMap::new(),
            wake_tokens: super::mob::throttle::WakeTokens::full(
                0,
                super::mob::throttle::Config::OFF,
            ),
        }
    }
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 换掉种子源。对拍的假世界用它和官方那边的种子序列对齐。
    ///
    /// Replace the seed source; oracle worlds use it to line up with the
    /// official side's seed sequence.
    pub fn set_entity_seeds(&mut self, seed: i64) {
        self.seeds = JavaRandom::new(seed);
    }

    /// 种子源现在的样子（连同内部状态）。主控把全部岛临时拼成一个时存下每个岛的种子源，
    /// 拆回去时原样换上（[`Self::set_entity_seed_source`]）。
    ///
    /// The seed source as it is now, state included. When the master
    /// temporarily joins every island into one it keeps each island's source
    /// and puts it back unchanged afterwards ([`Self::set_entity_seed_source`]).
    pub const fn entity_seed_source(&self) -> JavaRandom {
        self.seeds
    }

    /// 换上一个存下来的种子源（接着它原来的状态往下取）。
    ///
    /// Install a saved seed source, continuing from its state.
    pub fn set_entity_seed_source(&mut self, source: JavaRandom) {
        self.seeds = source;
    }

    /// 取下一个实体种子：新实体的随机源是 `JavaRandom::new(这个数)`。
    ///
    /// The next entity seed: a new entity's random is `JavaRandom::new(seed)`.
    pub fn next_entity_seed(&mut self) -> i64 {
        self.seeds.next_long()
    }

    /// 种子源现在的状态，拆合核对和测试用。
    ///
    /// The seed source's current state, for split checks and tests.
    pub const fn entity_seed_state(&self) -> i64 {
        self.seeds.state()
    }

    /// 从指定的实体 ID 开始分配（自己一个新计数器）。oracle 的假世界用这个对齐官方的
    /// `getNextEntityId`。
    ///
    /// Allocate from `next_id` on a fresh counter of its own; oracle worlds use
    /// this to line up with the official `getNextEntityId`.
    pub fn with_next_id(next_id: EntityUid) -> Self {
        Self::with_ids(EntityIds::starting_at(next_id))
    }

    /// 用世界共享的分配器建一张空表。
    ///
    /// An empty store drawing ids from the world's shared allocator.
    pub fn with_ids(ids: EntityIds) -> Self {
        Self {
            ids,
            ..Self::default()
        }
    }

    /// 这张表取号用的分配器句柄。
    ///
    /// The allocator handle this store draws ids from.
    pub fn ids_handle(&self) -> &EntityIds {
        &self.ids
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// 读盘时不认识的实体类型，按区块原样保留。给每刻的内容指纹用（补 9）。
    /// Unknown entity types kept verbatim from disk, used by the per-tick content fingerprint (item 9).
    pub fn preserved(&self) -> &[Tag] {
        &self.preserved
    }

    /// 按 uid 升序的所有 ID，也就是生成的先后。
    pub fn ids(&self) -> Vec<EntityUid> {
        debug_assert!(self.sub_index_matches(), "实体子索引和主表不一致");
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
        self.replace_row(row, replaced, true);
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

    /// uid 到行号。`ids()` 已经按 uid 升序，所以这个映射也是升序。
    pub fn row_of(&self, uid: EntityUid) -> Option<usize> {
        self.index.get(&uid).copied()
    }
    /// 一个类型里全部实体的 uid，按 uid 升序；遍历顺序和 `ids()` 一致。
    ///
    /// Uids of all entities of one registry type, in ascending order: the same
    /// iteration order as `ids()`.
    pub fn uids_of_type(&self, entity_type: i32) -> impl Iterator<Item = EntityUid> + '_ {
        let key = usize::try_from(entity_type).unwrap_or(usize::MAX);
        self.by_type
            .get(&key)
            .into_iter()
            .flat_map(BTreeSet::iter)
            .copied()
    }

    /// 硬碰撞实体的 uid，按 uid 升序。现在恒为空。
    ///
    /// Uids of hard-colliding entities, in ascending order. Empty today.
    pub fn hard_colliding_uids(&self) -> impl Iterator<Item = EntityUid> + '_ {
        self.hard_colliding.iter().copied()
    }

    /// 这两个子索引是不是和主表一致；调试构建和单测用它守住 O(1) 的取放路径。
    /// 直接核对已有集合的包含关系和总元素数，不在热路径上重新分配三张表。
    ///
    /// Whether the sub-indices match the main table. Checks membership and element
    /// counts in place without allocating fresh maps.
    fn sub_index_matches(&self) -> bool {
        if self.by_type.values().any(BTreeSet::is_empty)
            || self.by_chunk.values().any(BTreeSet::is_empty)
        {
            return false;
        }
        let mut live = 0usize;
        let mut hard = 0usize;
        for (&uid, &row) in &self.index {
            let Some(entry) = self.slots.get(row).and_then(Option::as_ref) else {
                continue;
            };
            live += 1;
            if !self
                .by_type
                .get(&entry.public.kind.type_index())
                .is_some_and(|uids| uids.contains(&uid))
            {
                return false;
            }
            if !self
                .by_chunk
                .get(&entry.public.chunk_position())
                .is_some_and(|uids| uids.contains(&uid))
            {
                return false;
            }
            if is_hard_colliding(&entry.public) {
                hard += 1;
                if !self.hard_colliding.contains(&uid) {
                    return false;
                }
            }
        }
        let type_total: usize = self.by_type.values().map(BTreeSet::len).sum();
        let chunk_total: usize = self.by_chunk.values().map(BTreeSet::len).sum();
        type_total == live && chunk_total == live && self.hard_colliding.len() == hard
    }

    /// 只给改坏确认用：从按区块的子索引里漏掉一个实体。
    ///
    /// Break-it checks only: drop one entity from the chunk sub-index.
    #[doc(hidden)]
    pub fn forget_chunk_index_for_test(&mut self, uid: EntityUid) {
        for uids in self.by_chunk.values_mut() {
            uids.remove(&uid);
        }
    }

    /// 按区块的子索引里记一笔 / 去掉一笔。
    fn chunk_index_add(&mut self, uid: EntityUid, chunk: ChunkPos) {
        self.by_chunk.entry(chunk).or_default().insert(uid);
    }

    fn chunk_index_remove(&mut self, uid: EntityUid, chunk: ChunkPos) {
        if let Some(uids) = self.by_chunk.get_mut(&chunk) {
            uids.remove(&uid);
            if uids.is_empty() {
                self.by_chunk.remove(&chunk);
            }
        }
    }

    /// 位置落在这个区块里的实体，uid 升序。
    fn uids_in_chunk(&self, chunk: ChunkPos) -> Vec<EntityUid> {
        self.by_chunk
            .get(&chunk)
            .map(|uids| uids.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 空间索引里覆盖 `area` 的行号，按行号升序（行号来自 uid 升序的槽位分配，
    /// 但回收槽位后不一定等价于 uid 升序，调用方要按 uid 排序）。
    pub fn grid_rows_in(&self, area: Aabb) -> Vec<usize> {
        let (first, last) = section_range(&area);
        let mut rows = Vec::new();
        for (_, set) in self.grid.range(first..=last) {
            rows.extend(set.iter().copied());
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// 只读一行。给并行算阶段的引用快照用：这里不克隆，也不搬走行。
    pub fn row_at(&self, row: usize) -> Option<&EntityRow> {
        self.slots.get(row)?.as_ref()
    }

    /// 把这一行的私有状态移出来，算阶段独占它；别的实体只读 `public`。
    pub fn take_state(&mut self, row: usize) -> Option<EntityState> {
        Some(std::mem::take(
            &mut self.slots.get_mut(row)?.as_mut()?.state,
        ))
    }

    /// 算阶段开始时按 uid 升序列出 (uid, 行号)，同时把各行的私有状态移出来。
    pub fn take_states_for_tick(&mut self) -> (Vec<EntityUid>, Vec<usize>, Vec<EntityState>) {
        let (uids, rows) = self.tick_rows();
        let states = rows
            .iter()
            .map(|&row| self.take_state(row).expect("indexed row is live"))
            .collect();
        (uids, rows, states)
    }

    /// 官方 `EntitySectionStorage` 的分区遍历顺序：`SectionPos.asLong` 升序。
    /// 打包顺序是 `x << 42 | z << 20 | y`，所以外层 x、中层 z、内层 y；段内
    /// `ClassInstanceMultiMap` 是加入顺序，等价 uid 升序。`getEntitiesOfClass`
    /// 用这个顺序建列表，再由 `nextInt(list.size())` 挑一个。
    ///
    /// Vanilla `EntitySectionStorage` order: ascending packed `SectionPos.asLong`
    /// (`x << 42 | z << 20 | y`), then insertion order within a section.
    pub fn for_each_in_section_order(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        let (first, last) = section_range(&area);
        // 只遍历真的登记过的段，按官方的打包键排序（负数坐标也按打包后的值）。
        let mut keys: Vec<(i64, (i32, i32, i32))> = self
            .grid
            .range(first..=last)
            .map(|(&key, _)| (section_key(key), key))
            .collect();
        keys.sort_by_key(|(packed, key)| (*packed, *key));
        keys.dedup_by_key(|(packed, _)| *packed);
        for (_, key) in keys {
            let Some(rows) = self.grid.get(&key) else {
                continue;
            };
            let mut candidates: Vec<(EntityUid, usize)> = rows
                .iter()
                .copied()
                .filter_map(|row| {
                    let entry = self.slots.get(row)?.as_ref()?;
                    Some((entry.public.uid, row))
                })
                .collect();
            candidates.sort_unstable_by_key(|(uid, _)| *uid);
            for (_, row) in candidates {
                let Some(entry) = self.slots.get(row).and_then(|slot| slot.as_ref()) else {
                    continue;
                };
                if entry.public.bounding_box().intersects(area) {
                    visit(&entry.public);
                }
            }
        }
    }

    /// 空间索引：和 `area` 相交的实体公开数据，按 uid 升序。
    pub fn for_each_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.for_each_in_limited(area, usize::MAX, &mut |public| {
            visit(public);
            true
        });
    }

    /// 同 [`Self::for_each_in`]，但 `visit` 返回 `true` 才算一个「够数的」结果：
    /// 收集到 `limit` 个就停止枚举。挤压上限下推到这里，塔场景不必把几百个
    /// 候选全排一遍；不合适的实体（掉落物、不可推挤的生物）不占额度。
    ///
    /// Like [`Self::for_each_in`], but only `true` returns count towards
    /// `limit`; enumeration stops at `limit`. A non-matching entity does not
    /// consume the push budget.
    pub fn for_each_in_limited(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        let mut found: Vec<(EntityUid, &EntityPublic)> = self
            .grid_rows_in(area)
            .into_iter()
            .filter_map(|row| self.slots.get(row)?.as_ref())
            .filter(|entry| entry.public.bounding_box().intersects(area))
            .map(|entry| (entry.public.uid, &entry.public))
            .collect();
        // BTreeMap 的键有序，但槽位会被回收复用，行号顺序不等于 uid 顺序。
        found.sort_unstable_by_key(|(uid, _)| *uid);
        let mut taken = 0;
        for (_, public) in found {
            if taken >= limit {
                break;
            }
            if visit(public) {
                taken += 1;
            }
        }
    }

    /// 把 `take_states_for_tick` 借出去的私有状态原样还回去，公开数据不动。
    /// 测试里临时造一份只读快照时用。
    #[cfg(test)]
    pub(crate) fn restore_states(&mut self, rows: &[usize], state: Vec<EntityState>) {
        debug_assert_eq!(rows.len(), state.len());
        for (&row, state) in rows.iter().zip(state) {
            if let Some(slot) = self.slots.get_mut(row).and_then(Option::as_mut) {
                slot.state = state;
            }
        }
    }

    /// 发布算阶段得到的公开和私有数据：只动位置变了的行，不重建整张表。
    ///
    /// `rows` 是算阶段那一份 uid 升序名单对应的行号；`commands` 之外的移除由调用方用
    /// `remove` 处理，因为算阶段就 `discard` 的实体不该留在表里。
    pub fn publish_rows(
        &mut self,
        rows: &[usize],
        public: Vec<EntityPublic>,
        state: Vec<EntityState>,
    ) {
        debug_assert_eq!(rows.len(), public.len());
        debug_assert_eq!(rows.len(), state.len());
        for ((row, public), state) in rows.iter().copied().zip(public).zip(state) {
            self.replace_row(row, EntityRow { public, state }, true);
        }
        debug_assert!(self.sub_index_matches(), "实体子索引和主表不一致");
    }

    /// `addFreshEntity`：分配 ID 放进世界。tick 期间生成的实体下一 tick 才动。
    pub fn spawn(&mut self, entity: Entity) -> EntityUid {
        let entity = self.prepare(entity);
        self.add(entity)
    }

    /// 官方实体构造函数里的那一半：分配 ID、从种子源取种子、取构造时的随机数（物品的
    /// 朝向和初速度、生物的 UUID 和朝向）。还不放进表；`/summon` 之类要在构造和
    /// `addFreshEntity` 之间读 NBT、`finalizeSpawn`，或者放不进去（UUID 重复）。
    ///
    /// The constructor half: allocate the id, draw the seed and the constructor
    /// draws. The entity is not in the table yet; `/summon` loads NBT and runs
    /// `finalizeSpawn` in between, and may fail to add it.
    pub fn prepare(&mut self, mut entity: Entity) -> Entity {
        entity.id = self.ids.allocate();
        entity.random = JavaRandom::new(self.next_entity_seed());
        // 物品的朝向和初速度用刚设好的实体随机源，不能用 `level.random`。
        entity.finish_item_random();
        super::orb::finish_spawn(&mut entity, Vec3::default());
        entity.finish_mob_random();
        if let EntityKind::Arrow(arrow) = &mut entity.kind {
            arrow.uuid = super::mob::insecure_uuid(&mut entity.random);
        }
        if let EntityKind::LlamaSpit(spit) = &mut entity.kind {
            spit.uuid = super::mob::insecure_uuid(&mut entity.random);
        }
        if let EntityKind::Egg(egg) = &mut entity.kind {
            egg.uuid = super::mob::insecure_uuid(&mut entity.random);
        }
        if let EntityKind::SplashPotion(potion) = &mut entity.kind {
            potion.uuid = super::mob::insecure_uuid(&mut entity.random);
        }
        if matches!(entity.kind, super::EntityKind::LightningBolt(_)) {
            super::lightning::finish_spawn(&mut entity);
        }
        entity
    }

    /// `addFreshEntity` 的另一半：把 [`Self::prepare`] 过的实体放进表。
    ///
    /// The other half of `addFreshEntity`: put a prepared entity in the table.
    pub fn add(&mut self, entity: Entity) -> EntityUid {
        let uid = entity.id;
        let row = entity.split();
        self.spawn_snapshots.insert(uid, row.public.clone());
        if self.ticking {
            self.pending.push(row);
        } else {
            self.insert_row(row);
        }
        uid
    }

    /// 表里（含这一拍刚生成的）有没有这个 UUID 的生物，`tryAddFreshEntityWithPassengers` 用。
    ///
    /// Whether a mob with this UUID is in the table, pending ones included.
    pub fn has_mob_uuid(&self, uuid: &[u8; 16]) -> bool {
        let matches =
            |row: &EntityRow| matches!(&row.public.kind, KindPublic::Mob(mob) if &mob.uuid == uuid);
        self.slots.iter().flatten().any(matches) || self.pending.iter().any(matches)
    }

    /// 从 [`EntityRow`] 直接生成，给命令落地时串行分配 ID 用。
    pub fn spawn_row(&mut self, mut row: EntityRow) -> EntityUid {
        let uid = self.ids.allocate();
        row.public.uid = uid;
        row.state.random = JavaRandom::new(self.next_entity_seed());
        row.public.removed = false;
        self.spawn_snapshots.insert(uid, row.public.clone());
        if self.ticking {
            self.pending.push(row);
        } else {
            self.insert_row(row);
        }
        uid
    }

    fn insert_row(&mut self, row: EntityRow) {
        if !self.watched_sections.is_empty() {
            self.note_entity_change(row.public.bounding_box());
        }
        self.insert_row_quietly(row);
    }

    /// 放进一行，不报告位置事件：岛的拆合只是换了张表，实体没有出现也没有动。
    fn insert_row_quietly(&mut self, row: EntityRow) {
        let row_index = self.allocate_slot();
        self.index_sub(row.public.uid, &row.public);
        self.index.insert(row.public.uid, row_index);
        self.chunk_index_add(row.public.uid, row.public.chunk_position());
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
    /// `Entity.setRemoved` 还会把仍在表里的实体乘客摘下来（这里只清载具名单，
    /// 乘客自己的 `vehicle` 由驱动的 `rideTick` 在下一刻清）。
    ///
    /// Removes an entity from the tick list. Vanilla `setRemoved` also drops its
    /// entity passengers; clear the vehicle's list here, and the driver clears
    /// each passenger's `vehicle` link on its next `rideTick`.
    fn clear_passenger_vehicle(&mut self, passenger: EntityUid, vehicle: EntityUid) {
        let Some(row) = self.index.get(&passenger).copied() else {
            return;
        };
        if let Some(entry) = self
            .slots
            .get_mut(row)
            .and_then(Option::as_mut)
            .filter(|entry| entry.public.vehicle == Some(vehicle))
        {
            entry.public.vehicle = None;
            entry.state.boarding_cooldown = 60;
        }
    }

    pub fn remove(&mut self, uid: EntityUid) {
        if !self.index.contains_key(&uid) {
            return;
        }
        // 直接读公开快照，不把私有状态移出来再拼回去：发布阶段可能已经把两半
        // 分别换过（并行驱动），`with_mut` 会把它们重新拆错。
        let passengers = self
            .public_at(uid)
            .and_then(|public| match &public.kind {
                KindPublic::Mob(mob) => Some(mob.passengers.clone()),
                _ => None,
            })
            .unwrap_or_default();
        self.unindex_sub(uid);
        let row = self.index.remove(&uid).expect("刚查过 uid");
        self.taken.remove(&uid);
        self.taken_snapshots.remove(&uid);
        if let Some(entry) = self.slots[row].take() {
            self.chunk_index_remove(uid, entry.public.chunk_position());
            self.grid_remove(row, &entry.public);
        }
        self.free.push(row);
        for passenger in passengers {
            if passenger < FIRST_ENTITY_ID {
                continue;
            }
            self.clear_passenger_vehicle(passenger, uid);
        }
    }

    /// tick 期间取出一行。取出的行不在表里，别的实体读不到它，和现在的 `take` 一致。
    pub fn take(&mut self, uid: EntityUid) -> Option<Entity> {
        self.unindex_sub(uid);
        let row = self.index.remove(&uid)?;
        let entry = self.slots[row].take()?;
        self.chunk_index_remove(uid, entry.public.chunk_position());
        self.grid_remove(row, &entry.public);
        if !self.watched_sections.is_empty() {
            // 只记旧状态；站住的实体不该在每刻 tick 里把自己的漏斗吵醒。
            self.taken_snapshots
                .insert(uid, Self::watch_facts(&entry.public));
        }
        self.taken.insert(uid, row);
        Some(entry.into_entity())
    }

    /// 放回 tick 过的行，占回原来的槽位。
    pub fn put(&mut self, entity: Entity) {
        let uid = entity.id;
        let split = entity.split();
        if let Some(before) = self.taken_snapshots.remove(&uid) {
            let after = Self::watch_facts(&split.public);
            if before != after {
                self.note_entity_change(after.0);
            }
        }
        let row = self
            .taken
            .remove(&uid)
            .unwrap_or_else(|| self.allocate_slot());
        self.replace_row(row, split, false);
        self.index_sub_from_row(uid, row);
        self.index.insert(uid, row);
        if let Some(chunk) = self.slots[row]
            .as_ref()
            .map(|entry| entry.public.chunk_position())
        {
            self.chunk_index_add(uid, chunk);
        }
    }

    fn replace_row(&mut self, row: usize, new: EntityRow, note: bool) {
        let old = self.slots[row].take();
        // 这一行在主索引里（uid 指向的就是这一行）。行和 uid 一一对应，所以和「有没有哪个
        // uid 指向这一行」是一回事，只是不用扫整张索引。
        // This row is indexed (its uid points at it). Rows and uids are one to
        // one, so this equals "some uid points at this row" without a scan.
        let indexed = old
            .as_ref()
            .map(|entry| entry.public.uid)
            .filter(|uid| self.index.get(uid) == Some(&row));
        if let (Some(uid), Some(old)) = (indexed, old.as_ref()) {
            let (from, to) = (old.public.chunk_position(), new.public.chunk_position());
            if from != to {
                self.chunk_index_remove(uid, from);
                self.chunk_index_add(uid, to);
            }
        }
        if let Some(uid) = indexed {
            let same_kind = old.as_ref().is_some_and(|entry| {
                entry.public.kind.type_index() == new.public.kind.type_index()
                    && is_hard_colliding(&entry.public) == is_hard_colliding(&new.public)
            });
            if !same_kind {
                self.by_type
                    .get_mut(&old.as_ref().expect("刚才有值").public.kind.type_index())
                    .expect("类型索引和主表一致")
                    .remove(&uid);
                self.hard_colliding.remove(&uid);
                self.index_sub(uid, &new.public);
            }
        }
        if note && !self.watched_sections.is_empty() {
            let now = Self::watch_facts(&new.public);
            let before = old.as_ref().map(|old| Self::watch_facts(&old.public));
            if before.as_ref() != Some(&now) {
                self.note_entity_change(now.0);
            }
        }
        let changed = old.as_ref().is_none_or(|old| {
            section_range(&old.public.bounding_box()) != section_range(&new.public.bounding_box())
        });
        if changed {
            if let Some(old) = &old {
                self.grid_remove(row, &old.public);
            }
            self.grid_insert(row, &new.public);
        }
        self.slots[row] = Some(new);
    }

    fn index_sub(&mut self, uid: EntityUid, public: &EntityPublic) {
        self.by_type
            .entry(public.kind.type_index())
            .or_default()
            .insert(uid);
        if is_hard_colliding(public) {
            self.hard_colliding.insert(uid);
        }
    }

    fn unindex_sub(&mut self, uid: EntityUid) {
        if let Some(entry) = self
            .index
            .get(&uid)
            .and_then(|&row| self.slots[row].as_ref())
        {
            let key = entry.public.kind.type_index();
            if let Some(uids) = self.by_type.get_mut(&key) {
                uids.remove(&uid);
                if uids.is_empty() {
                    self.by_type.remove(&key);
                }
            }
        } else {
            // 行已经取走时类型未知；全套取出/放回路径都会在放回时重新登记。
            for uids in self.by_type.values_mut() {
                uids.remove(&uid);
            }
            self.by_type.retain(|_, uids| !uids.is_empty());
        }
        self.hard_colliding.remove(&uid);
    }

    fn index_sub_from_row(&mut self, uid: EntityUid, row: usize) {
        let Some((type_index, hard)) = self.slots.get(row).and_then(Option::as_ref).map(|entry| {
            (
                entry.public.kind.type_index(),
                is_hard_colliding(&entry.public),
            )
        }) else {
            return;
        };
        self.by_type.entry(type_index).or_default().insert(uid);
        if hard {
            self.hard_colliding.insert(uid);
        }
    }

    /// 同步关注段集合。方块实体表里的登记变了才调，空集合时以后每次写入只多一个分支。
    pub fn set_watched_sections(&mut self, sections: HashSet<(i32, i32, i32)>) {
        self.watched_sections = sections;
    }

    /// 取走这一刻刚进世界的实体快照（uid 升序），同步层用它建跟踪状态。
    ///
    /// Take this tick's spawn snapshots in uid order for the sync layer.
    pub fn take_spawn_snapshots(&mut self) -> Vec<EntityPublic> {
        std::mem::take(&mut self.spawn_snapshots)
            .into_values()
            .collect()
    }

    /// 取走这一刻攒下的实体位置事件，按生成顺序。
    pub fn take_wake_events(&mut self) -> Vec<Aabb> {
        std::mem::take(&mut self.wake_events)
    }

    /// 取走这一刻生物的实体事件（受伤、死亡、音效），按 uid 升序、实体内按发生顺序。
    /// 发包方每刻调用一次，事件不会跨刻累积。
    ///
    /// Take this tick's mob events (damage, death, sound) in uid order and event
    /// order within each entity. The sender calls this once per tick.
    pub fn take_mob_events(&mut self) -> Vec<(EntityUid, Vec<super::mob::MobEvent>)> {
        let mut out = Vec::new();
        let take = |row: &mut EntityRow| -> Option<(EntityUid, Vec<super::mob::MobEvent>)> {
            match &mut row.state.kind {
                super::KindState::Mob(mob) => (!mob.events.is_empty())
                    .then(|| (row.public.uid, std::mem::take(&mut mob.events))),
                // 药水箭褪色的 `broadcastEntityEvent(this, 0)` 走同一条事件通道。
                // A tipped arrow's fading `broadcastEntityEvent(this, 0)` shares this channel.
                super::KindState::Arrow(arrow) => {
                    super::arrow::take_potion_puff(arrow).then(|| {
                        (
                            row.public.uid,
                            vec![super::mob::MobEvent::EntityEvent { event: 0 }],
                        )
                    })
                }
                super::KindState::Egg(egg) => super::thrown_egg::take_cracked(egg).then(|| {
                    (
                        row.public.uid,
                        vec![super::mob::MobEvent::EntityEvent { event: 3 }],
                    )
                }),
                _ => None,
            }
        };
        for &row in self.index.values() {
            if let Some(entry) = self.slots[row].as_mut()
                && let Some(events) = take(entry)
            {
                out.push(events);
            }
        }
        // tick 期间新生成的实体也在这一刻发事件，按 uid 排在表里的实体之后。
        for row in &mut self.pending {
            if let Some(events) = take(row) {
                out.push(events);
            }
        }
        out.extend(
            std::mem::take(&mut self.departed_mob_events)
                .into_iter()
                .map(|(uid, (_, events))| (uid, events)),
        );
        out
    }

    /// 实体这一拍被丢弃时，把它身上攒下的事件留给同步层。
    ///
    /// Keep a discarded entity's pending events for the sync layer.
    pub fn keep_departed_events(&mut self, entity: &Entity) {
        // 鸡蛋命中的那一刻就消失；`broadcastEntityEvent(this, 3)` 跟死亡事件走同一条路。
        // An egg vanishes on its impact tick; event 3 takes the same path as death events.
        if let EntityKind::Egg(egg) = &entity.kind
            && egg.is_cracked()
        {
            self.departed_mob_events.insert(
                entity.id,
                (
                    ChunkPos::containing(entity.position.x, entity.position.z),
                    vec![super::mob::MobEvent::EntityEvent { event: 3 }],
                ),
            );
        }
        if let Some(mob) = entity.kind.mob()
            && !mob.events.is_empty()
        {
            self.departed_mob_events.insert(
                entity.id,
                (
                    ChunkPos::containing(entity.position.x, entity.position.z),
                    mob.events.clone(),
                ),
            );
        }
    }

    /// 这个包围盒摸到的段里有没有人关注；顺带把「变化」记进事件表。
    /// 没有关注时直接返回，实体移动的热路径不受影响。
    fn note_entity_change(&mut self, area: Aabb) {
        if self.watched_sections.is_empty() {
            return;
        }
        let (first, last) = section_range(&area);
        for x in first.0..=last.0 {
            for y in first.1..=last.1 {
                for z in first.2..=last.2 {
                    if self.watched_sections.contains(&(x, y, z)) {
                        self.wake_events.push(area);
                        return;
                    }
                }
            }
        }
    }

    /// 一行的公开数据里，方块实体休眠关心的部分：位置和物品堆。
    fn watch_facts(public: &EntityPublic) -> (Aabb, Option<ItemStack>) {
        let stack = match &public.kind {
            KindPublic::Item { stack, .. } => Some(stack.clone()),
            _ => None,
        };
        (public.bounding_box(), stack)
    }

    /// 岛合并时把各实体记着的绝对时刻平移 `delta`。生物身上的受伤时刻、
    /// 导航的卡住检查和重算时刻，以及降频的三个时刻都要跟着走。
    /// tick 中被拿走的行不在表里，合并只在两刻之间发生，所以不会漏。
    pub fn rebase(&mut self, delta: i64) {
        let rows = self.slots.iter_mut().flatten().chain(&mut self.pending);
        for row in rows {
            if let KindState::Mob(mob) = &mut row.state.kind {
                mob.rebase(delta);
            }
        }
        self.wake_tokens.rebase(delta);
    }

    /// 三档降频的限量唤醒令牌桶。
    pub fn wake_tokens(&self) -> super::mob::throttle::WakeTokens {
        self.wake_tokens
    }

    /// 一刻开头推进令牌桶之后把状态写回来。
    pub fn set_wake_tokens(&mut self, tokens: super::mob::throttle::WakeTokens) {
        self.wake_tokens = tokens;
    }

    /// 只对一行做一次可变借用，给 [`super::mob::throttle::plans_for_tick`] 用：
    /// 不改槽位、不动任何子索引，公开的 uid 和位置都不变。
    pub fn with_throttle_row_mut(
        &mut self,
        row: usize,
        f: &mut dyn FnMut(&mut EntityPublic, &mut EntityState),
    ) {
        if let Some(slot) = self.slots.get_mut(row).and_then(Option::as_mut) {
            f(&mut slot.public, &mut slot.state);
        }
    }

    pub fn begin_tick(&mut self) {
        self.ticking = true;
    }

    /// tick 结束时把这次生成的实体并进来，它们下一 tick 才 tick；同时回收这一拍
    /// 被 `discard` 的实体留下的借用槽位。精确模式 `World::tick_entities` 把实体
    /// `take` 出去，tick 中被丢弃的实体既不 `put` 回来也不会再 `remove`
    /// （`take` 已经把它从 `index` 摘掉），于是 `taken` 里会永久留着一行、
    /// `slots[row]` 也一直是空的，让 `ready_to_move_chunks` 永远为假、区块收回
    /// 全卡在 "not ready"。这里统一把这些行从 `taken` 摘掉并放回 `free`。
    ///
    /// Merge the entities spawned this tick (they tick next tick) and release the
    /// borrowed slots of entities discarded this tick. The precise driver takes an
    /// entity out; a discarded one is never put back or removed (take already
    /// dropped it from `index`), so its `taken` entry and empty slot would leak
    /// forever, leaving `ready_to_move_chunks` permanently false and stalling every
    /// chunk reclaim. Return those rows to `free` so they can be reused.
    pub fn end_tick(&mut self) {
        self.ticking = false;
        // 这一拍被 `discard` 的实体不会 `put` 回来，旧位置快照在这里统一丢掉。
        self.taken_snapshots.clear();
        for (_, row) in std::mem::take(&mut self.taken) {
            self.free.push(row);
        }
        let pending = std::mem::take(&mut self.pending);
        for row in pending {
            self.insert_row(row);
        }
        debug_assert!(self.sub_index_matches(), "实体子索引和主表不一致");
    }

    /// 按 `tick_rows()` 的顺序，每个实体属于哪一棵骑乘树的根（给的是根在名单里的
    /// 下标）。没有载具、载具已离表、或链条成环时自己是根。精确驱动的降频计划和
    /// 并行驱动的分组都用这一份，保证两种驱动对同一棵树给同一个计划、同一份活。
    ///
    /// For each entity in `tick_rows()` order, the index of the root of its
    /// riding tree. An entity with no vehicle, a missing vehicle or a cyclic
    /// chain is its own root. Both drivers use this to give one tree one plan
    /// and one unit of work.
    pub fn riding_roots(&self) -> Vec<usize> {
        let (uids, rows) = self.tick_rows();
        let mut by_uid = std::collections::BTreeMap::new();
        for (index, &uid) in uids.iter().enumerate() {
            by_uid.insert(uid, index);
        }
        let mut out = Vec::with_capacity(uids.len());
        for index in 0..uids.len() {
            let mut current = index;
            let mut seen = std::collections::BTreeSet::new();
            while let Some(&next) = self
                .slots
                .get(rows[current])
                .and_then(Option::as_ref)
                .and_then(|entry| entry.public.vehicle)
                .and_then(|vehicle| by_uid.get(&vehicle))
            {
                if next == index || !seen.insert(next) {
                    // 链条绕回自己：坏数据不 panic，整棵树按当前节点算根。
                    current = next;
                    break;
                }
                current = next;
            }
            out.push(current);
        }
        out
    }

    /// 并行算阶段的只读名单：uid 升序的 ID 和对应的槽位行号。
    /// 公开数据和空间索引直接引用表本身，不再复制一份。
    pub fn tick_rows(&self) -> (Vec<EntityUid>, Vec<usize>) {
        let (mut uids, mut rows) = (
            Vec::with_capacity(self.index.len()),
            Vec::with_capacity(self.index.len()),
        );
        for (&uid, &row) in &self.index {
            uids.push(uid);
            rows.push(row);
        }
        (uids, rows)
    }

    /// 只读的一个空间索引格子。算阶段的范围查询只访问覆盖到的那几个格子。
    pub fn grid_section(&self, key: (i32, i32, i32)) -> Option<&BTreeSet<usize>> {
        self.grid.get(&key)
    }

    /// 现在能不能按区块搬实体：不在 tick 中途，也没有被 `take` 借走的实体。
    ///
    /// 和 [`BlockTicks::ready_to_move_chunks`](crate::BlockTicks::ready_to_move_chunks)
    /// 一样，调用方在 `take_chunk` 之前先问，好让整批区块要么都搬、要么都不搬。
    pub fn ready_to_move_chunks(&self) -> bool {
        !self.ticking && self.taken.is_empty()
    }

    /// 把 `chunk` 里的实体装进盒子搬走。只在 tick 边界调用，tick 中途返回 [`Busy`]。
    pub fn take_chunk(&mut self, chunk: ChunkPos) -> Result<ChunkEntities, Busy> {
        if !self.ready_to_move_chunks() {
            return Err(Busy);
        }
        let taken = self.uids_in_chunk(chunk);
        let mut rows = Vec::with_capacity(taken.len());
        for uid in taken {
            self.unindex_sub(uid);
            let row = self.index.remove(&uid).expect("just indexed");
            self.taken.remove(&uid);
            let entry = self.slots[row].take().expect("indexed row is live");
            self.chunk_index_remove(uid, chunk);
            self.grid_remove(row, &entry.public);
            self.free.push(row);
            rows.push(entry);
        }
        rows.sort_by_key(|entry| entry.public.uid);
        Ok(ChunkEntities {
            rows,
            preserved: self.take_preserved(chunk),
            from_disk: false,
        })
    }

    /// 只读快照：直接复制这一区块的实体行和保留条目，原表分毫不动（补 11）。
    /// 不要求 tick 边界，也不要求没有实体被借走——存档只编内容，不搬运行态。
    /// Read-only snapshot: copy this chunk's entity rows and preserved entries
    /// without requiring a tick boundary. Saving encodes content, never moves
    /// the live tables.
    pub fn snapshot_chunk(&self, chunk: ChunkPos) -> Option<ChunkEntities> {
        let rows: Vec<EntityRow> = self
            .uids_in_chunk(chunk)
            .into_iter()
            .filter_map(|uid| self.slots[*self.index.get(&uid)?].as_ref())
            .cloned()
            .collect();
        let preserved = self
            .preserved
            .iter()
            .filter(|tag| {
                let [x, _, z] = crate::archive::tag::vec3(tag, "Pos", [0.0; 3]);
                chunk == ChunkPos::new(x.floor() as i32 >> 4, z.floor() as i32 >> 4)
            })
            .cloned()
            .collect();
        Some(ChunkEntities {
            rows,
            preserved,
            from_disk: false,
        })
    }

    /// 岛拆分（岛的实施计划 3.3「实体表整批搬」）：把位置落在 `chunks` 里的实体整批搬进
    /// 一张新表，只扫一遍主表，和要搬几个区块无关。两边都按 uid 保序；搬过去的行在新表
    /// 里重新分配行号（行号只是槽位，外面看不见）。ID 分配器是同一个，不重新取号。
    /// 不认识的保留条目按 `Pos` 分；关注段集合两边各抄一份（多出来的段只会多报一次
    /// 位置事件，方块实体表按真实的关注盒再筛）；还没交出去的位置事件按盒子所在区块分。
    /// 只在 tick 边界调用，tick 中途返回 [`Busy`]。
    ///
    /// Island split (island plan 3.3): move every entity whose position lies in
    /// `chunks` into a new store in one pass over the table, independent of the
    /// number of chunks. Both sides keep uid order; moved rows get new slots in
    /// the new store. The id allocator is shared, nothing is re-allocated.
    /// Preserved entries split by `Pos`; watched sections are copied to both
    /// sides (a superset only reports extra events that the block-entity table
    /// filters); undelivered wake events split by chunk. Tick boundary only.
    pub fn split_off(&mut self, chunks: &BTreeSet<ChunkPos>, seeds: i64) -> Result<Self, Busy> {
        if !self.ready_to_move_chunks() {
            return Err(Busy);
        }
        debug_assert!(self.pending.is_empty(), "tick 边界上没有待并入的实体");
        debug_assert!(self.sub_index_matches(), "实体子索引和主表不一致");
        // 按区块的子索引只看要搬的区块；按 uid 升序，和按主表顺序筛出来的一样。
        // The chunk sub-index visits only the moved chunks; sorted by uid, the
        // same as filtering the main table in order.
        let mut moved: Vec<EntityUid> = chunks
            .iter()
            .filter_map(|chunk| self.by_chunk.get(chunk))
            .flatten()
            .copied()
            .collect();
        moved.sort_unstable();
        // 载具和乘客必须去同一个岛：先按区块选出要走的，再沿 `vehicle` /
        // `Passengers` 两边补全整组。任何一头落在要搬的区块里，整组都搬。
        // Vehicles and riders must stay on one island: seed from the chunks, then
        // close the set over both `vehicle` and `Passengers` links.
        if !moved.is_empty() {
            let mut neighbours: BTreeMap<EntityUid, Vec<EntityUid>> = BTreeMap::new();
            let mut link = |a: EntityUid, b: EntityUid| {
                if a != b && self.index.contains_key(&a) && self.index.contains_key(&b) {
                    neighbours.entry(a).or_default().push(b);
                    neighbours.entry(b).or_default().push(a);
                }
            };
            for (&uid, &row) in &self.index {
                let Some(entry) = self.slots[row].as_ref() else {
                    continue;
                };
                if let Some(vehicle) = entry.public.vehicle {
                    link(vehicle, uid);
                }
                if let KindPublic::Mob(mob) = &entry.public.kind {
                    for &passenger in &mob.passengers {
                        if passenger >= FIRST_ENTITY_ID {
                            link(uid, passenger);
                        }
                    }
                }
            }
            let mut seen: BTreeSet<EntityUid> = moved.iter().copied().collect();
            let mut stack = moved.clone();
            while let Some(uid) = stack.pop() {
                if let Some(list) = neighbours.get(&uid) {
                    for &next in list {
                        if seen.insert(next) {
                            moved.push(next);
                            stack.push(next);
                        }
                    }
                }
            }
            moved.sort_unstable();
        }
        // 新岛的种子源由调用方给（岛这一层按世界种子、起源 section 和建岛时刻派生，
        // 计划 3.3「岛的随机源」），不掺原岛种子源的状态；原岛的不动（拆了马上合回来
        // 逐位相同）。
        // The caller supplies the new island's seed source (derived by the island
        // layer from the world seed, origin section and creation time, plan 3.3),
        // without the old source's state; the old source is untouched.
        let seeds = JavaRandom::new(seeds);
        let mut out = Self {
            ids: self.ids.clone(),
            seeds,
            watched_sections: self.watched_sections.clone(),
            // 拆岛时两边都从满桶开始：令牌数只由刻号和类别决定，和生物数无关。
            wake_tokens: self.wake_tokens.split_full(),
            ..Self::default()
        };
        // 还没交给同步层的生成快照跟着实体走：同步层在实体所在的岛上按它建跟踪状态。
        // 实体已经不在表里的快照按快照里的位置分。
        // Spawn snapshots not yet handed to the sync layer follow their entity,
        // whose island builds the tracking state from them; snapshots of
        // entities no longer in the table split by their own position.
        // 只扫键、挑出要走的再逐个取出，不重建整张表（这张表只有这一刻新生成的实体，
        // 和岛的大小无关）。
        // Scan keys only and take out the moving ones instead of rebuilding the
        // table (it holds only this tick's new entities, independent of size).
        let going: Vec<EntityUid> = self
            .spawn_snapshots
            .iter()
            .filter(|(uid, public)| {
                if self.index.contains_key(uid) {
                    moved.binary_search(uid).is_ok()
                } else {
                    chunks.contains(&public.chunk_position())
                }
            })
            .map(|(&uid, _)| uid)
            .collect();
        for uid in going {
            let public = self.spawn_snapshots.remove(&uid).expect("刚看过");
            out.spawn_snapshots.insert(uid, public);
        }
        // 被丢弃生物留下的事件按生物离场时所在的区块分：它的行已经离表，`moved` 里
        // 不会有它的 uid（原来按 uid 分，结果是永远留在原岛）。看着它的玩家和那个区块在
        // 同一边（视野不跨岛），同步层在那一边把事件发给他们。
        // Events left by discarded mobs split by the chunk the mob was in when
        // it left: its row is gone, so its uid is never among `moved` (splitting
        // by uid left the events behind every time). The players watching it
        // are on the same side as that chunk (no view spans islands), and that
        // side's sync layer sends the events to them.
        let going: Vec<EntityUid> = self
            .departed_mob_events
            .iter()
            .filter(|(_, (chunk, _))| chunks.contains(chunk))
            .map(|(&uid, _)| uid)
            .collect();
        for uid in going {
            let entry = self.departed_mob_events.remove(&uid).expect("刚看过");
            out.departed_mob_events.insert(uid, entry);
        }
        for &uid in &moved {
            self.unindex_sub(uid);
            let row = self.index.remove(&uid).expect("just indexed");
            let entry = self.slots[row].take().expect("indexed row is live");
            self.chunk_index_remove(uid, entry.public.chunk_position());
            self.grid_remove(row, &entry.public);
            self.free.push(row);
            out.insert_row_quietly(entry);
        }
        let (moved, kept): (Vec<Tag>, Vec<Tag>) = std::mem::take(&mut self.preserved)
            .into_iter()
            .partition(|tag| chunks.contains(&preserved_chunk(tag)));
        self.preserved = kept;
        out.preserved = moved;
        let (moved, kept): (Vec<Aabb>, Vec<Aabb>) = std::mem::take(&mut self.wake_events)
            .into_iter()
            .partition(|area| {
                chunks.contains(&ChunkPos::new(
                    (area.min_x.floor() as i32) >> 4,
                    (area.min_z.floor() as i32) >> 4,
                ))
            });
        self.wake_events = kept;
        out.wake_events = moved;
        // 不变量：拆完以后两张表之间不能再有骑乘引用。
        #[cfg(debug_assertions)]
        {
            self.debug_assert_no_links_to(&out);
            out.debug_assert_no_links_to(self);
        }
        Ok(out)
    }

    /// 调试断言：本表里的实体没有引用 `other` 里的实体（双向）。只查两边都还在表里的
    /// uid，已经删掉的悬空引用不算跨岛。
    ///
    /// Debug assertion that no entity here links to an entity in `other`, in
    /// either direction. Only live ids on both sides count; dangling links to
    /// removed entities are not cross-island references.
    #[cfg(debug_assertions)]
    fn debug_assert_no_links_to(&self, other: &Self) {
        for (&uid, &row) in &self.index {
            let Some(entry) = self.slots[row].as_ref() else {
                continue;
            };
            if let Some(vehicle) = entry.public.vehicle {
                debug_assert!(
                    !other.index.contains_key(&vehicle),
                    "rider {uid} and its vehicle {vehicle} ended up on different islands"
                );
            }
            if let KindPublic::Mob(mob) = &entry.public.kind {
                for &passenger in &mob.passengers {
                    if passenger >= FIRST_ENTITY_ID {
                        debug_assert!(
                            !other.index.contains_key(&passenger),
                            "vehicle {uid} and its passenger {passenger} ended up on different islands"
                        );
                    }
                }
            }
        }
    }

    /// 岛合并：[`Self::split_off`] 的逆操作。另一张表的实体按 uid 升序放进来，行号
    /// 重新分配；两张表必须共用同一个 ID 分配器。只在 tick 边界调用。
    ///
    /// Island merge: the inverse of [`Self::split_off`]. The other table's rows
    /// go in by ascending uid with new slots; both tables share one allocator.
    pub fn absorb(&mut self, mut other: Self) {
        debug_assert!(self.ready_to_move_chunks() && other.ready_to_move_chunks());
        debug_assert!(other.pending.is_empty(), "tick 边界上没有待并入的实体");
        debug_assert!(
            self.ids.same_counter(&other.ids),
            "合并的两张实体表必须共用一个 ID 分配器"
        );
        let rows: Vec<usize> = other.index.values().copied().collect();
        for row in rows {
            let entry = other.slots[row].take().expect("indexed row is live");
            debug_assert!(!self.index.contains_key(&entry.public.uid), "实体 ID 重复");
            self.insert_row_quietly(entry);
        }
        self.preserved.append(&mut other.preserved);
        self.watched_sections.extend(other.watched_sections);
        // 合并：两个岛的刻号已经对齐，按类别取两份里剩余令牌较多的那份。
        self.wake_tokens.absorb(other.wake_tokens);
        self.wake_events.append(&mut other.wake_events);
        for (uid, entry) in std::mem::take(&mut other.departed_mob_events) {
            let previous = self.departed_mob_events.insert(uid, entry);
            debug_assert!(previous.is_none(), "离场生物事件的实体 ID 重复");
        }
        for (uid, public) in std::mem::take(&mut other.spawn_snapshots) {
            let previous = self.spawn_snapshots.insert(uid, public);
            debug_assert!(previous.is_none(), "生成快照的实体 ID 重复");
        }
    }

    /// 还没交给同步层的生成快照：实体 ID 和快照所在的区块，uid 升序。拆合核对用。
    ///
    /// Spawn snapshots not yet handed to the sync layer, as uid and the
    /// snapshot's chunk in ascending uid order; for split checks.
    pub fn spawn_snapshot_positions(&self) -> impl Iterator<Item = (EntityUid, ChunkPos)> + '_ {
        self.spawn_snapshots
            .iter()
            .map(|(&uid, public)| (uid, public.chunk_position()))
    }

    /// 离场生物留下、还没交给同步层的事件：实体 ID 和生物离场时所在的区块，uid 升序。
    /// 拆合核对用。
    ///
    /// Events left by departed mobs and not yet handed to the sync layer, as
    /// uid and the chunk the mob left from, in ascending uid order; for split
    /// checks.
    pub fn departed_event_positions(&self) -> impl Iterator<Item = (EntityUid, ChunkPos)> + '_ {
        self.departed_mob_events
            .iter()
            .map(|(&uid, (chunk, _))| (uid, *chunk))
    }

    /// 改坏确认专用：把 `other` 的离场生物事件全部挪回自己（模拟拆分时漏搬这一类）。
    ///
    /// Break-it checks only: move all of `other`'s departed-mob events back
    /// here, simulating a split that leaves this class behind.
    #[doc(hidden)]
    pub fn reclaim_departed_events(&mut self, other: &mut Self) {
        self.departed_mob_events
            .append(&mut std::mem::take(&mut other.departed_mob_events));
    }

    /// 测试专用：直接放一条离场生物的事件（平时这张表只在实体阶段到同步阶段之间有
    /// 东西，tick 边界上是空的；拆合的测试要它在拆分那一刻不空）。
    ///
    /// Test only: add one departed-mob entry directly (the table normally
    /// holds entries only between the entity phase and the sync phase and is
    /// empty at a tick boundary; split tests need it non-empty at the split).
    #[doc(hidden)]
    pub fn keep_departed_events_at(
        &mut self,
        uid: EntityUid,
        chunk: ChunkPos,
        events: Vec<super::mob::MobEvent>,
    ) {
        self.departed_mob_events.insert(uid, (chunk, events));
    }

    /// 改坏确认专用：把 `other` 的生成快照全部挪回自己（模拟拆分时漏搬这一类）。
    ///
    /// Break-it checks only: move all of `other`'s spawn snapshots back here,
    /// simulating a split that leaves this class behind.
    #[doc(hidden)]
    pub fn reclaim_spawn_snapshots(&mut self, other: &mut Self) {
        self.spawn_snapshots
            .append(&mut std::mem::take(&mut other.spawn_snapshots));
    }

    /// 把盒子里的实体放回表里。调用方保证表里已经没有这个区块的实体。
    /// 不认识的实体类型不进表，只在 `take_chunk` 时原样带回盒子。
    /// 刚读盘的盒子按行的先后重新取号、取种子（[`ChunkEntities::from_parts`]）。
    ///
    /// Put a box back. Unknown types stay out of the table. A freshly decoded box
    /// takes new ids and seeds in row order.
    pub fn insert_chunk(&mut self, entities: ChunkEntities) {
        for mut row in entities.rows {
            if entities.from_disk {
                row.public.uid = self.ids.allocate();
                row.state.random = JavaRandom::new(self.next_entity_seed());
            }
            self.insert_row(row);
        }
        self.preserved.extend(entities.preserved);
    }

    /// 按区块把保留条目分给盒子。`Pos` 不合法或缺失的条目留在原表，
    /// 下一次仍会写回，不会丢数据。
    fn take_preserved(&mut self, chunk: ChunkPos) -> Vec<Tag> {
        let (taken, kept) = std::mem::take(&mut self.preserved)
            .into_iter()
            .partition(|tag| {
                let x = crate::archive::tag::vec3(tag, "Pos", [0.0; 3])[0];
                let z = crate::archive::tag::vec3(tag, "Pos", [0.0; 3])[2];
                chunk == ChunkPos::new(x.floor() as i32 >> 4, z.floor() as i32 >> 4)
            });
        self.preserved = kept;
        taken
    }
}

/// 保留条目所在的区块，按它的 `Pos`；缺失时按原点算，和 [`EntityStore::take_chunk`] 一致。
fn preserved_chunk(tag: &Tag) -> ChunkPos {
    let [x, _, z] = crate::archive::tag::vec3(tag, "Pos", [0.0; 3]);
    ChunkPos::new(x.floor() as i32 >> 4, z.floor() as i32 >> 4)
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

/// `SectionPos.asLong(x, y, z)`：`x << 42 | z << 20 | y`（各轴按官方位宽掩码）。
fn section_key((x, y, z): (i32, i32, i32)) -> i64 {
    ((i64::from(x) & 0x3F_FFFF) << 42)
        | ((i64::from(z) & 0x3F_FFFF) << 20)
        | (i64::from(y) & 0xF_FFFF)
}

pub(crate) fn section_range(area: &Aabb) -> ((i32, i32, i32), (i32, i32, i32)) {
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

    /// 精确模式里被 `discard` 的实体 `take` 出去后不再 `put` 回来；`end_tick`
    /// 必须把这些借用行回收，否则 `taken` 永远非空、`ready_to_move_chunks`
    /// 永远为假，所有区块收回都会卡在 "not ready"。
    ///
    /// In the precise driver a discarded entity is taken out and never put back;
    /// `end_tick` must release those borrowed rows or `ready_to_move_chunks`
    /// stays false forever and every chunk reclaim stalls.
    #[test]
    fn discarded_entities_do_not_leave_borrowed_rows_at_tick_end() {
        let mut store = EntityStore::new();
        let kept = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let discarded = store.spawn(Entity::ground_item(
            Vec3::new(40.5, 64.0, 40.5),
            ItemStack::new(2, 1),
        ));
        assert!(store.ready_to_move_chunks());

        store.begin_tick();
        let alive = store.take(kept).unwrap();
        let gone = store.take(discarded).unwrap();
        assert!(!store.ready_to_move_chunks(), "借出去的实体还没放回");
        store.put(alive);
        // 这一拍被 discard：既不 put 也不 remove，和 World::tick_entities 一致。
        // Discarded this tick: neither put back nor removed, like World::tick_entities.
        drop(gone);
        store.end_tick();

        assert!(
            store.ready_to_move_chunks(),
            "被丢弃的实体不能让 ready_to_move_chunks 永远为假"
        );
        assert!(store.public_at(discarded).is_none(), "丢弃的实体不在表里");
        assert_eq!(store.public_at(kept).map(|public| public.uid), Some(kept));
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
        let (uids, rows) = store.tick_rows();
        assert_eq!(uids, [first, second]);
        let mut public: Vec<_> = rows
            .iter()
            .map(|&row| store.row_at(row).unwrap().public.clone())
            .collect();
        let state: Vec<_> = rows
            .iter()
            .map(|&row| store.row_at(row).unwrap().state.clone())
            .collect();
        public[0].position.x += 1.0;
        store.publish_rows(&rows, public, state);
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

    /// 离场生物留下的事件在岛拆分时按生物离场的区块分，合并时并回来；取走时仍按 uid
    /// 升序。它的行已经不在表里，按「搬走的 uid」分的话永远留在原岛。
    ///
    /// Events left by departed mobs split by the chunk the mob left from and
    /// merge back; they still come out in uid order. The row is gone, so
    /// splitting by moved uids would always leave them behind.
    #[test]
    fn departed_mob_events_split_by_the_chunk_the_mob_left_from() {
        use crate::entity::mob::MobEvent;

        let mut store = EntityStore::default();
        let (here, there) = (ChunkPos::new(0, 0), ChunkPos::new(40, 0));
        store.keep_departed_events_at(9, here, vec![MobEvent::Death]);
        store.keep_departed_events_at(5, there, vec![MobEvent::Poof]);
        store.keep_departed_events_at(7, there, vec![MobEvent::Hearts]);
        let mut far = store
            .split_off(&BTreeSet::from([there]), 1)
            .expect("tick 边界上可以拆");
        let positions = |store: &EntityStore| -> Vec<(EntityUid, ChunkPos)> {
            store.departed_event_positions().collect()
        };
        assert_eq!(positions(&store), [(9, here)]);
        assert_eq!(positions(&far), [(5, there), (7, there)]);
        let taken: Vec<EntityUid> = far
            .take_mob_events()
            .into_iter()
            .map(|(uid, _)| uid)
            .collect();
        assert_eq!(taken, [5, 7]);
        far.keep_departed_events_at(5, there, vec![MobEvent::Poof]);
        store.absorb(far);
        assert_eq!(positions(&store), [(5, there), (9, here)]);
    }

    /// 参照 `BlockEntities::take_chunk`：盒子能跨线程搬，实体按 uid 排序，
    /// 不认识的实体类型按区块原样保留，随盒子取出放入，不进实体表。
    #[test]
    fn unknown_entity_entries_follow_the_chunk_box() {
        use mc_bamboo_nbt::Tag;

        let mut store = EntityStore::new();
        let known = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 64.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let cow = Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:cow".to_owned())),
            (
                "Pos".to_owned(),
                Tag::List(vec![Tag::Double(2.5), Tag::Double(64.0), Tag::Double(3.5)]),
            ),
            ("Health".to_owned(), Tag::Float(10.0)),
        ]);
        let alien = Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:alien".to_owned())),
            (
                "Pos".to_owned(),
                Tag::List(vec![Tag::Double(40.5), Tag::Double(64.0), Tag::Double(3.5)]),
            ),
        ]);
        store.insert_chunk(ChunkEntities::from_parts(
            Vec::new(),
            vec![cow.clone(), alien.clone()],
        ));
        assert_eq!(store.ids(), [known]);
        assert!(!store.is_empty());

        let boxed = store.take_chunk(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(boxed.preserved(), std::slice::from_ref(&cow));
        let moved = std::thread::spawn(move || boxed).join().unwrap();
        assert!(!moved.is_empty());
        store.insert_chunk(moved);
        let other = store.take_chunk(ChunkPos::new(2, 0)).unwrap();
        assert_eq!(other.preserved(), &[alien]);
    }

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
    /// 按类型 / 硬碰撞的子索引：取放、移除、整区块搬进搬出都跟主表一致，
    /// 遍历顺序仍是 uid 升序。
    #[test]
    fn sub_indexes_follow_every_take_and_put_path() {
        use mc_bamboo_data::registries::entity_type;

        let mut store = EntityStore::new();
        let falling = store.spawn(Entity::falling_block(
            BlockPos::new(0, 70, 0),
            BlockState(blocks::STONE),
        ));
        let item = store.spawn(Entity::ground_item(
            Vec3::new(0.5, 70.0, 0.5),
            ItemStack::new(1, 1),
        ));
        let tnt = store.spawn(Entity::primed_tnt(Vec3::new(0.5, 70.0, 0.5), 0.25));
        assert!(store.sub_index_matches());
        assert_eq!(
            store
                .uids_of_type(entity_type::FALLING_BLOCK)
                .collect::<Vec<_>>(),
            [falling]
        );
        assert_eq!(
            store.uids_of_type(entity_type::ITEM).collect::<Vec<_>>(),
            [item]
        );
        assert_eq!(
            store.uids_of_type(entity_type::TNT).collect::<Vec<_>>(),
            [tnt]
        );
        assert!(store.hard_colliding_uids().next().is_none());

        // 精确驱动取出来再放回，索引和 uid 顺序不变。
        let taken = store.take(item).expect("物品在表里");
        assert!(store.uids_of_type(entity_type::ITEM).next().is_none());
        assert!(store.sub_index_matches());
        store.put(taken);
        assert_eq!(store.ids(), [falling, item, tnt]);
        assert!(store.sub_index_matches());

        // 移除、区块搬走和搬回也都同步。
        store.remove(tnt);
        assert!(store.uids_of_type(entity_type::TNT).next().is_none());
        assert!(store.sub_index_matches());
        let boxed = store.take_chunk(ChunkPos::new(0, 0)).expect("静止");
        assert!(store.uids_of_type(entity_type::ITEM).next().is_none());
        assert!(store.sub_index_matches());
        store.insert_chunk(boxed);
        assert_eq!(store.ids(), [falling, item]);
        assert!(store.sub_index_matches());

        // 改坏确认：故意漏掉一次登记，断言会抓到不一致。
        store
            .by_type
            .get_mut(&(entity_type::ITEM as usize))
            .unwrap()
            .remove(&item);
        assert!(!store.sub_index_matches());
    }

    /// 载具和乘客按组拆：种子只命中载具或只命中乘客的区块时整组一起走，拆完两边
    /// 都没有跨岛的骑乘引用。
    ///
    /// Vehicles and riders split as one group: a seed that only covers the
    /// vehicle's or only the rider's chunk still moves both.
    #[test]
    fn a_rider_and_its_vehicle_are_split_as_one_group() {
        use crate::entity::mob::MobType;

        for seed_chunk in [ChunkPos::new(0, 0), ChunkPos::new(2, 0)] {
            let mut store = EntityStore::new();
            let chicken = store.spawn(Entity::by_mob_type(
                MobType::Chicken,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            let zombie = store.spawn(Entity::by_mob_type(
                MobType::Zombie,
                Vec3::new(40.5, 64.0, 0.5),
            ));
            store.with_mut(chicken, |entity| {
                entity.with_mob(|_, mob| mob.passengers.push(zombie));
            });
            store.with_mut(zombie, |entity| entity.vehicle = Some(chicken));
            let island = store
                .split_off(&BTreeSet::from([seed_chunk]), 7)
                .expect("静止");
            let mut ids = island.ids();
            ids.sort_unstable();
            assert_eq!(ids, vec![chicken, zombie], "seed {seed_chunk:?}");
            assert!(store.ids().is_empty(), "seed {seed_chunk:?}");
            store.absorb(island);
            assert_eq!(store.ids(), vec![chicken, zombie]);
            let row = store.get(zombie).expect("合并回来了");
            assert_eq!(row.vehicle, Some(chicken));
        }
    }

    fn item_at(x: f64) -> Entity {
        Entity::ground_item(Vec3::new(x, 64.0, 0.5), ItemStack::new(1, 1))
    }

    /// 生成时不取随机数的实体，测种子用。
    fn block_at(x: f64) -> Entity {
        let mut entity = Entity::falling_block(BlockPos::new(0, 64, 0), BlockState(blocks::SAND));
        entity.position = Vec3::new(x, 64.0, 0.5);
        entity
    }

    /// 一张表单独分配时，号的序列和原来表内自带的计数器逐个相同：从
    /// `FIRST_ENTITY_ID` 起每次加一，`spawn` / `spawn_row` / tick 期间进 `pending`
    /// 的都一样。
    ///
    /// A single store allocates exactly like the old per-store counter: from
    /// `FIRST_ENTITY_ID`, one at a time, through `spawn`, `spawn_row` and the
    /// pending path during a tick.
    #[test]
    fn a_single_store_allocates_the_same_sequence_as_before() {
        let mut store = EntityStore::new();
        let mut got = vec![store.spawn(item_at(0.5))];
        got.push(store.spawn_row(item_at(1.5).split()));
        store.begin_tick();
        got.push(store.spawn(item_at(2.5)));
        got.push(store.spawn_row(item_at(3.5).split()));
        store.end_tick();
        got.push(store.spawn(item_at(4.5)));
        let expected: Vec<EntityUid> = (0..5).map(|n| FIRST_ENTITY_ID + n).collect();
        assert_eq!(got, expected);
        assert_eq!(store.ids(), expected);
        assert_eq!(store.ids_handle().peek(), FIRST_ENTITY_ID + 5);
    }

    /// 对拍入口：`with_next_id` 从给定的号起，各自一个新计数器。
    ///
    /// Oracle entry: `with_next_id` starts at the given id on a fresh counter.
    #[test]
    fn with_next_id_starts_its_own_counter() {
        let mut first = EntityStore::with_next_id(100);
        let mut second = EntityStore::with_next_id(100);
        assert_eq!(first.spawn(item_at(0.5)), 100);
        assert_eq!(first.spawn(item_at(1.5)), 101);
        assert_eq!(second.spawn(item_at(0.5)), 100);
        assert!(!first.ids_handle().same_counter(second.ids_handle()));
    }

    /// 两张表共用一个分配器、交替分配时不撞号，合起来正好是一段连续的号。
    ///
    /// Two stores sharing one allocator never collide when they alternate,
    /// and together they cover one contiguous run of ids.
    /// 岛的整批拆合：按区块分走、两边按 uid 保序、子索引和空间查询都对；共用分配器，
    /// 拆开以后两边新生成的实体不撞号；合回来和从没拆过逐项相同。
    #[test]
    fn an_island_split_moves_whole_chunks_and_merges_back() {
        let ids = EntityIds::new();
        let mut store = EntityStore::with_ids(ids.clone());
        // 两个区块交错生成。
        for step in 0..6 {
            let x = if step % 2 == 0 {
                1.5 + f64::from(step)
            } else {
                40.5 + f64::from(step)
            };
            store.spawn(item_at(x));
        }
        let before: Vec<Entity> = store.iter().collect();
        let wanted: BTreeSet<ChunkPos> = [ChunkPos::new(2, 0)].into_iter().collect();
        let seed_state = store.entity_seed_state();
        let mut island = store.split_off(&wanted, 0x1234).unwrap();
        // 拆分不动原岛的种子源；新岛的种子源就是调用方给的那一条，不掺原岛的状态。
        assert_eq!(store.entity_seed_state(), seed_state);
        assert_eq!(island.entity_seed_state(), JavaRandom::new(0x1234).state());
        assert!(store.sub_index_matches() && island.sub_index_matches());
        let rest: Vec<i32> = store.ids();
        let moved: Vec<i32> = island.ids();
        assert_eq!(rest.len() + moved.len(), before.len());
        assert!(
            moved
                .iter()
                .all(|&uid| island.get(uid).unwrap().position.x > 32.0)
        );
        assert!(rest.windows(2).all(|pair| pair[0] < pair[1]));
        let area = Aabb::new(32.0, -100.0, -10.0, 64.0, 400.0, 10.0);
        let mut found = Vec::new();
        island.for_each_in(area, &mut |public| found.push(public.uid));
        assert_eq!(found, moved);
        let mut none = Vec::new();
        store.for_each_in(area, &mut |public| none.push(public.uid));
        assert!(none.is_empty());

        // 两边各生成一个：共用分配器，不撞号。
        let left = store.spawn(item_at(3.5));
        let right = island.spawn(item_at(45.5));
        assert_ne!(left, right);
        store.remove(left);
        island.remove(right);

        store.absorb(island);
        assert!(store.sub_index_matches());
        let after: Vec<Entity> = store.iter().collect();
        assert_eq!(after, before);
    }

    #[test]
    fn two_stores_sharing_an_allocator_never_collide() {
        let ids = EntityIds::new();
        let mut left = EntityStore::with_ids(ids.clone());
        let mut right = EntityStore::with_ids(ids.clone());
        let mut seed = 0x5EED_u64;
        let mut all = Vec::new();
        for n in 0..400 {
            // 一个简单的线性同余数决定这一次在哪张表、走哪条路径。
            // A small LCG picks the store and the path for each allocation.
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let store = if seed >> 63 == 0 {
                &mut left
            } else {
                &mut right
            };
            if n % 50 == 0 {
                if store.ticking {
                    store.end_tick();
                } else {
                    store.begin_tick();
                }
            }
            let x = f64::from(n) + 0.5;
            let uid = if (seed >> 62) & 1 == 0 {
                store.spawn(item_at(x))
            } else {
                store.spawn_row(item_at(x).split())
            };
            all.push(uid);
        }
        if left.ticking {
            left.end_tick();
        }
        if right.ticking {
            right.end_tick();
        }
        let left_ids: BTreeSet<EntityUid> = left.ids().into_iter().collect();
        let right_ids: BTreeSet<EntityUid> = right.ids().into_iter().collect();
        assert!(left_ids.is_disjoint(&right_ids));
        assert!(!left_ids.is_empty() && !right_ids.is_empty());
        let expected: Vec<EntityUid> = (0..400).map(|n| FIRST_ENTITY_ID + n).collect();
        assert_eq!(all, expected);
        assert_eq!(ids.peek(), FIRST_ENTITY_ID + 400);
        assert!(left.ids_handle().same_counter(&ids));
        // 克隆一张表得到的是同一个计数器的句柄。
        // Cloning a store keeps a handle to the same counter.
        assert!(left.clone().ids_handle().same_counter(right.ids_handle()));
    }

    /// 实体随机源的种子只由它在自己岛上出现的先后决定：另一个岛插进来取了多少个网络
    /// ID，都不改变这个岛上第 k 个实体拿到的种子。改坏确认：换回按网络 ID 派生时这条
    /// 测试会失败（两种取号次序下的种子不同）。
    ///
    /// An entity's seed depends only on its order of appearance on its island:
    /// another island taking ids in between does not change the k-th seed.
    #[test]
    fn entity_seeds_do_not_depend_on_network_ids() {
        let seeds_of = |interleave: bool| {
            let ids = EntityIds::new();
            let mut mine = EntityStore::with_ids(ids.clone());
            let mut other = EntityStore::with_ids(ids);
            let mut seeds = Vec::new();
            for n in 0..20 {
                if interleave && n % 3 == 0 {
                    other.spawn(block_at(100.5));
                }
                let uid = mine.spawn(block_at(f64::from(n) + 0.5));
                seeds.push((uid, mine.get(uid).unwrap().random));
            }
            seeds
        };
        let alone = seeds_of(false);
        let shared = seeds_of(true);
        assert_ne!(
            alone.iter().map(|(uid, _)| *uid).collect::<Vec<_>>(),
            shared.iter().map(|(uid, _)| *uid).collect::<Vec<_>>(),
            "交替取号时网络 ID 应该不同"
        );
        assert_eq!(
            alone.iter().map(|(_, random)| *random).collect::<Vec<_>>(),
            shared.iter().map(|(_, random)| *random).collect::<Vec<_>>(),
        );
        // 第 k 个实体的种子就是种子源的第 k 个 `nextLong`。
        let mut source = JavaRandom::new(ENTITY_RANDOM_SEED);
        for (_, random) in &alone {
            assert_eq!(*random, JavaRandom::new(source.next_long()));
        }
    }

    /// 刚读盘的盒子放进表时按行的先后重新取号、取种子（两个盒子的占位 uid 相同也不撞）；
    /// 在岛之间整盒搬的盒子原样放回，号和随机源都不变。
    ///
    /// A freshly decoded box takes new ids and seeds in row order (two boxes with
    /// the same placeholder uids do not collide); a live box keeps both.
    #[test]
    fn disk_boxes_take_fresh_ids_and_seeds_and_live_boxes_keep_theirs() {
        let mut store = EntityStore::new();
        let first = store.spawn(block_at(0.5));
        let placeholder = |x: f64| {
            let mut row = block_at(x).split();
            row.public.uid = FIRST_ENTITY_ID;
            row.state.random = JavaRandom::default();
            row
        };
        store.insert_chunk(ChunkEntities::from_parts(
            vec![placeholder(16.5)],
            Vec::new(),
        ));
        store.insert_chunk(ChunkEntities::from_parts(
            vec![placeholder(32.5)],
            Vec::new(),
        ));
        assert_eq!(store.ids(), [first, first + 1, first + 2]);
        let mut source = JavaRandom::new(ENTITY_RANDOM_SEED);
        for uid in store.ids() {
            assert_eq!(
                store.get(uid).unwrap().random,
                JavaRandom::new(source.next_long())
            );
        }
        let before: Vec<Entity> = store.iter().collect();
        let state = store.entity_seed_state();
        let moved = store.take_chunk(ChunkPos::new(1, 0)).unwrap();
        store.insert_chunk(moved);
        assert_eq!(store.iter().collect::<Vec<_>>(), before);
        assert_eq!(store.entity_seed_state(), state, "整盒搬运不取种子");
    }
}
