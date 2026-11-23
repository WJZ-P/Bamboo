//! `BeehiveBlockEntity`：巢里存着的蜜蜂、花的位置，以及逐刻的进出巢。
//!
//! 存档形状照官方的 `Occupant.LIST_CODEC`（`entity_data` / `ticks_in_hive` /
//! `min_ticks_in_hive`）和 `flower_pos`。放出蜜蜂、加蜜、营火安抚的部分也在这里，
//! 方块本身只负责把剪刀 / 玻璃瓶 / 挖掘的时机交给它。
//!
//! `BeehiveBlockEntity`: stored bees, the saved flower position and the per-tick
//! hive logic. The block routes shears, bottles and mining to it.

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{BlockAncestor, entity_types, sounds, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    BlockEntity, BlockState, World,
    archive::tag::{self, Result},
    entity::{
        Entity, EntityRow,
        mob::{MobType, nbt as mob_nbt},
    },
    shape::{self, CollisionContext},
    state::{FACING, LEVEL_HONEY, LIT},
};

/// `BeehiveBlockEntity.MAX_OCCUPANTS`。
const MAX_OCCUPANTS: usize = entity_types::BEEHIVE_MAX_OCCUPANTS as usize;
/// 没有花蜜的蜂在巢里待多久。
const MIN_TICKS_NECTARLESS: i32 = entity_types::BEEHIVE_MIN_TICKS_NECTARLESS;
/// 带着花蜜回巢的蜂在巢里待多久。
const MIN_TICKS_NECTAR: i32 = entity_types::BEEHIVE_MIN_TICKS_NECTAR;
/// `BeehiveBlock.MAX_HONEY_LEVELS`。
const MAX_HONEY: i32 = 5;

/// `IGNORED_BEE_TAGS`：官方存蜂和放蜂时都删掉的 25 个键。
const IGNORED_BEE_TAGS: &[&str] = &[
    "Air",
    "drop_chances",
    "equipment",
    "Brain",
    "CanPickUpLoot",
    "DeathTime",
    "fall_distance",
    "FallFlying",
    "Fire",
    "HurtTime",
    "LeftHanded",
    "Motion",
    "NoGravity",
    "OnGround",
    "PortalCooldown",
    "Pos",
    "Rotation",
    "sleeping_pos",
    "CannotEnterHiveTicks",
    "TicksSincePollination",
    "CropsGrownSincePollination",
    "hive_pos",
    "Passengers",
    "leash",
    "UUID",
];

/// 官方 `Occupant`：`entity_data` 是抹掉了 `id` 和 25 个无关键的实体 NBT。
#[derive(Debug, Clone, PartialEq)]
pub struct Occupant {
    pub entity_data: Tag,
    pub ticks_in_hive: i32,
    pub min_ticks_in_hive: i32,
}

impl Occupant {
    /// `Occupant.create(ticks)`：生成期的蜂巢每只蜂一条空实体数据。
    pub fn generated(ticks_in_hive: i32) -> Self {
        Self {
            entity_data: Tag::Compound(Vec::new()),
            ticks_in_hive,
            min_ticks_in_hive: MIN_TICKS_NECTARLESS,
        }
    }

    /// `Occupant.of(Entity)`：实体存档去掉不该带的键，按 `HasNectar` 选最短停留时间。
    pub fn of(row: &EntityRow) -> Option<Self> {
        let mut data = mob_nbt::write_mob(row).ok()?;
        strip_ignored(&mut data);
        let nectar = tag::bool_or(&data, "HasNectar", false);
        Some(Self {
            entity_data: data,
            ticks_in_hive: 0,
            min_ticks_in_hive: if nectar {
                MIN_TICKS_NECTAR
            } else {
                MIN_TICKS_NECTARLESS
            },
        })
    }

    /// `BeeData.hasNectar()`。
    pub fn has_nectar(&self) -> bool {
        tag::bool_or(&self.entity_data, "HasNectar", false)
    }

    /// `Occupant.createEntity`：把存着的生物读回来。坐标由调用方按巢的朝向覆盖。
    fn into_entity(self, hive: BlockPos) -> Option<Entity> {
        let mut entity = Entity::by_mob_type(MobType::Bee, Vec3::default());
        let mut data = self.entity_data;
        strip_ignored(&mut data);
        // 读盘的实体不重取构造随机数；NBT 里没写坐标 / 朝向，官方按放出位置 snapTo。
        mob_nbt::load_into(&mut entity, &data).ok()?;
        entity.with_mob(|_, mob| {
            crate::entity::mob::bee::set_release_data(mob, hive, self.ticks_in_hive);
        });
        entity.with_mob(|_, mob| {
            mob.no_gravity = true;
        });
        Some(entity)
    }

    fn to_tag(&self) -> Tag {
        Tag::Compound(vec![
            ("entity_data".to_owned(), self.entity_data.clone()),
            ("ticks_in_hive".to_owned(), Tag::Int(self.ticks_in_hive)),
            (
                "min_ticks_in_hive".to_owned(),
                Tag::Int(self.min_ticks_in_hive),
            ),
        ])
    }

    /// `Occupant.CODEC`：三个必填字段，坏条目让整个列表解码失败回空。
    fn from_tag(value: &Tag) -> Option<Self> {
        let entity_data = tag::field(value, "entity_data")?;
        if !matches!(entity_data, Tag::Compound(_)) {
            return None;
        }
        Some(Self {
            entity_data: entity_data.clone(),
            ticks_in_hive: tag::java_i32(tag::field(value, "ticks_in_hive")?).ok()?,
            min_ticks_in_hive: tag::java_i32(tag::field(value, "min_ticks_in_hive")?).ok()?,
        })
    }
}

fn strip_ignored(data: &mut Tag) {
    if let Tag::Compound(entries) = data {
        entries.retain(|(key, _)| {
            !matches!(key.as_str(), "id") && !IGNORED_BEE_TAGS.contains(&key.as_str())
        });
    }
}

/// 放出蜜蜂的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStatus {
    /// 带着花蜜回巢：卸蜜并可能增加 `honey_level`。
    HoneyDelivered,
    /// 时间到了，正常出巢。
    BeeReleased,
    /// 取蜜 / 无精准采集挖掉：无视天气和出口阻挡。
    Emergency,
}

/// 蜂巢方块实体。
#[derive(Debug)]
pub struct BeehiveEntity {
    pos: BlockPos,
    bees: Vec<Occupant>,
    flower_pos: Option<BlockPos>,
}

impl BeehiveEntity {
    /// `BeehiveBlockEntity(pos, state)`。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            bees: Vec::new(),
            flower_pos: None,
        }
    }

    /// 按官方 NBT 解码。坏条目整条列表回空，花位置形状不对就丢掉，读盘不限只数。
    pub fn read(pos: BlockPos, tag: &Tag) -> Self {
        let bees = match tag::field(tag, "bees") {
            Some(Tag::List(values)) => values
                .iter()
                .map(Occupant::from_tag)
                .collect::<Option<Vec<_>>>()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let raw = tag::int_array(tag, "flower_pos");
        let flower_pos = (raw.len() == 3).then(|| BlockPos::new(raw[0], raw[1], raw[2]));
        // 官方 `loadAdditional` 对每一条调 `storeBee`，那只是 `List.add`，读盘不按
        // 三只截断；三只的上限只在 `addOccupant`（蜂主动进巢）时检查。
        // Vanilla `loadAdditional` calls `storeBee` for every entry, which is a plain
        // `List.add`; the three-bee cap only applies in `addOccupant`.
        Self {
            pos,
            bees,
            flower_pos,
        }
    }

    /// `storeBee`：最多三只。
    pub fn store(&mut self, occupant: Occupant) -> bool {
        if self.is_full() {
            return false;
        }
        self.bees.push(occupant);
        true
    }

    /// `addOccupant(Bee)`：把一只真的蜜蜂收进巢，返回成功后调用方可以丢弃它。
    pub fn add_bee(&mut self, row: &EntityRow, keep_flower: bool) -> bool {
        let Some(occupant) = Occupant::of(row) else {
            return false;
        };
        let flower = tag::int_array(&occupant.entity_data, "flower_pos");
        if flower.len() == 3 && (self.flower_pos.is_none() || keep_flower) {
            self.flower_pos = Some(BlockPos::new(flower[0], flower[1], flower[2]));
        }
        self.store(occupant)
    }

    pub fn is_empty(&self) -> bool {
        self.bees.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.bees.len() >= MAX_OCCUPANTS
    }

    pub fn occupant_count(&self) -> usize {
        self.bees.len()
    }

    pub fn saved_flower_pos(&self) -> Option<BlockPos> {
        self.flower_pos
    }

    pub fn set_saved_flower_pos(&mut self, pos: Option<BlockPos>) {
        self.flower_pos = pos;
    }

    /// `BeehiveBlockEntity.serverTick`：每只蜂加一刻，到点的按花蜜与否放出。
    pub fn server_tick(&mut self, world: &mut World) {
        let state = world.block_state(self.pos);
        let mut i = 0;
        while i < self.bees.len() {
            // `BeeData.tick()` 是 `ticksInHive++ > minTicksInHive`：字段先加一，
            // 但比较用的是加一之前的旧值，所以蜂比 `ticks > min` 晚一刻出巢。
            let was_due = self.bees[i].ticks_in_hive > self.bees[i].min_ticks_in_hive;
            self.bees[i].ticks_in_hive = self.bees[i].ticks_in_hive.saturating_add(1);
            if !was_due {
                i += 1;
                continue;
            }
            let status = if self.bees[i].has_nectar() {
                ReleaseStatus::HoneyDelivered
            } else {
                ReleaseStatus::BeeReleased
            };
            // `tickOccupants` 只在 `releaseOccupant` 返回真时移除；被天气 / 出口挡住时
            // 蜂留在巢里，下一刻再试，而且不重复取花位置 / 蜜等级 / 生成实体的随机数。
            let occupant = self.bees[i].clone();
            if self.release(world, state, occupant, status, None).is_some() {
                self.bees.remove(i);
            } else {
                i += 1;
            }
        }
        if !self.bees.is_empty() && world.random().next_double() < 0.005 {
            world.play_sound(
                Vec3::new(
                    f64::from(self.pos.x) + 0.5,
                    f64::from(self.pos.y),
                    f64::from(self.pos.z) + 0.5,
                ),
                sounds::sound_event("minecraft:block.beehive.work"),
                sounds::BLOCKS,
                1.0,
                1.0,
                false,
            );
        }
    }

    /// 官方 `BeehiveBlockEntity.emptyAllLivingFromHive(player, state, status)`。
    ///
    /// `player` 是这次取蜜 / 挖巢的玩家本身，不在附近玩家里挑、不取随机数；
    /// 为 `None` 时整段（除放出外）跳过。距离按**玩家到每只放出的蜂**算，
    /// 只有 4 格内的蜂才会被设目标或设 400 刻冷却。
    pub fn release_all(
        &mut self,
        world: &mut World,
        state: BlockState,
        status: ReleaseStatus,
        player: Option<crate::entity::ctx::PlayerView>,
    ) {
        let bees = std::mem::take(&mut self.bees);
        let sedated = is_sedated(world, self.pos);
        let mut spawn = Vec::new();
        for occupant in bees {
            if let Some(id) = self.release(world, state, occupant, status, None) {
                spawn.push(id);
            }
        }
        let Some(player) = player else {
            return;
        };
        for id in spawn {
            world.with_entity_mut(id, &mut |entity| {
                let close = player.position.distance_to_sqr(entity.position) <= 16.0;
                entity.with_mob(|_, mob| {
                    if close {
                        if sedated {
                            crate::entity::mob::bee::set_stay_out_of_hive(
                                mob,
                                entity_types::BEEHIVE_REENTER_TICKS,
                            );
                        } else {
                            mob.target = Some(player.uid);
                        }
                    }
                });
            });
        }
    }

    /// `releaseOccupant`：按官方顺序取随机数、写蜜等级、生成实体并放到朝向的那一面。
    /// 返回新实体的 ID（放出成功时）；调用方用它决定要不要发怒。
    fn release(
        &mut self,
        world: &mut World,
        state: BlockState,
        occupant: Occupant,
        status: ReleaseStatus,
        player_pos: Option<Vec3>,
    ) -> Option<i32> {
        if world.bees_stay_in_hive(self.pos) && status != ReleaseStatus::Emergency {
            return None;
        }
        let facing = state.try_get(FACING).unwrap_or(Direction::North);
        let exit = self.pos.relative(facing);
        let blocked = !world
            .block_state(exit)
            .collision_boxes(world, exit, CollisionContext::Empty)
            .is_empty();
        if blocked && status != ReleaseStatus::Emergency {
            return None;
        }
        let mut entity = occupant.into_entity(self.pos)?;
        // `releaseOccupant`：巢里存了花、蜂自己没花时，`level.random.nextFloat() < 0.9`
        // 就用巢里的花（注释里是 10% 概率，字节码是 `< 0.9`，照字节码）。
        if let Some(flower) = self.flower_pos
            && !entity
                .with_mob(|_, mob| crate::entity::mob::bee::has_saved_flower(mob))
                .unwrap_or(false)
            && world.random().next_float() < 0.9
        {
            entity.with_mob(|_, mob| crate::entity::mob::bee::set_saved_flower(mob, Some(flower)));
        }
        if status == ReleaseStatus::HoneyDelivered {
            entity.with_mob(|_, mob| crate::entity::mob::bee::drop_off_nectar(mob));
            if state.is_tagged(block_tags::BEEHIVES) && state.is_a(BlockAncestor::BeehiveBlock) {
                let level = state.get(LEVEL_HONEY);
                if level < MAX_HONEY {
                    let mut extra = if world.random().next_int(100) == 0 {
                        2
                    } else {
                        1
                    };
                    if level + extra > MAX_HONEY {
                        extra -= 1;
                    }
                    let _ = world
                        .set_block_and_update(self.pos, state.with(LEVEL_HONEY, level + extra));
                }
            }
        }
        let (width, height) = entity.dimensions;
        let offset = if blocked {
            0.0
        } else {
            0.55 + f64::from(width) / 2.0
        };
        let (dx, _, dz) = facing.normal();
        entity.position = Vec3::new(
            f64::from(self.pos.x) + 0.5 + offset * f64::from(dx),
            f64::from(self.pos.y) + 0.5 - f64::from(height) / 2.0,
            f64::from(self.pos.z) + 0.5 + offset * f64::from(dz),
        );
        world.play_block_sound(
            self.pos,
            sounds::sound_event("minecraft:block.beehive.exit"),
            1.0,
            1.0,
        );
        let id = world.spawn_entity(entity);
        let _ = player_pos;
        Some(id)
    }

    /// `saveAdditional`：两个键无条件写，花位置只在有值时写。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = vec![(
            "bees".to_owned(),
            Tag::List(self.bees.iter().map(Occupant::to_tag).collect()),
        )];
        if let Some(flower) = self.flower_pos {
            out.push((
                "flower_pos".to_owned(),
                Tag::IntArray(vec![flower.x, flower.y, flower.z]),
            ));
        }
        out
    }
}

impl BlockEntity for BeehiveEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>> {
        Ok(self.additional())
    }
}

/// 官方 `BeehiveBlock.angerNearbyBees`：蜂巢 AABB 外扩 (8, 6, 8) 里的蜂和玩家。
///
/// 蜂列表、玩家列表都按实体分区顺序（这里按 uid 升序）取。玩家列表为空时
/// 不取随机数；否则**每只无目标的蜂**都取一次世界随机源 `nextInt(玩家数)`，
/// 哪怕只有一个玩家、哪怕这只蜂不在 4 格内。和 `emptyAllLivingFromHive`
/// 是两段独立的逻辑，调用方按官方条件分别调用。
pub fn anger_nearby_bees(world: &mut World, hive: BlockPos) {
    let area = crate::shape::Aabb::new(
        f64::from(hive.x) - 8.0,
        f64::from(hive.y) - 6.0,
        f64::from(hive.z) - 8.0,
        f64::from(hive.x) + 9.0,
        f64::from(hive.y) + 7.0,
        f64::from(hive.z) + 9.0,
    );
    let mut bees = Vec::new();
    world.level().for_each_entity_in(area, &mut |public| {
        if matches!(&public.kind, crate::entity::KindPublic::Mob(mob) if mob.mob_type == MobType::Bee)
        {
            bees.push(public.uid);
        }
    });
    if bees.is_empty() {
        return;
    }
    let mut players = Vec::new();
    world.level().for_each_player(&mut |player| {
        if area.contains(player.position.x, player.position.y, player.position.z) {
            players.push((player.uid, player.position));
        }
    });
    if players.is_empty() {
        return;
    }
    players.sort_by_key(|(uid, _)| *uid);
    for uid in bees {
        // 先看目标；有目标的蜂不取随机数。
        let has_target = world
            .level()
            .entity_public(uid)
            .and_then(|public| match public.kind {
                crate::entity::KindPublic::Mob(mob) => Some(mob.has_target),
                _ => None,
            })
            .unwrap_or(true);
        if has_target {
            continue;
        }
        let pick = world.random().next_int(players.len() as i32) as usize;
        let target = players.get(pick).map(|(uid, _)| *uid);
        if let Some(target) = target {
            world.with_entity_mut(uid, &mut |entity| {
                entity.with_mob(|_, mob| mob.target = Some(target));
            });
        }
    }
}

/// `CampfireBlock.isSmokeyPos`：下面 1..5 格有点着的营火（或隔着虚拟柱子）时为真。
pub fn is_sedated(world: &World, pos: BlockPos) -> bool {
    for distance in 1..=5 {
        let below = pos.relative_by(Direction::Down, distance);
        let state = world.block_state(below);
        if lit_campfire(state) {
            return true;
        }
        // `SHAPE_VIRTUAL_POST` 是 4/16 宽的柱子：绝大多数带碰撞形状的方块都与它相交，
        // 空气和没有碰撞形状的方块不相交。
        let post = state.collision_boxes(world, below, CollisionContext::Empty);
        let intersects = !post.is_empty()
            && shape::join_is_not_empty(
                &post,
                &[mc_bamboo_core::Aabb::new(
                    6.0 / 16.0,
                    0.0,
                    6.0 / 16.0,
                    10.0 / 16.0,
                    1.0,
                    10.0 / 16.0,
                )],
                shape::BooleanOp::And,
            );
        if intersects {
            // 官方在虚拟柱子与这一格相交时立刻返回「下面一格是否点着的营火」，
            // 不再往下找，所以隔着一层石板的营火不算冒烟。
            return lit_campfire(world.block_state(below.below()));
        }
    }
    false
}

/// `CampfireBlock.isLitCampfire`。
fn lit_campfire(state: BlockState) -> bool {
    state.is_tagged(block_tags::CAMPFIRES) && state.try_get(LIT).unwrap_or(false)
}

/// `BeehiveBlock.newBlockEntity`：放置时给方块挂一个空蜂巢。
pub fn create_block_entity(pos: BlockPos) -> Box<dyn BlockEntity> {
    Box::new(BeehiveEntity::new(pos))
}
