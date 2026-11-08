//! 实体选择器的执行。Bamboo 只有玩家，`@e` 也只返回玩家。
//!
//! Entity-selector execution. Bamboo only has players, so `@e` returns players too.

use std::sync::atomic::{AtomicU64, Ordering};

use mc_bamboo_command::{
    EntitySelector, GameMode as CommandGameMode, NumberBounds, SortOrder, permission, wrap_degrees,
};
use mc_bamboo_core::{Aabb, Vec3};
use mc_bamboo_data::registries::entity_type;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::{
    command::feedback::{CommandFail, fail},
    inbound::PlayerId,
    player::Player,
    region::Region,
};

/// 官方 `EntityArgument.getPlayers`：空结果是 `argument.entity.notfound.player`。
///
/// Official `EntityArgument.getPlayers`: an empty result is `argument.entity.notfound.player`.
pub(crate) fn players(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    let found = find(region, source, level, selector)?;
    if found.is_empty() {
        Err(fail("argument.entity.notfound.player"))
    } else {
        Ok(found)
    }
}

/// 官方 `EntityArgument.getEntities`。没有其他实体时，`@e` 的候选就是玩家。
///
/// Official `EntityArgument.getEntities`. With no other entities, `@e` candidates are the players.
pub(crate) fn entities(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    let found = find(region, source, level, selector)?;
    if found.is_empty() {
        Err(fail("argument.entity.notfound.entity"))
    } else {
        Ok(found)
    }
}

/// 官方 `EntitySelector.findSingleEntity`。多于一个是 `argument.entity.toomany`。
///
/// Official `EntitySelector.findSingleEntity`. More than one is `argument.entity.toomany`.
pub(crate) fn one_entity(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<PlayerId, CommandFail> {
    let found = find(region, source, level, selector)?;
    match found.as_slice() {
        [only] => Ok(*only),
        [] => Err(fail("argument.entity.notfound.entity")),
        _ => Err(fail("argument.entity.toomany")),
    }
}

/// 消息里的选择器。空结果不报错，官方 `findEntities` 也是这样。
///
/// A selector inside a message. An empty result is not an error, matching official `findEntities`.
pub(crate) fn mention(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    find(region, source, level, selector)
}

fn find(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    if selector.uses_selectors && level < permission::GAMEMASTERS {
        return Err(fail("argument.entity.selector.not_allowed"));
    }
    let Some(origin_player) = region.players.get(&source) else {
        return Ok(Vec::new());
    };
    if let Some(name) = &selector.player_name {
        return Ok(region
            .players
            .iter()
            .find(|(_, player)| player.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
            .into_iter()
            .collect());
    }
    if let Some(uuid) = selector.uuid {
        return Ok(region
            .players
            .iter()
            .find(|(_, player)| player.uuid == uuid)
            .map(|(id, _)| *id)
            .into_iter()
            .collect());
    }

    let origin = selector.position(origin_player.position());
    let volume = volume_box(origin, selector);
    if selector.current_entity {
        return Ok(matches_player(origin_player, selector, origin, volume)
            .then_some(source)
            .into_iter()
            .collect());
    }
    if selector.includes_entities
        && selector
            .type_name
            .as_deref()
            .is_some_and(|name| !is_player_type(name))
    {
        return Ok(Vec::new());
    }

    let mut ids = Vec::new();
    let cap = selector.result_limit().max(0) as usize;
    for (id, player) in &region.players {
        if matches_player(player, selector, origin, volume) {
            ids.push(*id);
            if ids.len() >= cap {
                break;
            }
        }
    }
    sort_and_limit(region, &mut ids, selector, origin);
    Ok(ids)
}

fn matches_player(
    player: &Player,
    selector: &EntitySelector,
    origin: Vec3,
    volume: Option<Aabb>,
) -> bool {
    for (name, inverted) in &selector.name_filters {
        if (player.name == *name) == *inverted {
            return false;
        }
    }
    for (type_name, inverted) in &selector.type_filters {
        if is_player_type(type_name) == *inverted {
            return false;
        }
    }
    for (tag, inverted) in &selector.type_tag_filters {
        if entity_tag_has(tag, entity_type::PLAYER) == *inverted {
            return false;
        }
    }
    for (mode, inverted) in &selector.game_mode_filters {
        if same_mode(player.game_mode, *mode) == *inverted {
            return false;
        }
    }
    // 玩家没有队伍和标签。正向过滤落空，反向过滤通过。
    //
    // Players have no team and no tags. A positive filter fails and an inverted one passes.
    for (_, inverted) in selector.team_filters.iter().chain(&selector.tag_filters) {
        if !inverted {
            return false;
        }
    }
    if selector
        .level
        .as_ref()
        .is_some_and(|level| !level.contains(0))
    {
        return false;
    }
    if selector
        .rotation_x
        .as_ref()
        .is_some_and(|bounds| !bounds.contains(player.pitch))
    {
        return false;
    }
    if selector
        .rotation_y
        .as_ref()
        .is_some_and(|bounds| !bounds.contains(wrap_degrees(player.yaw)))
    {
        return false;
    }
    if volume.is_some_and(|box_| !box_.intersects(player.bounding_box())) {
        return false;
    }
    if selector
        .distance
        .as_ref()
        .is_some_and(|bounds| !matches_sqr(*bounds, player.position().distance_to_sqr(origin)))
    {
        return false;
    }
    true
}

fn is_player_type(name: &str) -> bool {
    name == "minecraft:player" || name == "player"
}

fn same_mode(player: GameMode, filter: CommandGameMode) -> bool {
    player as i32 == filter.id()
}

fn entity_tag_has(tag: &str, id: i32) -> bool {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:entity_type")
        .and_then(|group| group.tags.iter().find(|entry| entry.name == tag))
        .is_some_and(|entry| entry.ids.contains(&id))
}

fn matches_sqr(bounds: NumberBounds<f64>, distance_sqr: f64) -> bool {
    if let Some(min) = bounds.min
        && distance_sqr < min * min
    {
        return false;
    }
    if let Some(max) = bounds.max
        && distance_sqr > max * max
    {
        return false;
    }
    true
}

fn volume_box(origin: Vec3, selector: &EntitySelector) -> Option<Aabb> {
    if selector.delta_x.is_none() && selector.delta_y.is_none() && selector.delta_z.is_none() {
        return None;
    }
    let dx = selector.delta_x.unwrap_or(0.0);
    let dy = selector.delta_y.unwrap_or(0.0);
    let dz = selector.delta_z.unwrap_or(0.0);
    Some(Aabb::new(
        origin.x.min(origin.x + dx),
        origin.y.min(origin.y + dy),
        origin.z.min(origin.z + dz),
        origin.x.max(origin.x + dx),
        origin.y.max(origin.y + dy),
        origin.z.max(origin.z + dz),
    ))
}

fn sort_and_limit(
    region: &Region,
    ids: &mut Vec<PlayerId>,
    selector: &EntitySelector,
    origin: Vec3,
) {
    if ids.len() > 1 {
        match selector.order {
            SortOrder::Arbitrary => {}
            SortOrder::Nearest => ids.sort_by(|&left, &right| {
                distance(region, left, origin).total_cmp(&distance(region, right, origin))
            }),
            SortOrder::Furthest => ids.sort_by(|&left, &right| {
                distance(region, right, origin).total_cmp(&distance(region, left, origin))
            }),
            // 官方 `Collections.shuffle` 用的是 `java.util.Random`，不是 `level.random`。
            //
            // Official `Collections.shuffle` uses `java.util.Random`, not `level.random`.
            SortOrder::Random => shuffle(ids),
        }
    }
    let limit = selector.max_results.max(0) as usize;
    if ids.len() > limit {
        ids.truncate(limit);
    }
}

fn distance(region: &Region, id: PlayerId, origin: Vec3) -> f64 {
    region
        .players
        .get(&id)
        .map(|player| player.position().distance_to_sqr(origin))
        .unwrap_or(f64::MAX)
}

fn shuffle(ids: &mut [PlayerId]) {
    static STATE: AtomicU64 = AtomicU64::new(0x6a09_e667_f3bc_c909);
    let mut state = STATE.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    for index in (1..ids.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let choice = (state >> 33) as usize % (index + 1);
        ids.swap(index, choice);
    }
}
