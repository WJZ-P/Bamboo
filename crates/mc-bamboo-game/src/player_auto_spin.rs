//! 玩家自旋攻击：官方 `Player.startAutoSpinAttack` / `LivingEntity.checkAutoSpinAttack`。
//!
//! 激流三叉戟松手时进入自旋攻击，服务端要维护计时、生物标志位元数据里的
//! `SPIN_ATTACK` 位、撞到实体时按 `Player.attack` 造成伤害，并在结束时清标志位。
//! 玩家自身的位移由客户端上报，服务端只做官方服务端那一侧的这些副作用。
//!
//! Player auto-spin attack (riptide trident): vanilla `Player.startAutoSpinAttack`
//! and `LivingEntity.checkAutoSpinAttack`. The server tracks the timer, the
//! `SPIN_ATTACK` bit of the living-flags metadata, the `Player.attack` damage on
//! contact, and clears the flag at the end. Player motion is client-authoritative;
//! the server only performs these server-side effects.

use mc_bamboo_core::{Aabb, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{inbound::PlayerId, player::Player, region::Region};

/// `LivingEntity.DATA_LIVING_ENTITY_FLAGS` 里 `LIVING_ENTITY_FLAG_SPIN_ATTACK` 的位（4）。
///
/// The `LIVING_ENTITY_FLAG_SPIN_ATTACK` bit (4) of `DATA_LIVING_ENTITY_FLAGS`.
pub(crate) const LIVING_FLAG_SPIN_ATTACK: u8 = 0x04;

/// 站立 / 潜行 / 游泳的碰撞盒高度，宽度固定 0.6（和 `player_move` 一致）。
const STANDING_BODY: f64 = 1.8;
const CROUCHING_BODY: f64 = 1.5;
const HALF_WIDTH: f64 = 0.3;

/// `LivingEntity.autoSpinAttackTicks` / `autoSpinAttackDmg` / `autoSpinAttackItemStack`。
/// 另外记下上一刻的位置，`checkAutoSpinAttack` 用前后两盒的并集做扫掠。
///
/// The three vanilla spin-attack fields, plus the previous position so
/// `checkAutoSpinAttack` can sweep the union of the old and new boxes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoSpinAttack {
    pub(crate) ticks: i32,
    pub(crate) damage: f32,
    pub(crate) item: Option<ItemStack>,
    pub(crate) last_position: Vec3,
}

impl Default for AutoSpinAttack {
    fn default() -> Self {
        Self {
            ticks: 0,
            damage: 0.0,
            item: None,
            last_position: Vec3::default(),
        }
    }
}

impl AutoSpinAttack {
    /// `Player.isAutoSpinAttack()`：读的是标志位，服务端这边用计时判定。
    ///
    /// `Player.isAutoSpinAttack()` reads the flag; the server uses the timer.
    pub(crate) fn is_active(&self) -> bool {
        self.ticks > 0
    }
}

/// `Player.startAutoSpinAttack(ticks, damage, stack)`：记状态，服务端置标志位。
///
/// `Player.startAutoSpinAttack(ticks, damage, stack)`: store the state and set
/// the server-side flag (the client-predicted motion is not simulated here).
pub(crate) fn start(region: &mut Region, id: PlayerId, ticks: i32, damage: f32, item: ItemStack) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    player.auto_spin_attack = AutoSpinAttack {
        ticks,
        damage,
        item: Some(item),
        last_position: player.position(),
    };
    // 生物标志位（index 8）里的 SPIN_ATTACK 位变了，下一周期发出去。
    player.sync.metadata_dirty = true;
}

/// 该玩家在 `EntityData::LivingFlags` 里的自旋攻击位。
///
/// The spin-attack bit to fold into the player's `EntityData::LivingFlags`.
pub(crate) fn living_flag(player: &Player) -> u8 {
    if player.auto_spin_attack.is_active() {
        LIVING_FLAG_SPIN_ATTACK
    } else {
        0
    }
}

/// `LivingEntity.tick` 的 `push` 段：每刻递减计时并跑 `checkAutoSpinAttack`。
///
/// The `push` section of `LivingEntity.tick`: decrement the timer each tick and
/// run `checkAutoSpinAttack`.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        if !player.auto_spin_attack.is_active() {
            continue;
        }
        let position = player.position();
        let crouching = player.sync.is_crouching();
        let last = player.auto_spin_attack.last_position;
        let damage = player.auto_spin_attack.damage;
        // 官方先 `autoSpinAttackTicks--`，再用「travel 前的盒」和「当前盒」的并集查询。
        // Vanilla decrements first, then queries the union of the pre-travel box
        // and the current box.
        let area = union(player_box(last, crouching), player_box(position, crouching));
        let target = first_living_target(region, id, area);

        if let Some(uid) = target {
            // `Player.doAutoAttackOnTouch` -> `Player.attack`，自旋攻击用
            // `autoSpinAttackDmg` 当基础伤害（不是属性攻击力）。
            // `Player.doAutoAttackOnTouch` -> `Player.attack` with
            // `autoSpinAttackDmg` as the base damage (not the attribute value).
            crate::player_attack::on_attack_with_base(region, id, uid, damage);
        }
        let Some(player) = region.players.get_mut(&id) else {
            continue;
        };
        player.auto_spin_attack.ticks -= 1;
        if target.is_some() {
            player.auto_spin_attack.ticks = 0;
        }
        if player.auto_spin_attack.ticks <= 0 {
            // `checkAutoSpinAttack` 末尾：清标志位、伤害和物品堆。
            // End of `checkAutoSpinAttack`: clear the flag, damage and stack.
            player.auto_spin_attack.damage = 0.0;
            player.auto_spin_attack.item = None;
            player.sync.metadata_dirty = true;
        }
        player.auto_spin_attack.last_position = position;
    }
}

/// 盒内第一个生物目标（按实体分区顺序，和官方 `getEntities` 一致）；玩家单独补上。
///
/// The first living target in the box (entity section order, like vanilla's
/// `getEntities`); players are appended separately because they live outside
/// the world entity store.
fn first_living_target(region: &Region, id: PlayerId, area: Aabb) -> Option<i32> {
    let mut found = None;
    region
        .entities
        .for_each_in_section_order(area, &mut |public| {
            if found.is_none() && !public.removed && public.kind.mob_type().is_some() {
                found = Some(public.uid);
            }
        });
    if found.is_none() {
        for player in region.players.values() {
            if player.id == id {
                continue;
            }
            let bounds = player_box(player.position(), player.sync.is_crouching());
            if bounds.intersects(area) {
                found = Some(player.id.0);
                break;
            }
        }
    }
    found
}

fn player_box(position: Vec3, crouching: bool) -> Aabb {
    let height = if crouching {
        CROUCHING_BODY
    } else {
        STANDING_BODY
    };
    Aabb::new(
        position.x - HALF_WIDTH,
        position.y,
        position.z - HALF_WIDTH,
        position.x + HALF_WIDTH,
        position.y + height,
        position.z + HALF_WIDTH,
    )
}

fn union(a: Aabb, b: Aabb) -> Aabb {
    Aabb::new(
        a.min_x.min(b.min_x),
        a.min_y.min(b.min_y),
        a.min_z.min(b.min_z),
        a.max_x.max(b.max_x),
        a.max_y.max(b.max_y),
        a.max_z.max(b.max_z),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Player;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    fn region_with_player() -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        (region, id)
    }

    #[test]
    fn start_stores_state_and_sets_the_spin_flag() {
        let (mut region, id) = region_with_player();
        let item = ItemStack::new(mc_bamboo_data::items::TRIDENT, 1);
        start(&mut region, id, 20, 8.0, item.clone());
        let player = &region.players[&id];
        assert!(player.auto_spin_attack.is_active());
        assert_eq!(player.auto_spin_attack.ticks, 20);
        assert_eq!(player.auto_spin_attack.damage, 8.0);
        assert_eq!(player.auto_spin_attack.item, Some(item));
        assert_eq!(living_flag(player), LIVING_FLAG_SPIN_ATTACK);
        assert!(player.sync.metadata_dirty);
    }

    #[test]
    fn the_timer_runs_out_and_clears_the_flag_and_state() {
        let (mut region, id) = region_with_player();
        start(
            &mut region,
            id,
            3,
            8.0,
            ItemStack::new(mc_bamboo_data::items::TRIDENT, 1),
        );
        for _ in 0..3 {
            tick(&mut region);
        }
        let player = &region.players[&id];
        assert_eq!(player.auto_spin_attack.ticks, 0);
        assert!(!player.auto_spin_attack.is_active());
        assert_eq!(living_flag(player), 0);
        assert_eq!(player.auto_spin_attack.damage, 0.0);
        assert!(player.auto_spin_attack.item.is_none());
    }

    #[test]
    fn the_sweep_box_unions_the_old_and_new_positions() {
        let a = player_box(Vec3::new(0.0, 64.0, 0.0), false);
        let b = player_box(Vec3::new(1.0, 64.0, 0.0), false);
        let merged = union(a, b);
        assert_eq!(merged.min_x, a.min_x);
        assert_eq!(merged.max_x, b.max_x);
    }
}
