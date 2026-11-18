//! `/kill`：官方 `KillCommand`。无参时杀 `@s`，带选择器时杀选中的全部实体。
//!
//! `/kill`, vanilla `KillCommand`. Without arguments it kills `@s`; with a
//! selector it kills every selected entity.

use mc_bamboo_blocks::{
    World,
    entity::{
        KindPublic,
        mob::{DamageKind, DamageSource},
    },
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

/// 官方 `CommandResponseTracker`：所有实体都杀完才发一条成功反馈。
///
/// Official `CommandResponseTracker`: one success message after every entity is killed.
pub(crate) fn kill(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let level = source::level(region, id);
    let command_source = source::of(region, id);
    let targets = match parsed.get("targets").and_then(Value::as_entity) {
        Some(selector) => select::targets(region, id, level, selector)?,
        None => vec![source::entity_or_exception(command_source)?],
    };
    for target in &targets {
        if matches!(target, Target::Player(_)) {
            // 官方 `ServerPlayer.kill` 走完整的玩家死亡和重生流程；Bamboo 还没有，
            // 明确报错而不是悄悄当作成功。
            //
            // Official `ServerPlayer.kill` runs the full player death/respawn path;
            // Bamboo does not have it yet, so fail instead of silently pretending.
            return Err(fail("command.failed"));
        }
    }
    // 名称和种类要在借用方块世界之前读出来。
    // Names and kinds are read before borrowing the block world.
    let single = targets.len() == 1;
    let only = targets
        .first()
        .and_then(|target| display(region, *target, source::entity_name(region, *target)));
    let actions: Vec<Option<bool>> = targets
        .iter()
        .map(|target| match *target {
            Target::Player(_) => None,
            Target::World(uid) => region
                .entities
                .public_at(uid)
                .map(|public| matches!(public.kind, KindPublic::Mob(_))),
        })
        .collect();
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        for (target, action) in targets.iter().zip(&actions) {
            kill_one(&mut world, *target, *action);
        }
        world.into_effects()
    };
    interaction::send_effects(region, Some(id), &effects);
    // 生物在死亡动画结束前不能从实体表里消失：`tick_death` 每刻推进 `death_time`，
    // 20 刻后才 `discard()`，届时实体同步层发移除包和冒烟事件。
    // A dying mob stays in the entity table until `tick_death` reaches 20 ticks;
    // the entity sync layer then sends the removal and poof events.
    let message = match (single, only) {
        (true, Some(name)) => Component::translate("commands.kill.success.single", [name]),
        _ => Component::translate(
            "commands.kill.success.multiple",
            [TranslateArg::from(targets.len() as i32)],
        ),
    };
    feedback::success(region, id, message, true);
    Ok(())
}

fn display(region: &Region, target: Target, name: Option<String>) -> Option<TranslateArg> {
    let fallback = || name.map(|text| Component::text(text).into());
    match target {
        Target::Player(id) => region
            .players
            .get(&id)
            .map(|player| Component::text(player.name.clone()).into())
            .or_else(fallback),
        Target::World(uid) => {
            let Some(public) = region.entities.public_at(uid) else {
                return fallback();
            };
            if let KindPublic::Mob(mob) = &public.kind
                && let Some(tag) = &mob.custom_name
            {
                return Some(Component::trusted(tag.clone()).into());
            }
            let key = mc_bamboo_data::registries::entity_type::NAMES
                .get(public.kind.entity_type() as usize)
                .map(|name| name.replace("minecraft:", "entity.minecraft."))
                .unwrap_or_else(|| "entity.minecraft.cow".to_owned());
            Some(Component::translate(key, []).into())
        }
    }
}

fn kill_one(world: &mut World<'_>, target: Target, action: Option<bool>) {
    let Target::World(uid) = target else {
        return;
    };
    match action {
        // `LivingEntity.kill`: genericKill 伤害 `Float.MAX_VALUE`，正常死亡流程。
        // `LivingEntity.kill`: `Float.MAX_VALUE` genericKill damage, the normal death path.
        Some(true) => {
            world.hurt_mob(
                uid,
                DamageSource::environment(DamageKind::GenericKill),
                f32::MAX,
            );
        }
        // `Entity.kill` 对非生物只是 `remove(KILLED)`。
        // `Entity.kill` on a non-living entity is just `remove(KILLED)`.
        Some(false) => world.remove_entity(uid),
        // 选择器刚解析完实体就被移除：官方 `Entity.kill` 对已移除的实体没有副作用。
        // The entity disappeared after selection: official `Entity.kill` does nothing.
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::item::ItemStack;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use super::*;
    use crate::{
        command::{on_command, tests::world_with},
        global::Global,
        player::Player,
        region::Region,
    };

    fn world_with_entities() -> (Global, Region, PlayerId, Vec<i32>) {
        let (global, mut region, id) = world_with("Kitten", 2);
        let cow = region
            .entities
            .spawn(Entity::cow(Vec3::new(9.5, -59.0, 9.5)));
        let item = region.entities.spawn(Entity::ground_item(
            Vec3::new(10.5, -59.0, 9.5),
            ItemStack::new(mc_bamboo_data::items::STONE, 1),
        ));
        (global, region, id, vec![cow, item])
    }

    #[test]
    fn all_entities_are_candidates_and_type_filters_them() {
        let (_, region, id, ids) = world_with_entities();
        let selected = select::targets(&region, id, 2, &parse("@e[type=minecraft:cow]"))
            .unwrap_or_else(|_| panic!("一头牛"));
        assert_eq!(selected, vec![Target::World(ids[0])]);
        let all =
            select::targets(&region, id, 2, &parse("@e")).unwrap_or_else(|_| panic!("全部实体"));
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], Target::Player(id));
        assert!(all.contains(&Target::World(ids[1])));
    }

    #[test]
    fn killing_a_cow_starts_the_normal_death_animation() {
        let (mut global, mut region, id, ids) = world_with_entities();
        on_command(&mut global, &mut region, id, "kill @e[type=cow]", None);
        let cow = region.entities.get(ids[0]).expect("牛还活着但正在死");
        let mob = cow.kind.mob().expect("牛");
        assert!(mob.dead, "走了 `LivingEntity.die`");
        assert_eq!(mob.health, 0.0);
        assert_eq!(mob.death_time, 0);
        assert!(
            mob.events
                .iter()
                .any(|event| matches!(event, mc_bamboo_blocks::entity::mob::MobEvent::Death))
        );
    }

    #[test]
    fn killing_a_ground_item_removes_it_at_once() {
        let (mut global, mut region, id, ids) = world_with_entities();
        on_command(&mut global, &mut region, id, "kill @e[type=item]", None);
        assert!(region.entities.get(ids[1]).is_none());
        assert!(region.entities.get(ids[0]).is_some());
    }

    #[test]
    fn console_kill_uses_the_world_spawn_as_its_position() {
        let (mut global, mut region, _, ids) = world_with_entities();
        let spawn = region.global.spawn.pos;
        // 出生点旁边放一头牛，`@e[distance=..3]` 从控制台的出生点算起。
        let local = region.entities.spawn(Entity::cow(Vec3::new(
            f64::from(spawn.x) + 0.5,
            f64::from(spawn.y),
            f64::from(spawn.z) + 0.5,
        )));
        crate::command::console::on_console_command(
            &mut global,
            &mut region,
            "kill @e[type=cow,distance=..3]",
        );
        assert!(
            region
                .entities
                .get(local)
                .is_some_and(|cow| cow.kind.mob().is_some_and(|mob| mob.dead)),
            "控制台按出生点算距离"
        );
        assert!(
            region
                .entities
                .get(ids[0])
                .is_some_and(|cow| cow.kind.mob().is_some_and(|mob| !mob.dead)),
            "远处的牛没被选中"
        );
    }

    #[test]
    fn killing_a_player_reports_the_missing_death_path() {
        let (mut global, mut region, id, ids) = world_with_entities();
        let before = region.entities.ids();
        on_command(&mut global, &mut region, id, "kill @s", None);
        assert_eq!(region.entities.ids(), before, "没有实体被偷偷杀掉");
        assert!(region.entities.get(ids[0]).is_some());
    }

    #[test]
    fn bare_kill_from_the_console_requires_an_entity() {
        let (mut global, mut region, _, _) = world_with_entities();
        let before = region.entities.ids();
        crate::command::console::on_console_command(&mut global, &mut region, "kill");
        assert_eq!(region.entities.ids(), before, "没有实体可杀");
    }

    fn parse(text: &str) -> mc_bamboo_command::EntitySelector {
        let mut reader = mc_bamboo_command::StringReader::new(text);
        let mc_bamboo_command::Value::Entity(selector) =
            mc_bamboo_command::argument::entity::parse_entity(&mut reader, false, false)
                .expect("合法选择器")
        else {
            panic!("不是选择器");
        };
        assert!(!reader.can_read());
        *selector
    }

    #[test]
    fn a_second_player_is_a_candidate_too() {
        let (_, mut region, id, _) = world_with_entities();
        let other = Player::new(
            crate::inbound::PlayerId(2),
            Uuid::from_u128(2),
            "Mochi".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(10.0, -59.0, 10.0),
        );
        region.insert_player(other);
        let found = select::targets(&region, id, 2, &parse("@e[type=player]"))
            .unwrap_or_else(|_| panic!("玩家"));
        assert_eq!(found.len(), 2);
    }
}
