//! 命令来源（`CommandSourceStack`）：执行位置、朝向和它自己是哪一个实体。
//!
//! 官方 `ServerPlayer.createCommandSourceStack` 用玩家的脚底位置、朝向和玩家自己；
//! `MinecraftServer.createCommandSourceStack` 用出生方块的左下角、`ZERO` 朝向、没有实体。
//! Bamboo 只有主世界，所以维度恒为 `minecraft:overworld`。
//!
//! The command source (`CommandSourceStack`): its position, rotation and which
//! entity it is. A player source uses the player's feet, rotation and the player
//! itself; the server source uses the lower corner of the spawn block, a zero
//! rotation and no entity. Bamboo has only the overworld.

use mc_bamboo_command::{Coordinates, wrap_degrees};
use mc_bamboo_core::Vec3;

use crate::{
    command::{
        console,
        feedback::{CommandFail, fail},
    },
    inbound::PlayerId,
    region::Region,
};

/// 一条命令的执行者。
///
/// The executor of a command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Source {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// `CommandSourceStack.getEntity()`：玩家有，服务器控制台没有。
    pub entity: Option<Entity>,
}

/// 执行者自己这个实体。玩家 ID 和非玩家实体 ID 都从 1 和 `FIRST_ENTITY_ID` 起，
/// 两个空间不重叠，所以按整数就能区分。
///
/// The executor's own entity. Player ids and non-player entity ids occupy
/// disjoint ranges, so the integer alone tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Entity {
    Player(PlayerId),
    World(mc_bamboo_blocks::entity::EntityUid),
}

/// 这个来源的权限等级。控制台固定 4，其余按玩家表。
///
/// The source's permission level. The console is fixed at 4; players use their table entry.
pub(crate) fn level(region: &Region, id: PlayerId) -> u8 {
    if console::is_console(id) {
        console::PERMISSION
    } else {
        region
            .players
            .get(&id)
            .map(|player| player.permission_level)
            .unwrap_or(mc_bamboo_command::permission::ALL)
    }
}

/// 按 ID 造一个来源。控制台的位置取岛上的全局出生点快照。
///
/// Build a source from its id. The console position comes from the island's
/// global spawn snapshot.
pub(crate) fn of(region: &Region, id: PlayerId) -> Source {
    if console::is_console(id) {
        let pos = region.global.spawn.pos;
        Source {
            position: Vec3::new(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)),
            yaw: 0.0,
            pitch: 0.0,
            entity: None,
        }
    } else {
        let (position, yaw, pitch) = region
            .players
            .get(&id)
            .map(|player| (player.position(), player.yaw, player.pitch))
            .unwrap_or((Vec3::default(), 0.0, 0.0));
        Source {
            position,
            yaw,
            pitch,
            entity: Some(Entity::Player(id)),
        }
    }
}

/// 官方 `CommandSourceStack.getEntityOrException`：没有实体时报
/// `permissions.requires.player`。无参 `/kill` 和 `@s` 用它。
///
/// Official `CommandSourceStack.getEntityOrException`: `permissions.requires.player`
/// when the source has no entity. Used by bare `/kill` and `@s`.
pub(crate) fn entity_or_exception(source: Source) -> Result<Entity, CommandFail> {
    source
        .entity
        .ok_or_else(|| fail("permissions.requires.player"))
}

/// 把一串世界/局部坐标按来源的位置和朝向解成世界坐标。
pub(crate) fn position(coords: Coordinates, source: Source) -> Vec3 {
    coords.position(source.position, source.yaw, source.pitch)
}

/// 一串带 `~` 的相对旋转按来源的朝向解成绝对角度。
pub(crate) fn rotation(
    parsed: &mc_bamboo_command::ParsedCommand,
    name: &str,
    source: Source,
) -> Option<(f32, f32)> {
    let rotation = parsed.get(name)?.as_rotation()?;
    let (yaw, pitch) = rotation.resolve(source.yaw, source.pitch);
    Some((wrap_degrees(yaw), pitch.clamp(-90.0, 90.0)))
}

/// 官方 `Entity.getDisplayName()` 的纯文本近似：Bamboo 没有玩家队名和自定义名解析，
/// 玩家用名字，非玩家实体用注册表翻译键。
///
/// A plain-text approximation of official `Entity.getDisplayName()`: players use
/// their name and non-player entities use the registry translation key.
pub(crate) fn entity_name(region: &Region, entity: Entity) -> Option<String> {
    match entity {
        Entity::Player(id) => region.players.get(&id).map(|player| player.name.clone()),
        Entity::World(uid) => {
            let public = region.entities.public_at(uid)?;
            if let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind
                && let Some(name) = &mob.custom_name
                && let mc_bamboo_nbt::Tag::String(text) = name
            {
                return Some(text.clone());
            }
            mc_bamboo_data::registries::entity_type::NAMES
                .get(public.kind.entity_type() as usize)
                .map(|name| name.strip_prefix("minecraft:").unwrap_or(name).to_owned())
        }
    }
}

/// 官方 `CommandSourceStack.getPosition`：控制台用出生点，玩家用脚底。
pub(crate) fn position_of(region: &Region, id: PlayerId) -> Vec3 {
    of(region, id).position
}

/// 取坐标参数；没有这个参数时用 `default`。解析错误按 `command.failed`。
pub(crate) fn coordinates(
    parsed: &mc_bamboo_command::ParsedCommand,
    name: &str,
    source: Source,
    default: Vec3,
) -> Result<Vec3, CommandFail> {
    let Some(value) = parsed.get(name) else {
        return Ok(default);
    };
    let coords = value.as_vec3().ok_or_else(|| fail("command.failed"))?;
    Ok(position(coords, source))
}
