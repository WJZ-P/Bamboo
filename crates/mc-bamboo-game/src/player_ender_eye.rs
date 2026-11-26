//! 玩家右键投掷末影之眼：官方 `EnderEyeItem.use`。
//!
//! 对着末地传送门框架时返回 `PASS`（不管有没有装眼，交给 `useOn`）；否则调
//! `findNearestMapStructure(#eye_of_ender_located, blockPosition, 100, false)`：
//! 找不到要塞返回 `CONSUME`（不生成、不扣物品、不播音、不记统计、不触发进度）；
//! 找到后在 `(x, getY(0.5), z)` 生成 `EyeOfEnder`、`setItem`、`signalTo`、发
//! `PROJECTILE_SHOOT` 游戏事件、触发 `minecraft:used_ender_eye` 进度、播放
//! `entity.ender_eye.launch`（音高 `Mth.lerp(level.random.nextFloat(), 0.33F, 0.5F)`，
//! 26.3 字节码无 1003 世界事件）、非创造扣一个、记 `ITEM_USED` 统计，返回
//! `SUCCESS_SERVER`。
//!
//! Player throwing an eye of ender, vanilla `EnderEyeItem.use`. Returns `PASS`
//! when aiming at an end portal frame; otherwise locates the nearest stronghold,
//! returning `CONSUME` if none exists, or spawning `EyeOfEnder`, firing
//! `PROJECTILE_SHOOT`, triggering `used_ender_eye`, playing `ENDER_EYE_LAUNCH`,
//! consuming one stack item (non-creative), awarding `ITEM_USED`, and returning
//! `SUCCESS_SERVER`.

use mc_bamboo_blocks::{
    Actor, InteractionResult, World,
    game_event::{GameEvent, dispatch::EventContext},
};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::{blocks, items, sounds};
use mc_bamboo_protocol::packet::play::GameMode;

use crate::{
    inbound::PlayerId,
    player_weapon::{level_random_float, sync_slots},
    region::Region,
};

/// `EnderEyeItem.use`：手持末影之眼时处理并返回 `Some(InteractionResult)`；否则返回 `None`。
///
/// Handle `EnderEyeItem.use` when holding an eye of ender, returning `Some(result)`;
/// returns `None` for any other item.
pub(crate) fn use_eye(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    actor: &Actor,
) -> Option<InteractionResult> {
    let player = region.players.get(&id)?;
    let stack = player.inventory.held(hand).clone();
    if stack.item != items::ENDER_EYE {
        return None;
    }
    if crate::ender_pearl::on_cooldown(player, &stack) {
        return Some(InteractionResult::Pass);
    }
    let pos = player.position();
    let height = player.height();
    let sneaking = actor.secondary_use;
    // 1. `getPlayerPOVHitResult(level, player, ClipContext.Fluid.NONE)`：瞄着框架就 `PASS`。
    let aimed_at_frame = {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        let (from, to) = actor.view_segment();
        world
            .clip_outline(from, to, actor.main_hand)
            .is_some_and(|hit| world.block_state(hit.pos).is(blocks::END_PORTAL_FRAME))
    };
    if aimed_at_frame {
        return Some(InteractionResult::Pass);
    }
    // 2. 定位最近的 `#eye_of_ender_located` 结构（26.3 中为 `minecraft:stronghold`）。
    let origin = BlockPos::containing(pos);
    let Some(target) = crate::worldgen_structures::find_nearest_eye_of_ender_structure(
        &region.global.world_gen,
        origin,
    ) else {
        return Some(InteractionResult::Consume);
    };
    // 3. 在 `(x, getY(0.5), z)` 生成 `EyeOfEnder`、`setItem`、`signalTo` 并投递 `PROJECTILE_SHOOT`。
    let eye_pos = Vec3::new(pos.x, pos.y + f64::from(height) * 0.5, pos.z);
    let target_vec = Vec3::new(
        f64::from(target.x),
        f64::from(target.y),
        f64::from(target.z),
    );
    let spawn_effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let _ = world.spawn_eye_of_ender(eye_pos, stack.clone(), target_vec);
        world.game_event(
            GameEvent::ProjectileShoot,
            eye_pos,
            EventContext {
                sneaking,
                source_player: Some(id.0),
                source_entity: Some(id.0),
                ..EventContext::default()
            },
        );
        world.drain_effects()
    };
    crate::interaction::send_effects(region, Some(id), &spawn_effects);
    // 4. `CriteriaTriggers.USED_ENDER_EYE.trigger(serverPlayer, target)`。
    let dx = pos.x - f64::from(target.x);
    let dz = pos.z - f64::from(target.z);
    crate::advancement_triggers::record(
        region,
        id,
        "minecraft:used_ender_eye",
        serde_json::json!({ "distance_sqr": dx * dx + dz * dz }),
    );
    // 5. `Mth.lerp(level.getRandom().nextFloat(), 0.33F, 0.5F)` + `ENDER_EYE_LAUNCH`。
    let pitch = 0.33_f32 + level_random_float(region) * (0.5_f32 - 0.33_f32);
    let sound_effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.play_sound(
            pos,
            sounds::sound_event("minecraft:entity.ender_eye.launch"),
            sounds::NEUTRAL,
            1.0,
            pitch,
            false,
        );
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &sound_effects);
    // 6. `itemStack.consume(1, player)` + `player.awardStat(Stats.ITEM_USED.get(this))`。
    consume_one(region, id, hand);
    crate::item_use_stats::award(region, id, items::ENDER_EYE);
    Some(InteractionResult::SuccessServer)
}

/// `ItemStack.consume(1, player)`：创造模式不扣。
fn consume_one(region: &mut Region, id: PlayerId, hand: Hand) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.game_mode == GameMode::Creative {
        return;
    }
    let slot = player.inventory.hand_slot(hand);
    let stack = &mut player.inventory.slots[slot];
    stack.count -= 1;
    if stack.count <= 0 {
        *stack = mc_bamboo_protocol::item::ItemStack::empty();
    }
    sync_slots(region, id, [slot]);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::{
        BlockState,
        entity::KindPublic,
        state::{EYE, HORIZONTAL_FACING},
    };
    use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, Vec3};
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::{
        item::ItemStack,
        packet::play::{GameMode, PlayRequest},
    };

    use crate::{
        world::tests::Harness,
        world_options::{LevelType, WorldGenConfig},
    };

    /// 无结构的超平坦世界右键扔末影之眼：找不到要塞，返回 `CONSUME`，不生成实体、不扣物品。
    #[test]
    fn flat_world_without_structures_does_not_throw_eye() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("Alice");
        server.ensure_loaded(&[ChunkPos::new(0, 0)]);
        {
            let player = server
                .region()
                .players
                .get_mut(&client.id)
                .expect("player exists");
            player.game_mode = GameMode::Survival;
            player.inventory.slots[36] = ItemStack::new(items::ENDER_EYE, 4);
        }
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();
        let eyes = server
            .region()
            .entities
            .iter_public()
            .filter(|e| matches!(e.kind, KindPublic::EyeOfEnder(_)))
            .count();
        assert_eq!(eyes, 0);
        let remaining = server
            .region()
            .players
            .get(&client.id)
            .expect("player exists")
            .inventory
            .slots[36]
            .count;
        assert_eq!(remaining, 4);
    }

    /// 正常主世界（开启结构）朝空中扔末影之眼：生成 `EyeOfEnder`、生存扣 1 个、记使用统计；
    /// 瞄着末地传送门框架右键空气时走 `PASS`，不扔出末影之眼。
    #[test]
    fn normal_world_throws_eye_and_passes_when_aiming_at_frame() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("Bob");
        server.ensure_loaded(&[ChunkPos::new(0, 0)]);
        let world_gen = WorldGenConfig {
            seed: 42,
            level_type: LevelType::Normal,
            generate_structures: true,
            ..WorldGenConfig::default()
        };
        assert!(
            crate::worldgen_structures::find_nearest_eye_of_ender_structure(
                &world_gen,
                BlockPos::new(0, 64, 0),
            )
            .is_some(),
            "normal world with structures must locate a stronghold"
        );
        server.world.master.global.world_gen = world_gen.clone();
        server.region().global.world_gen = world_gen;
        {
            let region = server.region();
            let player = region.players.get_mut(&client.id).expect("player exists");
            player.game_mode = GameMode::Survival;
            player.set_position(&mut region.player_grid, Vec3::new(0.5, 64.0, 0.5));
            player.inventory.slots[36] = ItemStack::new(items::ENDER_EYE, 3);
        }
        // 放一个末地传送门框架在玩家脚前，低头看它再发 `UseItem` -> 必须 PASS。
        let frame_pos = BlockPos::new(0, 64, 2);
        let frame_state = BlockState(blocks::END_PORTAL_FRAME)
            .with(HORIZONTAL_FACING, Direction::North)
            .with(EYE, false);
        server.region().chunks.set_block(frame_pos, frame_state.0);

        // 看向 +Z 下方（对着 (0, 64, 2) 框架）。
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 35.0,
            },
        );
        server.tick();
        assert_eq!(
            server
                .region()
                .entities
                .iter_public()
                .filter(|e| matches!(e.kind, KindPublic::EyeOfEnder(_)))
                .count(),
            0,
            "aiming at end portal frame must PASS"
        );
        assert_eq!(
            server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .inventory
                .slots[36]
                .count,
            3
        );

        // 抬头看天空再发 `UseItem` -> 必须抛出 `EyeOfEnder` 并扣 1 个。
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 2,
                yaw: 0.0,
                pitch: -60.0,
            },
        );
        server.tick();
        let eyes: Vec<_> = server
            .region()
            .entities
            .iter_public()
            .filter(|e| matches!(e.kind, KindPublic::EyeOfEnder(_)))
            .collect();
        assert_eq!(eyes.len(), 1, "one EyeOfEnder spawned");
        assert!(
            (eyes[0].position.y - 64.9).abs() < 1e-6,
            "spawned at player.getY(0.5) = 64 + 1.8*0.5, got {}",
            eyes[0].position.y
        );
        assert_eq!(
            server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .inventory
                .slots[36]
                .count,
            2,
            "survival consumes 1 eye"
        );
    }
}
