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
                        source.attacker = Some(attacker.0);
                        source.direct = Some(attacker.0);
                        source.attacker_uuid = Some(*player.uuid.as_bytes());
                        source.position = Some(player.position());
                    }
                }
                Target::World(uid) => {
                    source.attacker = Some(uid);
                    source.direct = Some(uid);
                    source.scale_when_non_player = true;
                    if let Some(public) = region.entities.public_at(uid) {
                        source.position = Some(public.position);
                    }
                }
            }
        }
    }
    if let Some(cause) = parsed.get("cause").and_then(Value::as_entity) {
        let targets = select::targets(region, id, level, cause)?;
        if let Some(&target) = targets.first() {
            source.attacker = Some(match target {
                Target::Player(player) => player.0,
                Target::World(uid) => uid,
            });
            if matches!(target, Target::World(_)) {
                source.scale_when_non_player = true;
            }
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
