//! `SpawnerBlockEntity` / `BaseSpawner`：活的刷怪笼。
//!
//! 存档形状照 `BaseSpawner.load` / `save`（`Delay` / 六个计时字段、storeNullable 的
//! `SpawnData`、`SpawnPotentials` 加权表），和 `archive::resave` 的纯存档往返共用同一份
//! `SpawnData` 编码，保证读盘条目逐字节往返。
//!
//! `serverTick` 照官方字节码：`isNearPlayer`（`RequiredPlayerRange`，旁观者不算）、
//! `Delay` 倒数、`SpawnCount` 次取点、`noCollision`、`SpawnPlacements` / `CustomSpawnRules`
//! 的光照检查、`MaxNearbyEntities` 的范围计数、只有 `id` 时跑 `finalizeSpawn`、
//! `spawnAnim`、2004 号世界事件。随机数的来源和抽取顺序与官方一致（都走 `level.random`）。
//!
//! `SpawnerBlockEntity` / `BaseSpawner`: the live mob spawner. The save shape follows
//! `BaseSpawner.load` / `save` and shares its `SpawnData` codec with `archive::resave`,
//! so a disk entry round-trips byte for byte. `serverTick` follows the official bytecode.

use std::any::Any;

use mc_bamboo_core::{BlockPos, LightLayer, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::cursor::parse_identifier;

use crate::{
    BlockEntity, BlockState, World,
    archive::tag::{self, Result},
    entity::{
        Entity, KindPublic,
        ctx::BlockView,
        mob::{MobEvent, MobType, natural, nbt as mob_nbt, spawn},
    },
    random::RandomSource,
    shape::Aabb,
};

/// `SpawnData`：`entity` 是必填的 `CompoundTag`（构造时把 `entity.id` 按 `Identifier.CODEC`
/// 规范化），`custom_spawn_rules` / `equipment` 按官方 codec 的宽松规则原样带回。
///
/// `SpawnData`: `entity` is required and its `id` is canonicalized like the official
/// constructor; the two optional fields are carried through verbatim.
#[derive(Clone)]
struct SpawnData {
    entity: Tag,
    custom_spawn_rules: Option<Tag>,
    equipment: Option<Tag>,
}

impl SpawnData {
    /// `new SpawnData()`：空的实体标签，两个可省字段都没有。
    ///
    /// `new SpawnData()`: an empty entity tag and no optional fields.
    fn empty() -> Self {
        Self {
            entity: Tag::Compound(Vec::new()),
            custom_spawn_rules: None,
            equipment: None,
        }
    }

    /// `SpawnData.CODEC` 的宽松解码：`entity` 缺省为空复合标签，`entity.id` 规范化
    /// （解不开就移除），两个可省字段存在时原样带回。
    ///
    /// A lenient decode of `SpawnData.CODEC`: `entity` defaults to empty, `entity.id`
    /// is canonicalized (removed when invalid) and the optional fields are carried.
    fn read(tag: &Tag) -> Self {
        let mut entity = match tag::field(tag, "entity") {
            Some(Tag::Compound(values)) => values.clone(),
            _ => Vec::new(),
        };
        if let Some(Tag::String(id)) = tag::field(tag, "entity").and_then(|e| tag::field(e, "id")) {
            // `SpawnData` 的构造器用 `Identifier.CODEC` 把 `entity.id` 规范化；
            // 解不开时移除 `id`（`CompoundTag.remove`）。
            // The constructor canonicalizes `entity.id`, removing it when invalid.
            entity.retain(|(key, _)| key != "id");
            if let Ok(name) = parse_identifier(id) {
                entity.push(("id".to_owned(), Tag::String(name)));
            }
        }
        Self {
            entity: Tag::Compound(entity),
            custom_spawn_rules: tag::field(tag, "custom_spawn_rules")
                .map(normalize_custom_spawn_rules),
            equipment: tag::field(tag, "equipment").cloned(),
        }
    }

    /// `SpawnData.CODEC` 的重新编码：`entity` 必写，两个可省字段按读进来的形状原样带回。
    ///
    /// Re-encode `SpawnData.CODEC`: `entity` always, the optional fields verbatim.
    fn to_tag(&self) -> Tag {
        let mut out = vec![("entity".to_owned(), self.entity.clone())];
        if let Some(rules) = &self.custom_spawn_rules {
            out.push(("custom_spawn_rules".to_owned(), rules.clone()));
        }
        if let Some(equipment) = &self.equipment {
            out.push(("equipment".to_owned(), equipment.clone()));
        }
        Tag::Compound(out)
    }

    /// `entity.id` 的实体类型；缺了或不是注册过的建模实体时是 `None`。
    ///
    /// The entity type of `entity.id`; `None` when missing or not a modelled mob.
    fn kind(&self) -> Option<MobType> {
        match tag::field(&self.entity, "id") {
            Some(Tag::String(id)) => MobType::by_name(id),
            _ => None,
        }
    }

    /// 实体标签是否只有一个 `id` 键（`getEntityToSpawn().size() == 1 && id` 存在）。
    ///
    /// Whether the entity tag holds exactly one `id` key.
    fn is_id_only(&self) -> bool {
        match &self.entity {
            Tag::Compound(entries) if entries.len() == 1 => {
                matches!(entries.first(), Some((key, Tag::String(_))) if key == "id")
            }
            _ => false,
        }
    }

    /// `CustomSpawnRules.isValidPosition`：方块光和有效天空光都落在两个闭区间里。
    ///
    /// `CustomSpawnRules.isValidPosition`: the block light and the effective sky
    /// light both fall inside the two inclusive ranges.
    fn custom_rules_ok(&self, world: &World, pos: BlockPos) -> bool {
        let Some(Tag::Compound(entries)) = &self.custom_spawn_rules else {
            return true;
        };
        let raw = Tag::Compound(entries.clone());
        let (block_min, block_max) = light_limit(&raw, "block_light_limit");
        let (sky_min, sky_max) = light_limit(&raw, "sky_light_limit");
        (block_min..=block_max).contains(&world.brightness(LightLayer::Block, pos))
            && (sky_min..=sky_max).contains(&world.effective_sky_brightness(pos))
    }
}

/// `CustomSpawnRules.lightLimit` 的宽松读取：`InclusiveRange.INT` 接受单个 int、`[min, max]`
/// 列表或 `{min_inclusive, max_inclusive}` 复合标签，缺省 `LIGHT_RANGE = [0, 15]`。
///
/// A lenient read of `lightLimit`: `InclusiveRange.INT` accepts a single int, a
/// `[min, max]` list or a `{min_inclusive, max_inclusive}` compound; the default
/// `LIGHT_RANGE` is `[0, 15]`.
fn light_limit(raw: &Tag, key: &str) -> (i32, i32) {
    let Some(value) = tag::field(raw, key) else {
        return (0, 15);
    };
    match value {
        Tag::Compound(_) => (
            tag::int_or(value, "min_inclusive", 0),
            tag::int_or(value, "max_inclusive", 15),
        ),
        Tag::List(values) if values.len() == 2 => (
            value_at(&values[0]).unwrap_or(0),
            value_at(&values[1]).unwrap_or(15),
        ),
        _ => {
            let single = value_at(value).unwrap_or(0);
            (single, single)
        }
    }
}

/// 读一个数值标签成 int；形状不对时是 `None`。
fn value_at(tag: &Tag) -> Option<i32> {
    tag::java_i32(tag).ok()
}

/// 把 `custom_spawn_rules` 的 `block_light_limit` / `sky_light_limit` 按官方
/// `InclusiveRange.INT` 的编码形状规范化：先用 [`light_limit`] 宽松读成
/// `(min, max)`，等于缺省 `LIGHT_RANGE = [0, 15]` 的字段按
/// `lenientOptionalFieldOf` 的语义省略，其余 `min == max` 写单 int、
/// `min != max` 写 `[min, max]` 列表。官方 `load -> save` 也会把
/// `{min_inclusive, max_inclusive}` 复合标签重写成这两种形状，所以这里不能
/// 原样带回输入。
///
/// Normalize the two light limits of `custom_spawn_rules` to the shape the
/// official `InclusiveRange.INT` codec writes: a single int when `min == max`,
/// a `[min, max]` list otherwise, and omitted when equal to the default
/// `[0, 15]` (`lenientOptionalFieldOf`). A `{min_inclusive, max_inclusive}`
/// compound is rewritten by vanilla's `load -> save` too, so it cannot be
/// carried through verbatim.
fn normalize_custom_spawn_rules(tag: &Tag) -> Tag {
    let Tag::Compound(entries) = tag else {
        return tag.clone();
    };
    let raw = Tag::Compound(entries.clone());
    let mut out = Vec::new();
    for (key, value) in entries {
        if (key == "block_light_limit" || key == "sky_light_limit")
            && let (min, max) = light_limit(&raw, key)
        {
            if min == 0 && max == 15 {
                // `lenientOptionalFieldOf` 的缺省值，编码时省略。
                continue;
            }
            let encoded = if min == max {
                Tag::Int(min)
            } else {
                Tag::List(vec![Tag::Int(min), Tag::Int(max)])
            };
            out.push((key.clone(), encoded));
        } else {
            out.push((key.clone(), value.clone()));
        }
    }
    Tag::Compound(out)
}

/// `SpawnerBlockEntity`：挂在 `minecraft:spawner` 上的活方块实体。
///
/// The block entity of `minecraft:spawner`.
pub struct SpawnerBlockEntity {
    pos: BlockPos,
    spawn_delay: i32,
    /// 加权表里的每一项：(数据, 权重)。空表时 `WeightedList.of()` 的 `selector` 为空。
    ///
    /// The weighted list entries: (data, weight). An empty list has no selector.
    spawn_potentials: Vec<(SpawnData, i32)>,
    next_spawn_data: Option<SpawnData>,
    min_spawn_delay: i32,
    max_spawn_delay: i32,
    spawn_count: i32,
    max_nearby_entities: i32,
    required_player_range: i32,
    spawn_range: i32,
}

impl SpawnerBlockEntity {
    /// `new SpawnerBlockEntity(pos, state)`：`BaseSpawner` 的构造器默认值。
    ///
    /// `new SpawnerBlockEntity(pos, state)`: the `BaseSpawner` constructor defaults.
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            spawn_delay: 20,
            spawn_potentials: Vec::new(),
            next_spawn_data: None,
            min_spawn_delay: 200,
            max_spawn_delay: 800,
            spawn_count: 4,
            max_nearby_entities: 6,
            required_player_range: 16,
            spawn_range: 4,
        }
    }

    /// `BaseSpawner.load` 的宽松解码：坏数据一律回官方缺省值，不 panic。
    ///
    /// A lenient decode of `BaseSpawner.load`; bad data falls back to the defaults.
    pub fn read(pos: BlockPos, raw: &Tag) -> Self {
        let next_spawn_data = tag::field(raw, "SpawnData")
            .filter(|tag| matches!(tag, Tag::Compound(_)))
            .map(SpawnData::read);
        let mut this = Self {
            pos,
            spawn_delay: i32::from(tag::short_or(raw, "Delay", 20)),
            spawn_potentials: Vec::new(),
            next_spawn_data,
            min_spawn_delay: tag::int_or(raw, "MinSpawnDelay", 200),
            max_spawn_delay: tag::int_or(raw, "MaxSpawnDelay", 800),
            spawn_count: tag::int_or(raw, "SpawnCount", 4),
            max_nearby_entities: tag::int_or(raw, "MaxNearbyEntities", 6),
            required_player_range: tag::int_or(raw, "RequiredPlayerRange", 16),
            spawn_range: tag::int_or(raw, "SpawnRange", 4),
        };
        this.spawn_potentials = Self::read_potentials(raw);
        this
    }

    /// `BaseSpawner.load` 里 `SpawnPotentials` 缺失时按 `SpawnData` 造一份带一项的
    /// 列表：有 `SpawnData` 就它自己，否则一份空的 `SpawnData`（权重 1）。
    ///
    /// A missing `SpawnPotentials` list becomes one `SpawnData` item with weight 1.
    fn read_potentials(raw: &Tag) -> Vec<(SpawnData, i32)> {
        let fallback = tag::field(raw, "SpawnData")
            .filter(|tag| matches!(tag, Tag::Compound(_)))
            .map(|data| vec![(SpawnData::read(data), 1)])
            .unwrap_or_else(|| vec![(SpawnData::empty(), 1)]);
        let Some(Tag::List(values)) = tag::field(raw, "SpawnPotentials") else {
            return fallback;
        };
        let mut out = Vec::with_capacity(values.len());
        for value in values {
            let Tag::Compound(_) = value else {
                return fallback;
            };
            let Some(data) =
                tag::field(value, "data").filter(|tag| matches!(tag, Tag::Compound(_)))
            else {
                return fallback;
            };
            out.push((SpawnData::read(data), tag::int_or(value, "weight", 1)));
        }
        out
    }

    /// `BaseSpawner.save` 写出的键：7 个 short、storeNullable 的 `SpawnData`、`SpawnPotentials`。
    ///
    /// The keys of `BaseSpawner.save`: seven shorts, the nullable `SpawnData` and the
    /// `SpawnPotentials` list.
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        for (key, value) in [
            ("Delay", self.spawn_delay),
            ("MinSpawnDelay", self.min_spawn_delay),
            ("MaxSpawnDelay", self.max_spawn_delay),
            ("SpawnCount", self.spawn_count),
            ("MaxNearbyEntities", self.max_nearby_entities),
            ("RequiredPlayerRange", self.required_player_range),
            ("SpawnRange", self.spawn_range),
        ] {
            tag::put(&mut out, key, Tag::Short(value as i16));
        }
        // `SpawnData` 是 storeNullable：只有读盘条目里解得出时才写。
        // `SpawnData` is storeNullable: written only when it decoded from disk.
        if let Some(data) = &self.next_spawn_data {
            tag::put(&mut out, "SpawnData", data.to_tag());
        }
        tag::put(
            &mut out,
            "SpawnPotentials",
            Tag::List(
                self.spawn_potentials
                    .iter()
                    .map(|(data, weight)| {
                        Tag::Compound(vec![
                            ("data".to_owned(), data.to_tag()),
                            ("weight".to_owned(), Tag::Int(*weight)),
                        ])
                    })
                    .collect(),
            ),
        );
        out
    }

    /// `Spawner.setEntityId`（`SpawnEggItem.useOn` 对刷怪笼用）：改当前 `SpawnData` 的
    /// 实体类型。`getOrCreateNextSpawnData` 在缺省数据时会从加权表抽一次。
    ///
    /// `Spawner.setEntityId`: rewrite the current `SpawnData`'s entity type. A
    /// missing `nextSpawnData` is picked from the weighted list first.
    pub fn set_entity_id(&mut self, kind: MobType, world: &World) {
        self.ensure_next_spawn_data(world);
        let data = self.next_spawn_data.as_mut().expect("刚填过");
        if let Tag::Compound(entries) = &mut data.entity {
            entries.retain(|(key, _)| key != "id");
            entries.push(("id".to_owned(), Tag::String(kind.name().to_owned())));
        }
    }

    /// 对拍/测试用：直接把倒计时拨到 `delay`（跳过构造器的 20）。
    ///
    /// Test hook: set the countdown to `delay`, skipping the constructor's 20.
    #[doc(hidden)]
    pub fn set_delay_for_test(&mut self, delay: i32) {
        self.spawn_delay = delay;
    }

    /// 对拍/测试用：读出当前倒计时。
    ///
    /// Test hook: the current countdown.
    #[doc(hidden)]
    pub fn delay_for_test(&self) -> i32 {
        self.spawn_delay
    }

    /// 对拍/测试用：当前 `SpawnData` 的实体类型名（没有时是空串）。
    ///
    /// Test hook: the current `SpawnData` entity type name, or empty.
    #[doc(hidden)]
    pub fn entity_for_test(&self) -> String {
        self.next_spawn_data
            .as_ref()
            .and_then(|data| match tag::field(&data.entity, "id") {
                Some(Tag::String(id)) => Some(id.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// `getOrCreateNextSpawnData` 的「缺省才抽」那半：从加权表抽一份，表空时用空数据。
    ///
    /// The pick-on-demand half of `getOrCreateNextSpawnData`.
    fn ensure_next_spawn_data(&mut self, world: &World) {
        if self.next_spawn_data.is_none() {
            let picked = weighted(&self.spawn_potentials, world.random())
                .cloned()
                .unwrap_or_else(SpawnData::empty);
            self.next_spawn_data = Some(picked);
        }
    }

    /// `BaseSpawner.isNearPlayer`：`RequiredPlayerRange` 内有没有活着的非旁观者玩家。
    ///
    /// `BaseSpawner.isNearPlayer`: a live non-spectator player within `RequiredPlayerRange`.
    fn is_near_player(&self, world: &World) -> bool {
        let range = f64::from(self.required_player_range);
        let x = f64::from(self.pos.x) + 0.5;
        let y = f64::from(self.pos.y) + 0.5;
        let z = f64::from(self.pos.z) + 0.5;
        let mut found = false;
        BlockView::for_each_player(world, &mut |player| {
            if player.spectator || !player.alive {
                return;
            }
            let dx = player.position.x - x;
            let dy = player.position.y - y;
            let dz = player.position.z - z;
            if dx * dx + dy * dy + dz * dz < range * range {
                found = true;
            }
        });
        found
    }

    /// `BaseSpawner.delay`：按 `MinSpawnDelay` / `MaxSpawnDelay` 重抽倒计时，从
    /// `SpawnPotentials` 加权选下一个 `SpawnData`，再发方块事件 1。
    ///
    /// `BaseSpawner.delay`: re-roll the delay, pick the next `SpawnData` and fire
    /// block event 1.
    fn delay(&mut self, world: &mut World) {
        self.spawn_delay = if self.max_spawn_delay <= self.min_spawn_delay {
            self.min_spawn_delay
        } else {
            self.min_spawn_delay
                + world
                    .random()
                    .next_int(self.max_spawn_delay - self.min_spawn_delay)
        };
        // `spawnPotentials.getRandom(random).ifPresent(setNextSpawnData)`：只在抽得到时换。
        let picked = weighted(&self.spawn_potentials, world.random()).cloned();
        if let Some(data) = picked {
            self.next_spawn_data = Some(data);
        }
        // `SpawnerBlockEntity.broadcastEvent(level, pos, 1)`。
        world.block_event(self.pos, BlockState(blocks::SPAWNER).block(), 1, 0);
    }

    /// `BaseSpawner.serverTick` 的完整逐刻逻辑。
    ///
    /// The full per-tick logic of `BaseSpawner.serverTick`.
    fn server_tick(&mut self, world: &mut World) {
        if !self.is_near_player(world) || !world.block_game_rules().spawner_blocks_work {
            return;
        }
        if self.spawn_delay == -1 {
            self.delay(world);
        }
        if self.spawn_delay > 0 {
            self.spawn_delay -= 1;
            return;
        }
        self.ensure_next_spawn_data(world);
        // 这一轮要刷的数据在整个循环里不变；先按值拷出来，避免借走 `self` 挡住 `world`。
        let data = self.next_spawn_data.clone().expect("上面刚填过");
        let mut spawned = false;
        for _ in 0..self.spawn_count {
            let Some(kind) = data.kind() else {
                // 实体类型解不出来（没建模的实体也算）：`delay` 后放弃这一轮。
                // An unknown or unmodelled entity type: re-roll and give up.
                self.delay(world);
                return;
            };
            // `Pos`（Vec3.CODEC）或随机取点：`pos.x + (d - d) * range + 0.5`、
            // `pos.y + nextInt(3) - 1`、`pos.z + (d - d) * range + 0.5`。
            let at = read_pos(&data.entity)
                .unwrap_or_else(|| random_position(world, self.pos, self.spawn_range));
            if !natural::no_collision(world, natural::spawn_aabb_at(kind, at.x, at.y, at.z)) {
                continue;
            }
            let block_pos = BlockPos::containing(at);
            let custom = data.custom_spawn_rules.is_some();
            if custom {
                // `CustomSpawnRules` 分支：非友好生物在和平难度不刷，再查光照。
                // The custom-rules branch: no hostile mob on peaceful, then light.
                if !kind.mob_category().facts().friendly && world.difficulty_id() == 0 {
                    continue;
                }
                if !data.custom_rules_ok(world, block_pos) {
                    continue;
                }
            } else if !natural::check_spawner_rules(world, kind, block_pos) {
                continue;
            }
            // `EntityType.loadEntityRecursive`：读进 NBT，`snapTo` 到落点。
            let mut entity = world.prepare_entity(Entity::by_mob_type(kind, Vec3::default()));
            if mob_nbt::load_into(&mut entity, &data.entity).is_err() {
                self.delay(world);
                return;
            }
            let y_rot = entity.y_rot;
            let x_rot = entity.kind.mob().map_or(0.0, |mob| mob.x_rot);
            spawn::snap_to(&mut entity, at, y_rot, x_rot);
            // `MaxNearbyEntities`：精确类的实体数（`AABB(pos).inflate(spawnRange)`）。
            if count_nearby(world, self.pos, self.spawn_range, kind) >= self.max_nearby_entities {
                self.delay(world);
                return;
            }
            spawn::snap_to(&mut entity, at, world.random().next_float() * 360.0, 0.0);
            let is_mob = entity.kind.mob().is_some();
            if is_mob {
                if !custom && !natural::pathfinder_spawn_rules(world, &entity, block_pos) {
                    continue;
                }
                if !natural::check_spawn_obstruction(world, &entity, kind) {
                    continue;
                }
            }
            // 只有 `id` 时跑 `finalizeSpawn`；普通刷怪笼不标持久、不套装备表。
            // 与试炼刷怪笼共用 World 层的提交路径（`finalize` / `add` / `finish_jockeys`）。
            // Shared commit path with the trial spawner (`finalize` / `add` / jockeys).
            let (_, uid) = world.commit_spawn_data_entity(
                entity,
                kind,
                data.is_id_only(),
                spawn::SpawnReason::Spawner,
                false,
                None,
            );
            world.level_event(2004, self.pos, 0);
            // `gameEvent(entity, ENTITY_PLACE, BlockPos.containing(落点))`：源头是新刷出的实体，
            // 位置取落点所在格的格心。
            // `gameEvent(entity, ENTITY_PLACE, BlockPos.containing(spawn))`, sourced from the new
            // entity at the centre of the block it landed in.
            world.game_event(
                crate::game_event::GameEvent::EntityPlace,
                crate::world::block_center_public(block_pos),
                crate::game_event::dispatch::EventContext::default(),
            );
            if is_mob {
                world.with_entity_mut(uid, &mut |entity| {
                    entity.with_mob(|_, mob| {
                        mob.events.push(MobEvent::EntityEvent { event: 20 });
                    });
                });
            }
            spawned = true;
        }
        if spawned {
            self.delay(world);
        }
    }
}

impl BlockEntity for SpawnerBlockEntity {
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

/// `WeightedList.getRandom`：`nextInt(总权重)` 之后按累减权重挑一项；表空或总权重为 0
/// 时不取随机数，返回 `None`。
///
/// `WeightedList.getRandom`: draw `nextInt(totalWeight)` then walk the cumulative
/// weights; an empty table or a zero total returns `None` without a draw.
fn weighted<'a>(
    potentials: &'a [(SpawnData, i32)],
    random: &dyn RandomSource,
) -> Option<&'a SpawnData> {
    let total: i32 = potentials.iter().map(|(_, weight)| weight).sum();
    if total <= 0 {
        return None;
    }
    let mut selection = random.next_int(total);
    for (data, weight) in potentials {
        selection -= *weight;
        if selection < 0 {
            return Some(data);
        }
    }
    None
}

/// `BaseSpawner.lambda$serverTick$0`：随机落点。
///
/// The random spawn position of `lambda$serverTick$0`.
fn random_position(world: &World, pos: BlockPos, spawn_range: i32) -> Vec3 {
    let random = world.random();
    let range = f64::from(spawn_range);
    let x = f64::from(pos.x) + (random.next_double() - random.next_double()) * range + 0.5;
    let y = f64::from(pos.y + random.next_int(3) - 1);
    let z = f64::from(pos.z) + (random.next_double() - random.next_double()) * range + 0.5;
    Vec3::new(x, y, z)
}

/// `valueInput.read("Pos", Vec3.CODEC)`：恰好 3 个 double 的列表，否则 `None`。
///
/// `valueInput.read("Pos", Vec3.CODEC)`: a list of exactly three doubles, else `None`.
fn read_pos(entity: &Tag) -> Option<Vec3> {
    let Some(Tag::List(values)) = tag::field(entity, "Pos") else {
        return None;
    };
    if values.len() != 3 {
        return None;
    }
    let mut pos = [0.0f64; 3];
    for (index, value) in values.iter().enumerate() {
        pos[index] = tag::java_f64(value).ok()?;
    }
    Some(Vec3::new(pos[0], pos[1], pos[2]))
}

/// `getEntities(精确类, AABB(pos).inflate(spawnRange), NO_SPECTATORS).size()`：范围内
/// 精确实体类型的数量（含还没被移除的）。
///
/// The nearby count of the exact entity type within the spawn AABB.
fn count_nearby(world: &World, pos: BlockPos, spawn_range: i32, kind: MobType) -> i32 {
    let area = Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    )
    .inflate(
        f64::from(spawn_range),
        f64::from(spawn_range),
        f64::from(spawn_range),
    );
    let mut count = 0;
    world
        .level()
        .entities_table()
        .for_each_in(area, &mut |public| {
            if !public.removed
                && matches!(&public.kind, KindPublic::Mob(mob) if mob.mob_type == kind)
            {
                count += 1;
            }
        });
    count
}
