//! `/execute if predicate`：把命令上下文接到 `mc-bamboo-loot` 的条件求值上。
//!
//! 官方 `ExecuteCommand.checkCustomPredicate` 用 `LootContextParamSets.COMMAND` 建上下文：
//! `ORIGIN` 是来源位置，`THIS_ENTITY` 是来源实体（可以没有），随机源是世界的随机源。
//! 这里按同样的参数建 [`LootParams`]，实体侧面由 [`entity_view`] 从玩家表或实体表读，
//! 世界查询由 [`CommandLevel`] 回答。
//!
//! `/execute if predicate`: wires a command context to the condition evaluator in
//! `mc-bamboo-loot`. Official `ExecuteCommand.checkCustomPredicate` builds a
//! `LootContextParamSets.COMMAND` context: `ORIGIN` is the source position, `THIS_ENTITY`
//! the optional source entity, and the random source is the level's.

use mc_bamboo_advancements::AdvancementTree;
use mc_bamboo_blocks::{BlockState, entity::KindPublic, entity::KindState};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::registries::entity_type;
use mc_bamboo_loot::{
    AdvancementView, EffectView, EntityView, EquipmentSlot, LevelView, LootCondition, LootContext,
    LootError, LootParams, PlayerView, Position,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::packet::play::{EquipmentSlot as WireSlot, GameMode};
use serde_json::Value as Json;

use crate::{
    command::{
        data_source::{self, Kind, Source},
        feedback::{CommandFail, fail},
        select::Target,
    },
    interaction::level::game_stack_to_loot,
    player::Player,
    region::Region,
};

/// 载具链最多跟几层；官方没有上限，实体骑乘本身不会成环。
///
/// How many vehicle links are followed. Vanilla has no limit; riding never forms a cycle.
const MAX_VEHICLE_DEPTH: usize = 4;

/// 对一个内联谓词求值。`tag` 是命令里写的 SNBT 值。
///
/// Evaluate one inline predicate. `tag` is the SNBT value written in the command.
pub(crate) fn test(
    region: &Region,
    origin: Vec3,
    entity: Option<Target>,
    tag: &Tag,
) -> Result<bool, CommandFail> {
    let condition = LootCondition::parse(&tag_to_json(tag, None))
        .map_err(|_| fail("argument.resource_or_id.failed_to_parse"))?;
    let level = CommandLevel { region };
    let mut params = LootParams::new(0.0).with_origin(Position::new(origin.x, origin.y, origin.z));
    params.level = Some(&level);
    params.this_entity = entity.and_then(|target| entity_view(region, target, 0));
    let random = region.random.source();
    match condition.test(&LootContext::new(&params, random)) {
        Ok(passed) => Ok(passed),
        // 还没建模的谓词部分明确报错，不静默判假。
        // A predicate part that is not modelled fails loudly instead of testing false.
        Err(LootError::Unsupported(_) | LootError::Invalid(_)) => Err(fail("command.failed")),
        Err(_) => Err(fail("command.failed")),
    }
}

/// 把 SNBT 值转成条件解析器吃的 JSON。
///
/// 官方直接用 `NbtOps` 解码，这里借 JSON 这一层，所以有三处约定：字节 0 / 1 当布尔
/// （SNBT 的 `true` / `false` 就是它们）；键叫 `nbt` 或 `minecraft:nbt` 的复合标签转回
/// SNBT 字符串，保住数值类型（官方 `TagParser.LENIENT_CODEC` 两种写法等价）；`type`
/// 的值没写命名空间时补 `minecraft:`。
///
/// Convert an SNBT value into the JSON the condition parser reads. Vanilla decodes with
/// `NbtOps` directly; going through JSON needs three conventions: bytes 0 and 1 become
/// booleans (that is what SNBT `true` and `false` are); a compound under the key `nbt`
/// or `minecraft:nbt` goes back to an SNBT string so number types survive (both spellings
/// are equivalent under `TagParser.LENIENT_CODEC`); and a `type` value without a
/// namespace gets `minecraft:`.
pub(crate) fn tag_to_json(tag: &Tag, key: Option<&str>) -> Json {
    match tag {
        Tag::Byte(value @ (0 | 1)) => Json::Bool(*value == 1),
        Tag::Byte(value) => Json::from(*value),
        Tag::Short(value) => Json::from(*value),
        Tag::Int(value) => Json::from(*value),
        Tag::Long(value) => Json::from(*value),
        Tag::Float(value) => Json::from(*value),
        Tag::Double(value) => Json::from(*value),
        Tag::String(text) => match key {
            Some("type") if !text.contains(':') => Json::String(format!("minecraft:{text}")),
            _ => Json::String(text.clone()),
        },
        Tag::ByteArray(values) => values.iter().map(|value| Json::from(*value)).collect(),
        Tag::IntArray(values) => values.iter().map(|value| Json::from(*value)).collect(),
        Tag::LongArray(values) => values.iter().map(|value| Json::from(*value)).collect(),
        Tag::List(values) => values
            .iter()
            .map(|value| tag_to_json(value, None))
            .collect(),
        Tag::Compound(_) if matches!(key, Some("nbt" | "minecraft:nbt")) => {
            Json::String(tag.to_snbt())
        }
        Tag::Compound(entries) => Json::Object(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), tag_to_json(value, Some(name))))
                .collect(),
        ),
    }
}

/// 一个命令目标在谓词眼里的样子。目标已经不在时返回 `None`（官方的 `THIS_ENTITY` 为空）。
///
/// What a command target looks like to a predicate, or `None` once it is gone, which is
/// an absent `THIS_ENTITY` in vanilla.
pub(crate) fn entity_view(region: &Region, target: Target, depth: usize) -> Option<EntityView> {
    match target {
        Target::Player(id) => region
            .players
            .get(&id)
            .map(|player| player_view(region, player)),
        Target::World(uid) => world_entity_view(region, uid, depth),
    }
}

fn game_mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "survival",
        GameMode::Creative => "creative",
        GameMode::Adventure => "adventure",
        GameMode::Spectator => "spectator",
    }
}

fn effect_views<'a>(
    effects: impl Iterator<Item = &'a mc_bamboo_blocks::entity::mob::effects::EffectInstance>,
) -> Vec<EffectView> {
    effects
        .map(|instance| EffectView {
            effect: instance.effect,
            amplifier: instance.amplifier,
            duration: instance.duration,
            ambient: instance.ambient,
            visible: instance.visible,
        })
        .collect()
}

fn player_view(region: &Region, player: &Player) -> EntityView {
    let mut view = EntityView::new(entity_type::PLAYER);
    view.is_living = true;
    view.is_on_ground = player.on_ground;
    view.is_on_fire = crate::player_environment::is_on_fire(player);
    view.is_sneaking = player.sync.is_crouching();
    view.is_sprinting = player.sync.is_sprinting();
    view.is_fall_flying = player.sync.is_fall_flying();
    view.is_flying = view.is_fall_flying;
    let position = player.position();
    view.position = Some(Position::new(position.x, position.y, position.z));
    view.equipment[EquipmentSlot::Mainhand.index()] =
        Some(game_stack_to_loot(player.inventory.held(Hand::Main)));
    view.equipment[EquipmentSlot::Offhand.index()] =
        Some(game_stack_to_loot(player.inventory.held(Hand::Off)));
    view.effects = effect_views(player.effects.iter());
    view.team = region
        .global
        .scoreboard
        .team_of(&player.name)
        .map(str::to_owned);
    view.scoreboard_name = Some(player.name.clone());
    view.tick_count = Some(player.entity_ticks);
    view.player = Some(Box::new(PlayerView {
        experience_level: player.experience.level,
        food_level: player.health.food.food_level,
        saturation: player.health.food.saturation,
        game_mode: game_mode_name(player.game_mode),
        input: None,
        looking_at: None,
    }));
    view
}

fn loot_slot(slot: WireSlot) -> EquipmentSlot {
    match slot {
        WireSlot::MainHand => EquipmentSlot::Mainhand,
        WireSlot::OffHand => EquipmentSlot::Offhand,
        WireSlot::Feet => EquipmentSlot::Feet,
        WireSlot::Legs => EquipmentSlot::Legs,
        WireSlot::Chest => EquipmentSlot::Chest,
        WireSlot::Head => EquipmentSlot::Head,
        WireSlot::Body => EquipmentSlot::Body,
        WireSlot::Saddle => EquipmentSlot::Saddle,
    }
}

fn world_entity_view(region: &Region, uid: i32, depth: usize) -> Option<EntityView> {
    let public = region.entities.public_at(uid)?;
    let mut view = EntityView::new(public.kind.entity_type());
    view.position = Some(Position::new(
        public.position.x,
        public.position.y,
        public.position.z,
    ));
    view.movement = Some(Position::new(
        public.delta.x,
        public.delta.y,
        public.delta.z,
    ));
    view.is_on_ground = public.on_ground;
    if let KindPublic::Mob(mob) = &public.kind {
        view.is_living = true;
        view.is_baby = mob.baby;
        view.is_on_fire = mob.on_fire;
        view.effects = effect_views(mob.status_effects.iter());
        for (slot, stack) in &mob.equipment {
            view.equipment[loot_slot(*slot).index()] = Some(game_stack_to_loot(stack));
        }
        // 官方 `Entity.getScoreboardName()`：非玩家实体用带连字符的 UUID 字符串。
        // Official `Entity.getScoreboardName()`: the hyphenated UUID string for a
        // non-player entity.
        let name = uuid::Uuid::from_bytes(mob.uuid).to_string();
        view.team = region.global.scoreboard.team_of(&name).map(str::to_owned);
        view.scoreboard_name = Some(name);
    }
    let row = region
        .entities
        .row_of(uid)
        .and_then(|row| region.entities.row_at(row));
    if let Some(KindState::Mob(mob)) = row.map(|row| &row.state.kind) {
        view.scoreboard_tags = mob.tags.clone();
    }
    view.nbt = data_source::read_view(
        &region.global,
        region,
        &Source {
            kind: Kind::Entity,
            block: None,
            entity: Some(Target::World(uid)),
            storage: None,
        },
    )
    .ok();
    if depth < MAX_VEHICLE_DEPTH
        && let Some(vehicle) = public.vehicle
    {
        view.vehicle = world_entity_view(region, vehicle, depth + 1).map(Box::new);
    }
    Some(view)
}

/// 命令谓词要问的世界：方块、群系、流体走区块表，天气和积分板走岛上的全局快照，
/// 进度走玩家表。结构、天空可见、亮度、烟雾和统计 / 配方没接，问到就报不支持。
///
/// The world a command predicate can ask. Blocks, biomes and fluids come from the chunk
/// store, weather and the scoreboard from the island's global snapshot, advancements from
/// the player table. Structures, sky visibility, light, smoke, statistics and recipes are
/// not wired in and report unsupported.
pub(crate) struct CommandLevel<'a> {
    pub(crate) region: &'a Region,
}

impl LevelView for CommandLevel<'_> {
    fn is_loaded(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        Some(self.region.chunks.block(BlockPos::new(x, y, z)).is_some())
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        self.region
            .chunks
            .block(BlockPos::new(x, y, z))
            .map(BlockState)
    }

    fn biome(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        biome_at(self.region, BlockPos::new(x, y, z))
    }

    fn can_see_sky(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        None
    }

    fn is_raining(&self) -> Option<bool> {
        Some(self.region.global.block_weather.is_raining())
    }

    fn is_thundering(&self) -> Option<bool> {
        Some(self.region.global.block_weather.is_thundering())
    }

    fn day_time(&self) -> Option<i64> {
        None
    }

    fn game_time(&self) -> Option<i64> {
        None
    }

    fn dimension(&self) -> Option<&'static str> {
        Some(crate::region::DIMENSION)
    }

    fn score(&self, holder: &str, objective: &str) -> Option<Option<i32>> {
        let board = &self.region.global.scoreboard;
        if !board.objective_exists(objective) {
            return Some(None);
        }
        Some(board.score_value(holder, objective))
    }

    fn player_advancement(&self, player: &str, id: &str) -> Option<Option<AdvancementView>> {
        let player = self
            .region
            .players
            .values()
            .find(|candidate| candidate.name == player)?;
        let tree = AdvancementTree::global();
        let Some(index) = tree.index(id) else {
            return Some(None);
        };
        // 官方 `getOrStartProgress`：还没有记录时是一份全未完成的进度。
        // Official `getOrStartProgress`: no record yet means nothing is done.
        let started;
        let progress = match player.advancements.progress.get(&index) {
            Some(progress) => progress,
            None => {
                started = mc_bamboo_advancements::progress_file::AdvancementProgress::start(
                    &tree.nodes[index].advancement,
                );
                &started
            }
        };
        Some(Some(AdvancementView {
            done: progress.is_done(),
            criteria: progress
                .criteria
                .iter()
                .map(|(name, obtained)| (name.clone(), obtained.is_some()))
                .collect(),
        }))
    }
}

/// `Level.getBiome(pos)` 的注册表 ID；区块没加载或者高度在世界外时返回 `None`。
///
/// The registry id of `Level.getBiome(pos)`, or `None` when the chunk is not loaded or
/// the height is outside the world.
pub(crate) fn biome_at(region: &Region, pos: BlockPos) -> Option<i32> {
    region
        .chunks
        .biome_at(pos)
        .and_then(|id| i32::try_from(id).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snbt_converts_with_the_three_conventions() {
        let tag = mc_bamboo_nbt::parse_snbt(
            "{type:\"entity_properties\",flag:true,count:3b,predicate:{nbt:{Health:10.0f}}}",
        )
        .expect("valid SNBT");
        let json = tag_to_json(&tag, None);
        assert_eq!(json["type"], "minecraft:entity_properties");
        assert_eq!(json["flag"], true);
        assert_eq!(json["count"], 3);
        assert_eq!(json["predicate"]["nbt"], "{Health:10.0f}");
    }
}
