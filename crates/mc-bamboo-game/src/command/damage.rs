//! `/damage`：官方 `DamageCommand`。给玩家或非玩家实体造成一次指定类型的伤害。
//!
//! 语法 `/damage <target> <amount> [damageType] [at <location> | by <entity> [from <cause>]]`。
//! 玩家走期 1a 的 `hurt_player`，非玩家走 `World::hurt_mob`。
//! `/damage` 对血量归零的玩家不会触发死亡流程（1b）。
//!
//! `/damage`, vanilla `DamageCommand`. Players go through the 1a hurt path,
//! non-player entities through `World::hurt_mob`.

use mc_bamboo_blocks::{
    World,
    entity::mob::{DamageKind, DamageSource},
};
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select::{self, Target},
        source,
    },
    global_port::GlobalPort,
    inbound::PlayerId,
    interaction,
    region::Region,
};

pub(crate) fn damage(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let level = source::level(region, id);
    let command_source = source::of(region, id);
    let selector = parsed
        .get("target")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let amount = parsed
        .get("amount")
        .and_then(Value::as_float)
        .ok_or_else(|| fail("command.failed"))?;
    let kind = match parsed.get("damageType").and_then(Value::as_str) {
        Some(name) => DamageKind::by_name(name).unwrap_or(DamageKind::Generic),
        None => DamageKind::Generic,
    };
    let mut source = DamageSource::environment(kind);
    if let Some(at) = parsed.get("location").and_then(Value::as_vec3) {
        source = source.with_position(source::position(at, command_source));
    }
    if let Some(by) = parsed.get("entity").and_then(Value::as_entity) {
        let targets = select::targets(region, id, level, by)?;
        if let Some(&target) = targets.first() {
            match target {
                Target::Player(attacker) => {
                    if let Some(player) = region.players.get(&attacker) {
                        source.direct = Some(attacker.0);
                        source.position = Some(player.position());
                    }
                }
                Target::World(uid) => {
                    source.direct = Some(uid);
                    if let Some(public) = region.entities.public_at(uid) {
                        source.position = Some(public.position);
                    }
                }
            }
            set_causing_entity(&mut source, region, target);
        }
    }
    if let Some(cause) = parsed.get("cause").and_then(Value::as_entity) {
        let targets = select::targets(region, id, level, cause)?;
        if let Some(&target) = targets.first() {
            set_causing_entity(&mut source, region, target);
        }
    }
    let targets = select::targets(region, id, level, selector)?;
    let mut success = 0;
    let mut last_name = None;
    for target in targets {
        last_name = source::entity_name(region, target)
            .map(|text| Component::text(text).into())
            .or(last_name);
        match target {
            Target::Player(player) => {
                let mut random = Some(
                    region
                        .players
                        .get(&player)
                        .map(|value| value.random)
                        .unwrap_or_default(),
                );
                let hurt =
                    crate::player_hurt::hurt_player(region, player, source, amount, &mut random);
                if let (Some(state), Some(player)) = (random, region.players.get_mut(&player)) {
                    player.random = state;
                }
                if hurt {
                    success += 1;
                }
            }
            Target::World(uid) => {
                let ok = {
                    let (mut level, ticks) = region.block_level();
                    let mut world = World::new(&mut level, ticks);
                    let ok = world.hurt_mob(uid, source, amount);
                    let effects = world.into_effects();
                    interaction::send_effects(region, Some(id), &effects);
                    ok
                };
                if ok {
                    success += 1;
                }
            }
        }
    }
    if success == 0 {
        return Err(fail("commands.damage.invulnerable"));
    }
    // 官方 `damage()` 每次只打一个实体，反馈带那一个实体的显示名。
    // Vanilla's `damage()` hurts one entity and names it in the feedback.
    let message = Component::translate(
        "commands.damage.success",
        [
            TranslateArg::from(amount),
            last_name.unwrap_or_else(|| TranslateArg::from("")),
        ],
    );
    feedback::success(region, id, message, true);
    Ok(())
}

fn set_causing_entity(source: &mut DamageSource, region: &Region, target: Target) {
    // `by` 是直接实体，`from` 只替换致因实体及其属性。
    // `by` is the direct entity; `from` replaces only the causing entity and its properties.
    source.attacker_is_player = false;
    source.creative_player = false;
    source.attacker_uuid = None;
    source.scale_when_non_player = false;
    source.attacker = Some(match target {
        Target::Player(id) => {
            if let Some(player) = region.players.get(&id) {
                source.attacker_is_player = true;
                source.creative_player =
                    player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative;
                source.attacker_uuid = Some(*player.uuid.as_bytes());
            }
            id.0
        }
        Target::World(uid) => {
            use mc_bamboo_blocks::entity::KindPublic;
            source.scale_when_non_player = region
                .entities
                .public_at(uid)
                .is_some_and(|entity| matches!(entity.kind, KindPublic::Mob(_)));
            uid
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Player;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    #[test]
    fn from_replaces_credit_but_keeps_the_direct_damage_entity() {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1025);
        let uuid = uuid::Uuid::from_u128(52);
        region.insert_player(Player::new(
            id,
            uuid,
            "Killer".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(2.5, 64.0, 0.5),
        ));
        let mut source = DamageSource::environment(DamageKind::Arrow);
        source.direct = Some(4);
        set_causing_entity(&mut source, &region, Target::Player(id));
        assert_eq!(source.direct, Some(4));
        assert_eq!(source.attacker, Some(id.0));
        assert_eq!(source.attacker_uuid, Some(*uuid.as_bytes()));
        assert!(source.attacker_is_player);
        set_causing_entity(&mut source, &region, Target::World(4));
        assert_eq!(source.attacker, Some(4));
        assert_eq!(source.attacker_uuid, None);
        assert!(!source.attacker_is_player);
        assert!(!source.scale_when_non_player);
    }
}
