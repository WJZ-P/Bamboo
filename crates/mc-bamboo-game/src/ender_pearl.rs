//! 末影珍珠与扔出它的玩家之间的绑定：使用冷却、飞行中珍珠的注册 / 注销、
//! 区块票、下线 / 上线的存档往返，以及命中落地时的传送。
//!
//! 官方 `ServerPlayer.enderPearls` 是一组 `ThrownEnderpearl` 引用，`setOwner` /
//! `onRemoval` 注册与注销；`saveEnderPearls` 把每枚珍珠整份实体 NBT 写进
//! `ender_pearls` 列表，`loadAndSpawnEnderPearls` 读回来并给落点挂票。
//! `ThrownEnderpearl.tick` 每刻按 `ticketTimer` 续 `TicketType.ENDER_PEARL` 票。
//!
//! Binding between an ender pearl and its player: use cooldown, registration of
//! in-flight pearls, chunk tickets, save/load across sessions, and the landing
//! teleport. Vanilla keeps a `Set<ThrownEnderpearl>` on the player, saves each
//! pearl's full entity NBT in `ender_pearls`, and refreshes the
//! `TicketType.ENDER_PEARL` ticket every tick.

use std::collections::BTreeMap;

use mc_bamboo_blocks::{
    archive,
    entity::{KindPublic, KindState},
};
use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};
use mc_bamboo_storage::player_data::{OVERWORLD, PlayerData};
use mc_bamboo_world::{director::TicketOp, ticket::TicketType};

use crate::{inbound::PlayerId, region::Region};

/// `TicketType.ENDER_PEARL.timeout()`；`registerAndUpdateEnderPearlTicket`
/// 返回超时减一，所以每次续票之间隔 39 刻。
///
/// `TicketType.ENDER_PEARL.timeout()`; `registerAndUpdateEnderPearlTicket`
/// returns timeout - 1, so a refresh happens every 39 ticks.
const TICKET_TIMEOUT: i32 = 40;
/// `ServerChunkCache.addTicketWithRadius(TicketType.ENDER_PEARL, pos, 2)`。
const TICKET_RADIUS: i32 = 2;
/// `saveEnderPearls` 里每枚珍珠带的维度键。
const DIMENSION_KEY: &str = "ender_pearl_dimension";

/// 物品堆的 `use_cooldown` 组件：返回（刻数，冷却组）。`ticks()` 是
/// `seconds * 20.0F` 再截成 int，官方的 `UseCooldown.apply` 用它。
///
/// The stack's `use_cooldown` component: (ticks, cooldown group). `ticks()` is
/// `seconds * 20.0F` narrowed to an int.
pub(crate) fn use_cooldown(stack: &ItemStack) -> (i32, Option<String>) {
    let patch = stack.patch();
    let Some(ComponentValue::UseCooldown {
        seconds,
        cooldown_group,
    }) = defaults::resolve(
        stack.item,
        &patch,
        mc_bamboo_data::registries::data_component_type::USE_COOLDOWN,
    )
    else {
        return (0, None);
    };
    ((*seconds * 20.0) as i32, cooldown_group.clone())
}

/// `ServerPlayerGameMode.useItem` 开头的 `getCooldowns().isOnCooldown(stack)`。
pub(crate) fn on_cooldown(player: &crate::player::Player, stack: &ItemStack) -> bool {
    let (_, group) = use_cooldown(stack);
    player
        .cooldowns
        .is_on_cooldown(stack.item, group.as_deref())
}

/// `ItemStack.use` 成功后 `UseCooldown.apply`：给玩家挂上冷却。
pub(crate) fn apply_cooldown(player: &mut crate::player::Player, stack: &ItemStack) {
    let (ticks, group) = use_cooldown(stack);
    if ticks > 0 {
        player.cooldowns.add(stack.item, group.as_deref(), ticks);
    }
}

/// 玩家扔出一枚珍珠，注册到他的 `enderPearls` 集合里。
pub(crate) fn register(region: &mut Region, id: PlayerId, pearl: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        player.ender_pearls.insert(pearl);
    }
}

/// `ThrownEnderpearl.onRemoval`：注销一枚珍珠。
pub(crate) fn unregister(region: &mut Region, owner: [u8; 16], pearl: i32) {
    for player in region.players.values_mut() {
        if *player.uuid.as_bytes() == owner {
            player.ender_pearls.remove(&pearl);
        }
    }
}

/// `Effect::EnderPearlTeleport`：走正式的传送入口，清下落距离。
/// 官方 `ServerPlayer.teleport` 成功后调 `resetFallDistance()` 和
/// `resetCurrentImpulseContext()`，再受 5 点 `ender_pearl` 伤害。
///
/// `Effect::EnderPearlTeleport`: use the normal teleport entry and clear fall
/// distance.
pub(crate) fn teleport(region: &mut Region, target: i32, position: Vec3) {
    let id = PlayerId(target);
    let now = region.tick;
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    // 借用拆开：`movement::teleport` 需要玩家、链路和网格三份可变借用。
    // Split the borrows: `movement::teleport` needs all three.
    let links = &mut region.links;
    let grid = &mut region.player_grid;
    crate::movement::teleport(player, links, grid, now, position);
    player.motion.fall_distance = 0.0;
    player.on_ground = false;
}

/// 按官方 `ThrownEnderpearl.tick` 的节奏维护珍珠票。
///
/// 主控每刻调用：对每枚还在飞的珍珠，跨区块或计时到点就重挂半径 2 的
/// `ENDER_PEARL` 票（票表自己有 40 刻超时，重挂会重置）；消失的珍珠撤票。
///
/// Refreshes the pearl tickets like `ThrownEnderpearl.tick`: whenever a live
/// pearl changes chunk or its 39-tick timer runs out, re-add the radius-2
/// `ENDER_PEARL` ticket; remove the ticket of pearls that are gone.
pub(crate) fn sync_tickets(
    live: &BTreeMap<i32, ChunkPos>,
    tracked: &mut BTreeMap<i32, (ChunkPos, i32)>,
) -> Vec<TicketOp> {
    let mut ops = Vec::new();
    let gone: Vec<i32> = tracked
        .keys()
        .filter(|uid| !live.contains_key(uid))
        .copied()
        .collect();
    for uid in gone {
        let Some((pos, _)) = tracked.remove(&uid) else {
            continue;
        };
        ops.push(TicketOp::WithRadius {
            ty: TicketType::EnderPearl,
            pos,
            radius: TICKET_RADIUS,
            add: false,
        });
    }
    for (&uid, &pos) in live {
        match tracked.get_mut(&uid) {
            Some((old, timer)) => {
                *timer -= 1;
                if *old != pos {
                    ops.push(TicketOp::WithRadius {
                        ty: TicketType::EnderPearl,
                        pos: *old,
                        radius: TICKET_RADIUS,
                        add: false,
                    });
                    *old = pos;
                    *timer = TICKET_TIMEOUT - 1;
                } else if *timer > 0 {
                    continue;
                } else {
                    *timer = TICKET_TIMEOUT - 1;
                }
            }
            None => {
                tracked.insert(uid, (pos, TICKET_TIMEOUT - 1));
            }
        }
        ops.push(TicketOp::WithRadius {
            ty: TicketType::EnderPearl,
            pos,
            radius: TICKET_RADIUS,
            add: true,
        });
    }
    ops
}

/// 从一个岛的实体表里读出所有还在飞的、由玩家扔出的末影珍珠所在区块。
/// 官方只在主人是 `ServerPlayer` 时挂票，所以没有玩家主人的珍珠不算。
pub(crate) fn live_pearls(region: &Region) -> BTreeMap<i32, ChunkPos> {
    let mut out = BTreeMap::new();
    for public in region.entities.iter_public() {
        if public.removed {
            continue;
        }
        let KindPublic::Egg(egg) = &public.kind else {
            continue;
        };
        if egg.kind != mc_bamboo_blocks::entity::thrown_egg::ThrowableKind::EnderPearl
            || !egg
                .owner
                .is_some_and(|uid| region.players.values().any(|player| player.id.0 == uid))
        {
            continue;
        }
        out.insert(
            public.uid,
            ChunkPos::containing(public.position.x, public.position.z),
        );
    }
    out
}

/// `ServerPlayer.saveEnderPearls`：每枚未移除的珍珠写整份实体 NBT 加
/// `ender_pearl_dimension`。空集合返回空表，编码时整个键都不写。
///
/// `ServerPlayer.saveEnderPearls`: one full entity tag plus
/// `ender_pearl_dimension` per live pearl. An empty set omits the key.
fn pearl_tags(region: &Region, id: PlayerId) -> Vec<Tag> {
    let Some(player) = region.players.get(&id) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for &uid in &player.ender_pearls {
        let Some(entity) = region.entities.get(uid) else {
            // 官方跳过已经 `isRemoved()` 的珍珠；表里查不到同义。
            continue;
        };
        if entity.removed {
            continue;
        }
        let Ok(mut tag) = archive::write_entity(&entity.split()) else {
            continue;
        };
        if let Tag::Compound(entries) = &mut tag {
            entries.push((DIMENSION_KEY.to_owned(), Tag::String(OVERWORLD.to_owned())));
        }
        tags.push(tag);
    }
    tags
}

/// `loadAndSpawnEnderPearls`：把存档里的珍珠放回实体表，注册到玩家的集合里。
/// 维度不是主世界时官方跳过，Bamboo 只有主世界。
///
/// 官方 `loadEntityRecursive` 保留存档里的 `Owner`，不会改成读档的玩家；只有主人
/// 就是这名玩家时才能解析成实体号，也才注册（其余情况等珍珠自己 tick 时再找主人）。
pub(crate) fn load(region: &mut Region, id: PlayerId, data: &PlayerData) {
    let Some(player_uuid) = region.players.get(&id).map(|p| *p.uuid.as_bytes()) else {
        return;
    };
    for tag in &data.ender_pearls {
        let Tag::Compound(entries) = tag else {
            continue;
        };
        let dimension = entries.iter().find(|(key, _)| key == DIMENSION_KEY);
        let Some((_, Tag::String(dimension))) = dimension else {
            continue;
        };
        if dimension != OVERWORLD {
            continue;
        }
        let mut saved = tag.clone();
        if let Tag::Compound(entries) = &mut saved {
            entries.retain(|(key, _)| key != DIMENSION_KEY);
        }
        let Ok(Some(mut row)) = archive::read_entity(&saved, 0) else {
            continue;
        };
        let KindState::Egg(state) = &mut row.state.kind else {
            continue;
        };
        let owned = state.owner_uuid == Some(player_uuid);
        if owned {
            state.owner = Some(id.0);
            if let KindPublic::Egg(public) = &mut row.public.kind {
                public.owner = Some(id.0);
            }
        }
        let uid = region.entities.spawn_row(row);
        if owned && let Some(player) = region.players.get_mut(&id) {
            player.ender_pearls.insert(uid);
        }
    }
}

/// `ThrownEnderpearl.tick` 开头：主人死了且 `ender_pearls_vanish_on_death` 为真
/// 就丢弃珍珠。官方条件是 `!isAlive() && !wonGame`；Bamboo 没有终末之诗，
/// 所以只看 `dead`（`ServerPlayer.dead`，也就是死亡流程已开始）。
pub(crate) fn remove_dead_owners_pearls(region: &mut Region, vanish: bool) {
    let doomed: Vec<(i32, [u8; 16])> = region
        .players
        .iter()
        .filter(|(_, player)| vanish && player.dead && !player.ender_pearls.is_empty())
        .flat_map(|(_, player)| {
            let uuid = *player.uuid.as_bytes();
            player
                .ender_pearls
                .iter()
                .copied()
                .map(move |uid| (uid, uuid))
        })
        .collect();
    for (uid, uuid) in doomed {
        region
            .entities
            .with_mut(uid, |entity| entity.removed = true);
        unregister(region, uuid, uid);
    }
}

/// `PlayerList.remove`：先把珍珠写进玩家存档，再按 `UNLOADED_WITH_PLAYER` 把
/// 世界里的珍珠标记移除。调用方在 `save_one_player` 之前刷新快照，之后调用这里。
///
/// `PlayerList.remove`: save the in-flight pearls into the player file, then mark
/// them removed with `UNLOADED_WITH_PLAYER`. The caller refreshes the snapshot
/// before saving and calls this afterwards.
pub(crate) fn remove_on_leave(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    for uid in std::mem::take(&mut player.ender_pearls) {
        region
            .entities
            .with_mut(uid, |entity| entity.removed = true);
    }
}

/// 保存前刷新这名玩家的 `ender_pearls` 快照。
///
/// Refresh one player's `ender_pearls` snapshot before saving.
pub(crate) fn refresh_snapshot(region: &mut Region, id: PlayerId) {
    let tags = pearl_tags(region, id);
    if let Some(player) = region.players.get_mut(&id) {
        player.saved_ender_pearls = tags;
    }
}

/// 玩家 tick：推进物品冷却，清掉已经不在实体表里的珍珠号，并把当前飞行中的
/// 珍珠 NBT 快照写进玩家（存档编码是 `&Player` 的纯函数，需要这份快照）。
///
/// Player tick: advance item cooldowns, forget pearls that left the entity
/// table, and snapshot the in-flight pearl NBT on the player (save encoding is
/// a pure function of `&Player`).
pub(crate) fn tick(region: &mut Region) {
    for player in region.players.values_mut() {
        player.cooldowns.tick();
    }
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in &ids {
        // `onRemoval`：离开世界的珍珠从 `enderPearls` 里注销。
        if let Some(player) = region.players.get_mut(id) {
            player.ender_pearls.retain(|uid| {
                region
                    .entities
                    .public_at(*uid)
                    .is_some_and(|public| !public.removed)
            });
        }
        refresh_snapshot(region, *id);
    }
}

#[cfg(test)]
mod tests;
