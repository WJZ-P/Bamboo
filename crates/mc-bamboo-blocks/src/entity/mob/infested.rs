//! 虫蚀方块：`InfestedBlock` 和宿主方块之间的对应关系，以及挖掘、爆炸时的 `spawnInfestation`。
//!
//! 宿主表来自生成器导出的每个 `InfestedBlock` 的 `hostBlock` 字段；没有单独写死七个方块 ID。
//! 方块本身的行为（硬度、战利品表）在 `mc-bamboo-data` 的表里，这里只管生成蠹虫和钻回石头。
//!
//! Infested blocks: the host↔infested mapping from the generator's `hostBlock`
//! fields, plus `spawnInfestation` on mining and explosions. The block's own
//! hardness and loot table live in the generated data.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{
    BlockAncestor, FieldValue, blocks, damage_type_id, entity_types, registries::entity_type, tags,
};
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};

use crate::{
    BlockState, World,
    entity::{Entity, mob::MobType},
};

/// 官方 `InfestedBlock.isCompatibleHostBlock`：这个方块有没有对应的虫蚀版本。
///
/// Vanilla `InfestedBlock.isCompatibleHostBlock`: whether a block has an infested version.
pub fn is_compatible_host_block(state: BlockState) -> bool {
    infested_state_by_host(state).is_some()
}

/// 官方 `InfestedBlock.infestedStateByHost`：默认状态，再把宿主状态上的属性逐个抄过去。
/// 宿主里只有 `infested_deepslate` 有 `axis` 属性，其余都是单状态方块。
///
/// Vanilla `InfestedBlock.infestedStateByHost`: the default state with the host's
/// properties copied over. Only `infested_deepslate` has a property (`axis`).
pub fn infested_state_by_host(host: BlockState) -> Option<BlockState> {
    let host_id = host.block_id();
    let infested = blocks::BLOCKS.iter().find(|block| {
        matches!(
            block.class,
            mc_bamboo_data::BlockClass::InfestedBlock | mc_bamboo_data::BlockClass::InfestedRotatedPillarBlock
        ) && matches!(block.field("hostBlock"), Some(FieldValue::Block(id)) if id as usize == host_id)
    })?;
    Some(host.copied_onto(infested))
}

/// 官方 `InfestedBlock.hostStateByInfested`：默认状态，再把虫蚀状态上的属性抄过去。
///
/// Vanilla `InfestedBlock.hostStateByInfested`: the default state with the infested
/// state's properties copied over.
pub fn host_state_by_infested(infested: BlockState) -> Option<BlockState> {
    let Some(FieldValue::Block(host_id)) = infested.block().field("hostBlock") else {
        return None;
    };
    let host = blocks::BLOCKS.get(host_id as usize)?;
    Some(BlockState(infested.0).copied_onto(host))
}

/// 这一格是不是虫蚀方块（含 `InfestedRotatedPillarBlock`）。
///
/// Whether this state is an infested block, including the rotated pillar variant.
pub fn is_infested(state: BlockState) -> bool {
    state.is_a(BlockAncestor::InfestedBlock)
}

/// 官方 `InfestedBlock.spawnInfestation`：`EntityTypes.SILVERFISH.create(level, TRIGGERED)`，
/// `snapTo(x+0.5, y, z+0.5, 0, 0)`，`addFreshEntity`，最后 `spawnAnim`（实体事件 20）。
///
/// Vanilla `InfestedBlock.spawnInfestation`: create a silverfish with reason
/// `TRIGGERED`, snap it to the block, add it to the level and play its spawn animation.
pub fn spawn_infestation(world: &mut World<'_>, pos: BlockPos) {
    let mut entity = world.prepare_entity(Entity::by_mob_type(
        MobType::Silverfish,
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        ),
    ));
    // 官方 `EntityType.create` 之后 `snapTo(..., 0.0F, 0.0F)` 把构造时的随机朝向清成 0。
    // `snapTo(..., 0.0F, 0.0F)` resets the constructor's random facing to zero.
    entity.y_rot = 0.0;
    entity.with_mob(|_, mob| {
        mob.x_rot = 0.0;
        mob.y_head_rot = 0.0;
        mob.y_body_rot = 0.0;
    });
    let uid = world.add_prepared_entity(entity);
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(|_, mob| {
            mob.events.push(super::MobEvent::EntityEvent { event: 20 });
        });
    });
}

/// 官方 `InfestedBlock.spawnAfterBreak`：`GameRules.BLOCK_DROPS` 开着、工具上没有
/// `#prevents_infested_spawns`（精准采集）时才刷蠹虫。
///
/// Vanilla `InfestedBlock.spawnAfterBreak`: spawn a silverfish only when
/// `GameRules.BLOCK_DROPS` is on and the tool lacks `prevents_infested_spawns`.
pub fn spawn_after_break(
    world: &mut World<'_>,
    state: BlockState,
    pos: BlockPos,
    tool: &ItemStack,
) {
    if !is_infested(state) || !world.block_game_rules().block_drops {
        return;
    }
    let patch = tool.patch();
    let prevented = matches!(
        patch.get(mc_bamboo_data::registries::data_component_type::ENCHANTMENTS),
        Some(ComponentValue::Enchantments(levels))
            if levels.iter().any(|&(id, level)| {
                level > 0 && prevents_infested_spawns(id)
            })
    );
    if !prevented {
        spawn_infestation(world, pos);
    }
}

/// `spawnInfestation` 用的实体类型，给方块 oracle 的出生实体表核对。
///
/// The entity type spawned by `spawnInfestation`, for the block oracle's spawn check.
pub const SPAWNED_TYPE: i32 = entity_type::SILVERFISH;

/// 蠹虫 / 末影螨共用的尺寸：`EntityTypes` 里的 0.4×0.3，眼高由生成器导出。
///
/// Dimensions shared by silverfish and endermites: 0.4×0.3 from `EntityTypes`,
/// with the eye height exported by the generator.
pub fn dimensions(kind: MobType) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    let eye = if kind == MobType::Silverfish {
        entity_types::SILVERFISH_EYE_HEIGHT
    } else {
        entity_types::ENDERMITE_EYE_HEIGHT
    };
    (facts.width, facts.height, eye)
}

/// `DamageTypeTags.ALWAYS_TRIGGERS_SILVERFISH` 的 ID 列表（生成器导出的标签表）。
///
/// The damage types in `DamageTypeTags.ALWAYS_TRIGGERS_SILVERFISH`.
pub fn damage_always_triggers_silverfish(kind_name: &str) -> bool {
    let Some(id) = damage_type_id(kind_name) else {
        return false;
    };
    tag_contains(
        "minecraft:damage_type",
        "minecraft:always_triggers_silverfish",
        id,
    )
}

/// `EnchantmentTags.PREVENTS_INFESTED_SPAWNS` 里有没有这个附魔 ID。
///
/// Whether an enchantment id is in `EnchantmentTags.PREVENTS_INFESTED_SPAWNS`.
pub fn prevents_infested_spawns(enchantment: i32) -> bool {
    tag_contains(
        "minecraft:enchantment",
        "minecraft:prevents_infested_spawns",
        enchantment,
    )
}

fn tag_contains(registry: &str, tag: &str, id: i32) -> bool {
    tags::TAG_GROUPS
        .iter()
        .filter(|group| group.registry == registry)
        .flat_map(|group| group.tags.iter())
        .find(|entry| entry.name == tag)
        .is_some_and(|entry| entry.ids.contains(&id))
}
