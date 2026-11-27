//! 选择器选出来的记分板名字。
//!
//! 官方 `ScoreHolderArgument` 的选择器选的是所有实体：玩家的记分板名字是玩家名，
//! 别的实体是 UUID 字符串（`Entity.getScoreboardName`）。
//!
//! Score holder names picked by a selector. The selector of official
//! `ScoreHolderArgument` selects every entity: a player's scoreboard name is the player
//! name, any other entity's is its UUID string (`Entity.getScoreboardName`).

use mc_bamboo_blocks::{
    archive,
    entity::{EntityPublic, KindPublic},
};
use mc_bamboo_protocol::text::Component;
use uuid::Uuid;

use crate::{command::select::Target, region::Region};

/// 一个非玩家实体的记分板名字。生物有自己存盘的 UUID，其余实体按实体号导出。
///
/// The scoreboard name of a non-player entity. Mobs carry their saved UUID; other
/// entities derive one from the entity id.
fn entity_holder(public: &EntityPublic) -> String {
    match &public.kind {
        KindPublic::Mob(mob) => Uuid::from_bytes(mob.uuid).to_string(),
        _ => Uuid::from_u128(archive::entity_uuid(public.uid)).to_string(),
    }
}

/// 把选中的目标换成记分板名字，顺序不变。
///
/// Turn the selected targets into scoreboard names, keeping their order.
pub(crate) fn holder_names(region: &Region, targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|target| match target {
            Target::Player(id) => region.players.get(id).map(|player| player.name.clone()),
            Target::World(uid) => region.entities.public_at(*uid).map(entity_holder),
        })
        .collect()
}

/// 一个 UUID 形式的记分板名字对应的实体的显示名：自定义名，没有就是实体类型的翻译键。
/// 这个岛上找不到这只实体时返回 `None`，调用方按纯名字显示。
///
/// The display name of the entity behind a UUID-shaped scoreboard name: its custom name,
/// or the entity type's translation key. `None` when this island has no such entity; the
/// caller then shows the bare name.
pub(crate) fn entity_display(region: &Region, holder: &str) -> Option<Component> {
    Uuid::parse_str(holder).ok()?;
    let public = region
        .entities
        .iter_public()
        .find(|public| entity_holder(public) == holder)?;
    if let KindPublic::Mob(mob) = &public.kind
        && let Some(tag) = &mob.custom_name
    {
        return Some(Component::trusted(tag.clone()));
    }
    let name = mc_bamboo_data::registries::entity_type::NAMES
        .get(usize::try_from(public.kind.entity_type()).ok()?)?;
    Some(Component::translate(
        name.replace("minecraft:", "entity.minecraft."),
        [],
    ))
}
