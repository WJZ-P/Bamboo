//! 实体选择器的执行。玩家和非玩家实体都进候选，过滤和排序按官方 `EntitySelector`。
//!
//! Entity-selector execution. Players and non-player entities are all candidates;
//! filtering and ordering follow official `EntitySelector`.

use std::sync::{Mutex, OnceLock};

use mc_bamboo_blocks::entity::{EntityPublic, JavaRandom};
use mc_bamboo_command::{
    EntitySelector, GameMode as CommandGameMode, NumberBounds, SortOrder, permission, wrap_degrees,
};
use mc_bamboo_core::{Aabb, Vec3};
use mc_bamboo_data::registries::entity_type;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::{
    command::{
        feedback::{CommandFail, fail},
        source,
    },
    inbound::PlayerId,
    player::Player,
    region::Region,
};

pub(crate) use source::Entity as Target;

/// 官方 `EntityArgument.getPlayers`：空结果是 `argument.entity.notfound.player`。
///
/// Official `EntityArgument.getPlayers`: an empty result is `argument.entity.notfound.player`.
pub(crate) fn players(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    let found = find(region, None, source, level, selector)?;
    let mut out = Vec::with_capacity(found.len());
    for target in found {
        // `players_only` 参数在解析时已经排除 `@e`；UUID 形式仍可能命中非玩家实体，
        // 官方 `getPlayers` 也会把不是玩家的实体过滤掉。
        //
        // The `players_only` argument rules out `@e` at parse time; a UUID may still
        // name a non-player entity, and official `getPlayers` filters that out too.
        if let Target::Player(id) = target {
            out.push(id);
        }
    }
    if out.is_empty() {
        Err(fail("argument.entity.notfound.player"))
    } else {
        Ok(out)
    }
}

/// 官方 `EntityArgument.getOptionalPlayers`：空结果不报错，但 `scores=`/`team=` 照常过滤。
///
/// Official `EntityArgument.getOptionalPlayers`: an empty result is not an error,
/// while `scores=` / `team=` still filter.
pub(crate) fn optional_players(
    region: &Region,
    board: &crate::scoreboard::Scoreboard,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    Ok(find(region, Some(board), source, level, selector)?
        .into_iter()
        .filter_map(|target| match target {
            Target::Player(id) => Some(id),
            Target::World(_) => None,
        })
        .collect())
}

/// 官方 `EntityArgument.getEntities`：玩家和非玩家实体都要。
///
/// Official `EntityArgument.getEntities`: players and non-player entities alike.
pub(crate) fn targets(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<Target>, CommandFail> {
    let found = find(region, None, source, level, selector)?;
    if found.is_empty() {
        Err(fail("argument.entity.notfound.entity"))
    } else {
        Ok(found)
    }
}

/// 官方 `EntityArgument.getEntity`（单一实体）。空结果和多个结果各有自己的报错。
///
/// Official `EntityArgument.getEntity` (single). Empty and multiple results have
/// their own errors.
pub(crate) fn one_target(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Target, CommandFail> {
    let found = find(region, None, source, level, selector)?;
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
    Ok(find(region, None, source, level, selector)?
        .into_iter()
        .filter_map(|target| match target {
            Target::Player(id) => Some(id),
            // `ComponentUtils.formatList` 会列出实体的显示名；文本消息的参数只认玩家，
            // 这里按玩家 ID 的接口返回，非玩家实体不参与。
            //
            // `ComponentUtils.formatList` lists entity display names; message
            // arguments here have a player-id interface, so others are skipped.
            Target::World(_) => None,
        })
        .collect())
}

/// 带计分板的选择器。`scores=`、`team=` 的判定要查全服状态。
///
/// A selector that sees the scoreboard. `scores=` and `team=` need the server-wide state.
pub(crate) fn with_scoreboard(
    region: &Region,
    board: &crate::scoreboard::Scoreboard,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    optional_players(region, board, source, level, selector)
}

/// 官方 `EntitySelector.findEntities`：先按名字/UUID 短路，再收集候选、排序、截断。
///
/// Official `EntitySelector.findEntities`: short-circuit on a name/UUID, then
/// collect candidates, sort and truncate.
fn find(
    region: &Region,
    board: Option<&crate::scoreboard::Scoreboard>,
    id: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<Target>, CommandFail> {
    let source = source::of(region, id);
    find_with_source(region, board, source, level, selector)
}

/// 给 `/execute` 用：从已经改过的执行上下文（位置/朝向/执行者）出发选实体。
///
/// For `/execute`: select entities from an already modified context.
pub(crate) fn targets_with(
    region: &Region,
    source: source::Source,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<Target>, CommandFail> {
    find_with_source(region, None, source, level, selector)
}

fn find_with_source(
    region: &Region,
    board: Option<&crate::scoreboard::Scoreboard>,
    source: source::Source,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<Target>, CommandFail> {
    if selector.uses_selectors && level < permission::GAMEMASTERS {
        return Err(fail("argument.entity.selector.not_allowed"));
    }
    let origin = selector.position(source.position);
    let volume = volume_box(origin, selector);
    if let Some(name) = &selector.player_name {
        return Ok(player_by_name(region, name).into_iter().collect());
    }
    if let Some(uuid) = selector.uuid {
        return Ok(entity_by_uuid(region, uuid).into_iter().collect());
    }
    if selector.current_entity {
        let Some(entity) = source.entity else {
            // 官方 `@s` 在来源没有实体时返回空列表，随后 `getEntities` 报 notfound。
            //
            // Official `@s` with no source entity returns an empty list; `getEntities`
            // then reports notfound.
            return Ok(Vec::new());
        };
        return Ok(matches(region, entity, selector, origin, volume, board)
            .then_some(entity)
            .into_iter()
            .collect());
    }
    // 官方 `limitToType`：`@a` / `@p` / `@r` / `type=player` / `level=` 把候选缩到玩家；
    // 别的 `type=` 只是把候选缩到那一种实体，仍然要遍历非玩家实体。
    // Official `limitToType`: `@a` / `@p` / `@r` / `type=player` / `level=` narrow
    // candidates to players; another `type=` narrows to that entity type and still
    // needs the non-player entities.
    let players_only = !selector.includes_entities;
    let mut found = Vec::new();
    // 官方 `ServerLevel.getEntities` 按实体 ID 升序遍历世界里的实体；Bamboo 的玩家
    // ID 从 1 起、非玩家实体从 `FIRST_ENTITY_ID` 起，玩家在前。
    //
    // Official `ServerLevel.getEntities` walks entities in ascending id order;
    // Bamboo's player ids start at 1 and non-player ids at `FIRST_ENTITY_ID`.
    let limited_type = positive_type(selector);
    let skip_players =
        selector.level.is_some() || limited_type.is_some_and(|id| id != entity_type::PLAYER);
    let skip_entities = players_only || limited_type.is_some_and(|id| id == entity_type::PLAYER);
    if !skip_players {
        for player in region.players.keys() {
            if matches(
                region,
                Target::Player(*player),
                selector,
                origin,
                volume,
                board,
            ) {
                found.push(Target::Player(*player));
                if found.len() >= result_limit(selector) {
                    return Ok(sort_and_limit(region, found, selector, origin));
                }
            }
        }
    }
    if !skip_entities {
        for public in region.entities.iter_public() {
            if public.removed {
                continue;
            }
            let target = Target::World(public.uid);
            if matches(region, target, selector, origin, volume, board) {
                found.push(target);
                if found.len() >= result_limit(selector) {
                    return Ok(sort_and_limit(region, found, selector, origin));
                }
            }
        }
    }
    Ok(sort_and_limit(region, found, selector, origin))
}

/// 官方 `Entity.getPlainTextName()`：自定义名字取组件的纯文本，否则取类型翻译键。
/// `typeName()` 是按注册表 ID 查的语言键，Bamboo 没有语言表，用实体的英文类名。
///
/// Official `Entity.getPlainTextName()`: the custom name's plain text, or the
/// type translation key. Bamboo has no language file, so the English type name
/// stands in for the key.
/// 官方 `EntitySelectorParser.limitToType`：正值的 `type=` 已经把候选缩到一个类型；
/// 这里用它跳过肯定不匹配的另一类实体（过滤仍会逐项复核）。
///
/// Official `EntitySelectorParser.limitToType`: a positive `type=` already
/// narrows the candidate set; skip the other class here (filtering still checks
/// each candidate).
fn positive_type(selector: &EntitySelector) -> Option<i32> {
    let name = selector.type_name.as_deref()?;
    mc_bamboo_data::registries::entity_type::NAMES
        .iter()
        .position(|entry| *entry == name || entry.strip_prefix("minecraft:") == Some(name))
        .and_then(|id| i32::try_from(id).ok())
}

/// 官方 `getResultLimit`：`arbitrary` 时才按 `limit` 提前收手。
fn result_limit(selector: &EntitySelector) -> usize {
    let limit = if matches!(selector.order, SortOrder::Arbitrary) {
        selector.max_results
    } else {
        i32::MAX
    };
    usize::try_from(limit.max(0)).unwrap_or(usize::MAX)
}

fn player_by_name(region: &Region, name: &str) -> Option<Target> {
    region
        .players
        .iter()
        .find(|(_, player)| player.name.eq_ignore_ascii_case(name))
        .map(|(id, _)| Target::Player(*id))
}

fn entity_by_uuid(region: &Region, uuid: uuid::Uuid) -> Option<Target> {
    let bytes = uuid.as_bytes();
    region
        .players
        .iter()
        .find(|(_, player)| player.uuid.as_bytes() == bytes)
        .map(|(id, _)| Target::Player(*id))
        .or_else(|| {
            region.entities.iter_public().find_map(|public| {
                let archive = match &public.kind {
                    // 生物的存档 `UUID` 和网络层的 UUID 是同一份。
                    // A mob's archive `UUID` is the same as its network UUID.
                    mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.uuid),
                    // 其它实体的网络 UUID 由实体表按 ID 派生。
                    // Other entities derive their network UUID from the entity id.
                    _ => Some(mc_bamboo_blocks::archive::entity_uuid(public.uid).to_be_bytes()),
                };
                (archive.as_ref() == Some(bytes)).then_some(Target::World(public.uid))
            })
        })
}

/// 一个候选过不过全部选项。
fn matches(
    region: &Region,
    target: Target,
    selector: &EntitySelector,
    origin: Vec3,
    volume: Option<Aabb>,
    board: Option<&crate::scoreboard::Scoreboard>,
) -> bool {
    let (player, public, row) = match target {
        Target::Player(id) => (region.players.get(&id), None, None),
        Target::World(uid) => (
            None,
            region.entities.public_at(uid),
            region
                .entities
                .row_of(uid)
                .and_then(|row| region.entities.row_at(row)),
        ),
    };
    let Some(facts) = Facts::of(player, public, row) else {
        return false;
    };
    for (name, inverted) in &selector.name_filters {
        if (facts.plain_name.as_str() == name.as_str()) == *inverted {
            return false;
        }
    }
    for (type_name, inverted) in &selector.type_filters {
        if is_same_type(type_name, facts.entity_type) == *inverted {
            return false;
        }
    }
    for (tag, inverted) in &selector.type_tag_filters {
        if entity_tag_has(tag, facts.entity_type) == *inverted {
            return false;
        }
    }
    for (mode, inverted) in &selector.game_mode_filters {
        if facts.game_mode.is_some_and(|mode_| same_mode(mode_, *mode)) == *inverted {
            return false;
        }
    }
    for (team, inverted) in &selector.team_filters {
        let in_team = match player {
            Some(player) => board
                .and_then(|board| board.team_of(&player.name))
                .is_some_and(|name| name == team),
            None => false,
        };
        if in_team == *inverted {
            return false;
        }
    }
    for (tag, inverted) in &selector.tag_filters {
        // 官方 `Entity.entityTags()`：空字符串判断标签集合是否为空，否则查集合里有没有它。
        // Official `Entity.entityTags()`: the empty string tests whether the set
        // is empty; any other value is looked up in the set.
        let has = if tag.is_empty() {
            !facts.tags.is_empty()
        } else {
            facts.tags.iter().any(|entry| entry == tag)
        };
        if has == *inverted {
            return false;
        }
    }
    // `level=` 只对 `ServerPlayer.experienceLevel` 生效：非玩家实体永远不匹配，
    // Bamboo 的玩家等级恒为 0。
    // `level=` tests `ServerPlayer.experienceLevel` only: non-player entities
    // never match; Bamboo players are always level 0.
    if selector
        .level
        .as_ref()
        .is_some_and(|bounds| player.is_none() || !bounds.contains(0))
    {
        return false;
    }
    if selector
        .rotation_x
        .as_ref()
        .is_some_and(|bounds| !bounds.contains(facts.pitch))
    {
        return false;
    }
    if selector
        .rotation_y
        .as_ref()
        .is_some_and(|bounds| !bounds.contains(wrap_degrees(facts.yaw)))
    {
        return false;
    }
    if volume.is_some_and(|box_| !box_.intersects(facts.bounding_box)) {
        return false;
    }
    if selector
        .distance
        .as_ref()
        .is_some_and(|bounds| !matches_sqr(*bounds, facts.position.distance_to_sqr(origin)))
    {
        return false;
    }
    for (objective, bounds) in &selector.score_filters {
        let Some(board) = board else {
            return false;
        };
        if !board.score_in(facts.score_name, objective, |value| bounds.contains(value)) {
            return false;
        }
    }
    true
}

/// 一个候选在过滤时要读的字段。
struct Facts<'a> {
    plain_name: String,
    entity_type: i32,
    game_mode: Option<GameMode>,
    tags: &'a [String],
    pitch: f32,
    yaw: f32,
    bounding_box: Aabb,
    position: Vec3,
    score_name: &'a str,
}

impl<'a> Facts<'a> {
    fn of(
        player: Option<&'a Player>,
        public: Option<&'a EntityPublic>,
        row: Option<&'a mc_bamboo_blocks::entity::EntityRow>,
    ) -> Option<Self> {
        match (player, public, row) {
            (Some(player), _, _) => Some(Self {
                plain_name: player.name.clone(),
                entity_type: entity_type::PLAYER,
                game_mode: Some(player.game_mode),
                tags: &[],
                pitch: player.pitch,
                yaw: player.yaw,
                bounding_box: player.bounding_box(),
                position: player.position(),
                score_name: player.name.as_str(),
            }),
            (None, Some(public), row) => {
                let (pitch, yaw, tags) = match &public.kind {
                    mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                        (mob.x_rot, public.yaw, mob_tags(row))
                    }
                    _ => (0.0, public.yaw, &[][..]),
                };
                Some(Self {
                    // `Entity.getPlainTextName()`：没有自定义名字时用类型的翻译键。
                    // `Entity.getPlainTextName()`: the type's translation key when unnamed.
                    plain_name: plain_name_of(public, row),
                    entity_type: public.kind.entity_type(),
                    game_mode: None,
                    tags,
                    pitch,
                    yaw,
                    bounding_box: public.bounding_box(),
                    position: public.position,
                    // `ScoreHolder` 对非玩家实体用它的 UUID 字符串；Bamboo 的计分板
                    // 只有玩家名字，实体在这个名字下永远没有分数。
                    // `ScoreHolder` uses the entity UUID string; Bamboo's scoreboard
                    // only has player names, so an entity never has a score here.
                    score_name: "",
                })
            }
            (None, None, _) => None,
        }
    }
}

fn mob_tags(row: Option<&mc_bamboo_blocks::entity::EntityRow>) -> &[String] {
    // `Entity.entityTags()` 存在存档那一半；公开那一半没有带出来。
    // `Entity.entityTags()` lives in the archive half, not the public half.
    match row.map(|row| &row.state.kind) {
        Some(mc_bamboo_blocks::entity::KindState::Mob(mob)) => &mob.tags,
        _ => &[],
    }
}

/// 官方 `Entity.getPlainTextName()`：自定义名字取组件的纯文本，否则取类型翻译键。
/// `typeName()` 是按注册表 ID 查的语言键，Bamboo 没有语言表，用实体的英文类名。
///
/// Official `Entity.getPlainTextName()`: the custom name's plain text, or the
/// type translation key. Bamboo has no language file, so the English type name
/// stands in for the key.
fn plain_name_of(
    public: &EntityPublic,
    row: Option<&mc_bamboo_blocks::entity::EntityRow>,
) -> String {
    if let Some(mc_bamboo_blocks::entity::KindState::Mob(mob)) = row.map(|row| &row.state.kind)
        && let Some(tag) = &mob.custom_name
    {
        return match tag {
            mc_bamboo_nbt::Tag::String(text) => text.clone(),
            mc_bamboo_nbt::Tag::Compound(entries) => entries
                .iter()
                .find(|(key, _)| key == "text")
                .and_then(|(_, value)| match value {
                    mc_bamboo_nbt::Tag::String(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            _ => String::new(),
        };
    }
    match public.kind.entity_type() {
        entity_type::COW => "Cow",
        entity_type::ITEM => "Item",
        entity_type::FALLING_BLOCK => "Falling Block",
        entity_type::TNT => "Block of TNT",
        entity_type::EXPERIENCE_ORB => "Experience Orb",
        entity_type::PLAYER => "Player",
        _ => "Entity",
    }
    .to_owned()
}

fn is_same_type(name: &str, id: i32) -> bool {
    if is_player_type(name) {
        return id == entity_type::PLAYER;
    }
    let full = name.strip_prefix("minecraft:").unwrap_or(name);
    mc_bamboo_data::registries::entity_type::NAMES
        .get(id as usize)
        .is_some_and(|entry| entry.strip_prefix("minecraft:") == Some(full))
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

/// 官方 `EntitySelectorParser.getSelector` / `createAabb`：
/// `dx/dy/dz` 是从原点出发的相对盒，负轴 `[d, 0]`、非负轴 `[0, d+1]`；
/// 没写 delta 但 `distance` 有上限时，用 `[-max, max+1]` 的立方盒先筛一遍。
///
/// Official `EntitySelectorParser.getSelector` / `createAabb`: deltas describe
/// `[d, 0]` on a negative axis and `[0, d+1]` otherwise; without deltas a
/// bounded `distance` prefilters with a `[-max, max+1]` cube.
fn volume_box(origin: Vec3, selector: &EntitySelector) -> Option<Aabb> {
    let box_ =
        if selector.delta_x.is_none() && selector.delta_y.is_none() && selector.delta_z.is_none() {
            let max = selector.distance?.max?;
            Aabb::new(
                origin.x - max,
                origin.y - max,
                origin.z - max,
                origin.x + max + 1.0,
                origin.y + max + 1.0,
                origin.z + max + 1.0,
            )
        } else {
            let axis = |value: Option<f64>| {
                let value = value.unwrap_or(0.0);
                if value < 0.0 {
                    (value, 0.0)
                } else {
                    (0.0, value + 1.0)
                }
            };
            let (x0, x1) = axis(selector.delta_x);
            let (y0, y1) = axis(selector.delta_y);
            let (z0, z1) = axis(selector.delta_z);
            Aabb::new(
                origin.x + x0,
                origin.y + y0,
                origin.z + z0,
                origin.x + x1,
                origin.y + y1,
                origin.z + z1,
            )
        };
    Some(box_)
}

/// 官方 `sortAndLimit`：多于一个候选才排序，最后截断到 `maxResults`。
fn sort_and_limit(
    region: &Region,
    mut found: Vec<Target>,
    selector: &EntitySelector,
    origin: Vec3,
) -> Vec<Target> {
    if found.len() > 1 {
        match selector.order {
            SortOrder::Arbitrary => {}
            SortOrder::Nearest => found.sort_by_key(|target| distance_key(region, target, origin)),
            SortOrder::Furthest => {
                found.sort_by_key(|target| std::cmp::Reverse(distance_key(region, target, origin)))
            }
            // 官方 `Collections.shuffle` 用的是 `java.util.Random`，不是 `level.random`。
            //
            // Official `Collections.shuffle` uses `java.util.Random`, not `level.random`.
            SortOrder::Random => shuffle(&mut found),
        }
    }
    let limit = selector.max_results.max(0);
    if (found.len() as i64) > i64::from(limit) {
        found.truncate(limit as usize);
    }
    found
}

/// `Doubles.compare` 的保序位模式；距离都是有限数，NaN 也按官方的位序排。
fn distance_key(region: &Region, target: &Target, origin: Vec3) -> u64 {
    let squared = distance_sqr(region, target, origin);
    if squared.is_nan() {
        u64::MAX
    } else if squared == 0.0 {
        // `Doubles.compare` 让 -0.0 排在 +0.0 之前。
        1u64 << 63
    } else if squared.is_sign_negative() {
        !squared.to_bits()
    } else {
        squared.to_bits() | (1u64 << 63)
    }
}

fn distance_sqr(region: &Region, target: &Target, origin: Vec3) -> f64 {
    match *target {
        Target::Player(id) => region
            .players
            .get(&id)
            .map(|player| player.position().distance_to_sqr(origin))
            .unwrap_or(f64::MAX),
        Target::World(uid) => region
            .entities
            .public_at(uid)
            .map(|public| public.position.distance_to_sqr(origin))
            .unwrap_or(f64::MAX),
    }
}

/// 官方 `EntitySelectorParser.ORDER_RANDOM` 是 `Collections.shuffle(list)`：
/// 它用 `java.util.Collections` 里进程共用的那个不带种子的 `java.util.Random`，
/// 和 `level.random` / 实体自己的随机源都无关，所以它不推进世界的随机源。
/// Bamboo 同样用一份进程级的 `JavaRandom`（等价于 `java.util.Random`），
/// `nextInt(bound)` 的次数、顺序和 `Collections.shuffle` 一致。
///
/// Official `EntitySelectorParser.ORDER_RANDOM` is `Collections.shuffle(list)`,
/// which uses the process-wide unseeded `java.util.Random` held inside
/// `java.util.Collections`, not `level.random` or any entity random. Bamboo
/// keeps one process-wide `JavaRandom` with the same `nextInt(bound)` draws.
fn shuffle(ids: &mut [Target]) {
    static RANDOM: OnceLock<Mutex<JavaRandom>> = OnceLock::new();
    let random = RANDOM.get_or_init(|| Mutex::new(JavaRandom::new(0x6a09_e667_f3bc_c909)));
    let mut random = random
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    random.shuffle(ids);
}
