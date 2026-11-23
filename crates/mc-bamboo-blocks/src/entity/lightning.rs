//! 闪电，官方 `LightningBolt`。
//!
//! 雷暴里由 [`crate::random_tick::tick_thunder`] 生成，`/summon minecraft:lightning_bolt`
//! 和陷阱马触发也能生成。客户端的闪光次数、种子、存活刻数都在这里；服务端第一刻引火、
//! 激活避雷针、除铜锈，之后每一刻对附近实体调 `thunderHit`。
//!
//! Lightning bolts, vanilla `LightningBolt`. Spawned by a thunder tick, by
//! `/summon`, or when a skeleton trap triggers. The client reads the flash seed
//! and life; the server lights fires, powers lightning rods, cleans copper and
//! applies `thunderHit` to nearby entities.

use std::collections::BTreeSet;

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{BlockAncestor, BlockClass, block_maps::WEATHERING, blocks};

use crate::{
    behaviour::misc::fire,
    entity::{
        Entity, EntityKind, EntityPublic, JavaRandom,
        ctx::{Command, EntityCtx, PlayerView},
        mob::{self, DamageKind, DamageSource, MobType},
    },
    shape::Aabb,
    state::{BlockState, FACING, POWERED},
    world::World,
};

/// `LightningBolt.START_LIFE`，构造时的 `life`。
const START_LIFE: i32 = 2;
/// `LightningBolt.tick` 里 `spawnFire(4)` 的难度门槛：普通或困难。
const FIRE_DIFFICULTY: i32 = 2;
/// 第一刻额外点着的格子数（`spawnFire(4)`）。
const FIRST_FIRE_SPREADS: i32 = 4;
/// 命中实体用的盒：水平 3 格，下方 3 格，上方 9 格（6 + 3）。
const HIT_HORIZONTAL: f64 = 3.0;
const HIT_BELOW: f64 = 3.0;
const HIT_ABOVE: f64 = 9.0;
/// `spawnFire` 的偏移分布半径（`nextInt(3) - 1`）。
const FIRE_OFFSET_BOUND: i32 = 3;
/// `clearCopperOnLightningStrike` 的随机游走参数。
const COPPER_WALK_COUNT: i32 = 3;
const COPPER_STEP_BOUND: i32 = 8;
const COPPER_CUBE_LIMIT: i32 = 10;
const COPPER_CUBE_REACH: i32 = 1;
/// `LightningRodBlock.onLightningStrike` 的激活刻数。
const ROD_ACTIVATION_TICKS: i32 = 8;
/// `LightningRodBlock.onLightningStrike` 发的 3002 世界事件。
const COPPER_CLEAN_EVENT: i32 = 3002;
/// `Entity.igniteForSeconds(8)` 的刻数。
const IGNITE_SECONDS: i32 = 160;
/// `Entity.thunderHit` 的伤害。
const THUNDER_DAMAGE: f32 = 5.0;

/// 闪电自己的状态。没有可同步的元数据；`seed` 由客户端用来画闪电分叉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningBolt {
    /// 构造时的 `nextLong()`，一次新的闪光会重抽。
    pub seed: i64,
    /// 剩余的存活刻数，每刻减一。
    pub life: i32,
    /// 还剩几次闪光。
    pub flashes: i32,
    /// 陷阱马或雷电事件专用的视觉闪电：不引火、不伤实体。
    pub visual_only: bool,
    /// 已经打中过的实体，避免同一道闪电多次伤害。
    pub hit_entities: BTreeSet<i32>,
    /// `blocksSetOnFire`，进度触发器用；Bamboo 目前没有触发器。
    pub blocks_set_on_fire: i32,
    /// 用三叉戟引雷的玩家，进度触发器用。
    pub cause: Option<i32>,
}

impl LightningBolt {
    /// `new LightningBolt(...)`：从实体自己的随机源取种子和闪光次数。
    pub fn new(random: &mut JavaRandom) -> Self {
        Self {
            seed: random.next_long(),
            life: START_LIFE,
            flashes: random.next_int_bounded(3) + 1,
            visual_only: false,
            hit_entities: BTreeSet::new(),
            blocks_set_on_fire: 0,
            cause: None,
        }
    }

    /// `setVisualOnly`。
    pub fn set_visual_only(&mut self, visual_only: bool) {
        self.visual_only = visual_only;
    }

    /// `getStrikePosition()`：`BlockPos.containing(x, y - 1e-6, z)`。
    pub fn strike_position(&self, position: Vec3) -> BlockPos {
        BlockPos::containing(Vec3::new(position.x, position.y - 1.0e-6, position.z))
    }
}

impl Default for LightningBolt {
    fn default() -> Self {
        let mut random = JavaRandom::new(0);
        Self::new(&mut random)
    }
}

/// 造一道闪电实体；构造时的两个数由 [`crate::EntityStore::prepare`] 之后补齐。
pub fn entity(position: Vec3, visual_only: bool) -> Entity {
    let mut bolt = LightningBolt::default();
    bolt.set_visual_only(visual_only);
    Entity::new(EntityKind::LightningBolt(Box::new(bolt)), position)
}

/// 在实体拿到自己的随机源之后重抽构造时的 `seed` 和 `flashes`。
pub fn finish_spawn(entity: &mut Entity) {
    let mut random = entity.random;
    let seed = random.next_long();
    let flashes = random.next_int_bounded(3) + 1;
    entity.random = random;
    if let EntityKind::LightningBolt(bolt) = &mut entity.kind {
        bolt.seed = seed;
        bolt.flashes = flashes;
        bolt.life = START_LIFE;
    }
}

/// `LightningBolt.tick()`。
///
/// 官方 `Entity.tick` 只有 `baseTick()`；Bamboo 的 `common_tick` 已经跑过，这里只做子类那一段。
/// 写方块和取 `level.random` 都走一条意图命令，精确与并行两种驱动的取数顺序一致。
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::LightningBolt(bolt) = &mut me.kind else {
        return;
    };
    let first = bolt.life == START_LIFE;
    bolt.life -= 1;
    let flashes = bolt.flashes;
    let mut command = LightningTick {
        bolt_id: me.id,
        first,
        life: bolt.life,
        flashes,
        position: me.position,
        strike: bolt.strike_position(me.position),
        visual_only: bolt.visual_only,
        hit: std::mem::take(&mut bolt.hit_entities),
        seed: bolt.seed,
        difficulty: ctx.blocks().difficulty(),
        ..LightningTick::default()
    };
    // `spawnFire` 的取数在算阶段按官方顺序展开成偏移表；落地阶段只照表放火。
    let mut random = me.random;
    if first && command.difficulty >= FIRE_DIFFICULTY {
        // 官方 `spawnFire` 开头就按 `visualOnly` 短路，连三个 `nextInt(3)` 都不取。
        if command.visual_only {
            command.fire_rolls = Vec::new();
        } else {
            command.fire_rolls =
                spawn_fire_offsets(me.block_position(), FIRST_FIRE_SPREADS, &mut random);
        }
    }
    if command.life < 0 && flashes != 0 {
        // 官方在判断闪光分支前取一次 `nextInt(10)`，无论是否命中都取。
        let threshold = -random.next_int_bounded(10);
        if command.life < threshold {
            command.life = 1;
            command.flashes = flashes - 1;
            command.seed = random.next_long();
            // 官方闪光时 `spawnFire(0)`，视觉闪电在里面短路，但本格仍不取数。
            command.fire_rolls = vec![me.block_position()];
            command.reseed = true;
        }
    }
    me.random = random;
    if let EntityKind::LightningBolt(bolt) = &mut me.kind {
        bolt.life = command.life;
        bolt.flashes = command.flashes;
        bolt.seed = command.seed;
        bolt.hit_entities = command.hit.clone();
        if command.life < 0 && bolt.flashes == 0 {
            me.discard();
        }
    }
    // 官方 `life < 0 && flashes == 0` 时只调 `discard()`，这一刻剩下的 `life >= 0` 分支
    // 不走；但实体仍在假世界的 tracked 名单里（Rust 这边保留到下一刻再移除）。
    ctx.emit(Command::LightningTick(command), me);
}

/// 一次闪电 tick 的落地事实：算阶段读到的东西和实体随机源取出的偏移。
#[derive(Debug, Clone, PartialEq)]
pub struct LightningTick {
    /// 这道闪电的实体 ID，哞菇用它按 UUID 去重。
    pub bolt_id: i32,
    /// 是不是 `life == 2` 的第一刻（引火、避雷针、铜、游戏事件）。
    pub first: bool,
    pub life: i32,
    pub flashes: i32,
    pub position: Vec3,
    pub strike: BlockPos,
    pub visual_only: bool,
    pub hit: BTreeSet<i32>,
    pub seed: i64,
    /// `spawnFire` 要试的格子：先是本格，再是若干随机偏移格。
    pub fire_rolls: Vec<BlockPos>,
    pub difficulty: i32,
    /// 这次闪光重抽了种子和 `life`。
    pub reseed: bool,
}

impl Default for LightningTick {
    fn default() -> Self {
        Self {
            bolt_id: 0,
            first: false,
            life: START_LIFE,
            flashes: 0,
            position: Vec3::default(),
            strike: BlockPos::new(0, 0, 0),
            visual_only: false,
            hit: BTreeSet::new(),
            seed: 0,
            fire_rolls: Vec::new(),
            difficulty: 2,
            reseed: false,
        }
    }
}

/// `spawnFire(n)` 的取数顺序：本格不取数，之后每次取三个 `nextInt(3)-1`。
fn spawn_fire_offsets(origin: BlockPos, count: i32, random: &mut JavaRandom) -> Vec<BlockPos> {
    let mut out = vec![origin];
    for _ in 0..count {
        let dx = random.next_int_bounded(FIRE_OFFSET_BOUND) - 1;
        let dy = random.next_int_bounded(FIRE_OFFSET_BOUND) - 1;
        let dz = random.next_int_bounded(FIRE_OFFSET_BOUND) - 1;
        // 官方 `BlockPos.offset` 在 i32 上溢出回绕；极端位置直接跳过这个候选格。
        if let (Some(x), Some(y), Some(z)) = (
            origin.x.checked_add(dx),
            origin.y.checked_add(dy),
            origin.z.checked_add(dz),
        ) {
            out.push(BlockPos::new(x, y, z));
        }
    }
    out
}

/// 落地：引火、避雷针、铜除锈、命中实体。
pub fn apply(me: &mut Entity, world: &mut World, mut facts: LightningTick) {
    // 官方 `spawnFire` 先判断 `canSpreadFireAround`，整段短路就不放任何火。
    if !facts.fire_rolls.is_empty() && !facts.visual_only {
        let centre = BlockPos::containing(facts.position);
        if world.can_spread_fire_around(centre) {
            for pos in &facts.fire_rolls {
                let state = fire::state_at(world, *pos);
                if world.block_state(*pos).is_air() && state.can_survive(world, *pos) {
                    world.set_block_and_update(*pos, state);
                    if let EntityKind::LightningBolt(bolt) = &mut me.kind {
                        bolt.blocks_set_on_fire += 1;
                    }
                }
            }
        }
    }
    if facts.first && !facts.visual_only {
        power_lightning_rod(world, facts.strike);
        clear_copper(world, facts.strike);
    }
    if facts.life < 0 || facts.visual_only {
        return;
    }
    // 3 格盒里的实体逐个 `thunderHit`。`hitEntities` 是算阶段读到的旧集合，
    // 这里按 uid 升序补进新集合，和官方 `getEntities` 的顺序一致。
    let area = Aabb::new(
        facts.position.x - HIT_HORIZONTAL,
        facts.position.y - HIT_BELOW,
        facts.position.z - HIT_HORIZONTAL,
        facts.position.x + HIT_HORIZONTAL,
        facts.position.y + HIT_ABOVE,
        facts.position.z + HIT_HORIZONTAL,
    );
    let mut hits: Vec<EntityPublic> = Vec::new();
    world.level().for_each_entity_in(area, &mut |public| {
        if !public.removed && public.uid != facts.bolt_id && !facts.hit.contains(&public.uid) {
            hits.push(public.clone());
        }
    });
    hits.sort_by_key(|public| public.uid);
    let bolt_uuid = crate::archive::entity::entity_uuid(facts.bolt_id).to_be_bytes();
    for public in &hits {
        facts.hit.insert(public.uid);
        thunder_hit(world, public, bolt_uuid);
    }
    // 玩家不在世界实体表里，用只读玩家视图补上 `Entity.thunderHit`。
    let mut players: Vec<PlayerView> = Vec::new();
    world
        .level()
        .for_each_player(&mut |player| players.push(*player));
    players.retain(|player| {
        !facts.hit.contains(&player.uid)
            && area.min_x <= player.position.x
            && player.position.x < area.max_x
            && area.min_y <= player.position.y
            && player.position.y < area.max_y
            && area.min_z <= player.position.z
            && player.position.z < area.max_z
    });
    players.sort_by_key(|player| player.uid);
    for player in players {
        facts.hit.insert(player.uid);
        world.ignite_player_from_mob(player.uid, 1);
        world.hurt_player_from_mob(
            player.uid,
            DamageSource::environment(DamageKind::LightningBolt),
            THUNDER_DAMAGE,
            None,
        );
    }
    if let EntityKind::LightningBolt(bolt) = &mut me.kind {
        bolt.hit_entities = facts.hit;
    }
}

/// `Entity.thunderHit` 的虚分派，按实体种类走覆盖层。
fn thunder_hit(world: &mut World, public: &EntityPublic, bolt_uuid: [u8; 16]) {
    let uid = public.uid;
    let source = DamageSource::environment(DamageKind::LightningBolt);
    let Some(mob_type) = public.kind.mob_type() else {
        // 非生物实体也走 `Entity.thunderHit`；Bamboo 的非生物实体没有血量，只处理着火。
        set_remaining_fire(world, uid, 1);
        return;
    };
    match mob_type {
        MobType::Mooshroom => {
            mob::thunder::mooshroom_hit(world, uid, bolt_uuid);
        }
        MobType::Turtle => {
            // 海龟只受极大伤害，碗由 `entities/turtle` 战利品表的 `#is_lightning` 条件给。
            world.hurt_mob(uid, source, f32::MAX);
        }
        MobType::Creeper => {
            general_thunder_hit(world, uid, source);
            mob::thunder::set_creeper_powered(world, uid);
        }
        MobType::Pig => {
            // 猪变僵尸猪灵不在这批的建模范围，按父类处理，README 记已知限制。
            general_thunder_hit(world, uid, source);
        }
        _ => general_thunder_hit(world, uid, source),
    }
}

/// `Entity.thunderHit` 的基类：加 1 刻火、为 0 时点火 8 秒、受 5 点雷击伤害。
fn general_thunder_hit(world: &mut World, uid: i32, source: DamageSource) {
    if set_remaining_fire(world, uid, 1) == 0 {
        set_remaining_fire(world, uid, IGNITE_SECONDS);
    }
    world.hurt_mob(uid, source, THUNDER_DAMAGE);
}

/// `Entity.setRemainingFireTicks(remainingFireTicks + 1)`，返回设置后的值。
fn set_remaining_fire(world: &mut World, uid: i32, ticks: i32) -> i32 {
    let mut left = 0;
    world.with_entity_mut(uid, &mut |entity| {
        entity.remaining_fire_ticks += ticks;
        left = entity.remaining_fire_ticks;
    });
    left
}

/// `LightningRodBlock.onLightningStrike`：置 `POWERED`，通知背面、排 8 刻、发 3002。
fn power_lightning_rod(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if state.class() != BlockClass::LightningRodBlock {
        return;
    }
    let powered = state.with(POWERED, true);
    world.set_block_and_update(pos, powered);
    let back = state.get(FACING).opposite();
    world.update_neighbors_at(pos.relative(back), state.block());
    world.schedule_tick(pos, state.block(), ROD_ACTIVATION_TICKS);
    world.level_event(COPPER_CLEAN_EVENT, pos, axis_ordinal(state.get(FACING)));
}

fn axis_ordinal(facing: mc_bamboo_core::Direction) -> i32 {
    use mc_bamboo_core::Axis;
    match facing.axis() {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// `LightningBolt.clearCopperOnLightningStrike` 和随机游走。
fn clear_copper(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::WeatheringCopper) && !is_waxed(state) {
        return;
    }
    if state.is_a(BlockAncestor::WeatheringCopper) {
        // `WeatheringCopper.getFirst`：这一串氧化链的第一块（未氧化）。
        if let Some(first) = weathering_first(state.block_id() as i32) {
            world.set_block_and_update(pos, state.copied_onto(block_of_id(first)));
        }
    }
    let walks = world.random().next_int(COPPER_WALK_COUNT) + COPPER_WALK_COUNT;
    for _ in 0..walks {
        let steps = world.random().next_int(COPPER_STEP_BOUND) + 1;
        let mut cursor = pos;
        let mut moved = false;
        for _ in 0..steps {
            let Some(next) = random_copper_step(world, cursor) else {
                break;
            };
            cursor = next;
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// `HoneycombItem.WAX_OFF_BY_BLOCK` 的键：涂蜡方块都在 `WAXABLES` 的值一侧。
fn is_waxed(state: BlockState) -> bool {
    mc_bamboo_data::block_maps::WAXABLES
        .iter()
        .any(|&(_, to)| to == state.block_id() as i32)
}

/// `WeatheringCopper.getFirst`：沿 `WEATHERING` 的反向链走到没有前驱的那一块。
fn weathering_first(id: i32) -> Option<i32> {
    let mut first = id;
    loop {
        match WEATHERING.iter().find(|&&(_, to)| to == first) {
            Some(&(from, _)) => first = from,
            None => return Some(first),
        }
    }
}

/// `WeatheringCopper.getPrevious`：氧化链里前一个阶段，未氧化时是 `None`。
fn weathering_previous(id: i32) -> Option<i32> {
    WEATHERING
        .iter()
        .find(|&&(_, to)| to == id)
        .map(|&(from, _)| from)
}

/// `BlockPos.randomInCube(level.random, 10, cursor, 1)` 的逐个采样。
/// 每次抽 3 个 `nextInt(3)`，最多 10 次；找到铜就当场降级并返回这一格。
fn random_copper_step(world: &mut World, cursor: BlockPos) -> Option<BlockPos> {
    for _ in 0..COPPER_CUBE_LIMIT {
        let span = 2 * COPPER_CUBE_REACH + 1;
        let x = cursor.x - COPPER_CUBE_REACH + world.random().next_int(span);
        let y = cursor.y - COPPER_CUBE_REACH + world.random().next_int(span);
        let z = cursor.z - COPPER_CUBE_REACH + world.random().next_int(span);
        let pos = BlockPos::new(x, y, z);
        let state = world.block_state(pos);
        if !state.is_a(BlockAncestor::WeatheringCopper) {
            continue;
        }
        if let Some(previous) = weathering_previous(state.block_id() as i32) {
            world.set_block_and_update(pos, state.copied_onto(block_of_id(previous)));
        }
        world.level_event(COPPER_CLEAN_EVENT, pos, 0);
        return Some(pos);
    }
    None
}

fn block_of_id(id: i32) -> &'static mc_bamboo_data::Block {
    &blocks::BLOCKS[id as usize]
}
