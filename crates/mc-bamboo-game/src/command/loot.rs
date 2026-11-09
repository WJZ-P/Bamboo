//! `/loot`：用战利品表产出物品并分发到目标。
//!
//! 目标 `give`、`insert`、`replace`、`spawn` 已接上；来源 `kill` 和 `fish` 要
//! 别的实体系统，命令树照官方注册，执行时返回合理的失败。

use mc_bamboo_blocks::{
    BlockState, World,
    entity::{Entity, EntityKind},
};
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_loot::{LootParams, LootStack, LootTable, Position};
use mc_bamboo_protocol::{item::ItemStack, text::Component};

use crate::{
    command::{
        blocks::block_position,
        feedback::{self, CommandFail, fail, fail_with},
        players, select,
    },
    container::kind_of,
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 来源的种类，决定求值时用哪张表。
#[derive(Clone, Copy)]
enum Source {
    Loot,
    Mine,
    Kill,
    Fish,
}

/// 目标的种类。
#[derive(Clone, Copy)]
enum Target {
    Give,
    Insert,
    ReplaceBlock,
    ReplaceEntity,
    Spawn,
}

struct Request {
    source: Source,
    target: Target,
}

/// 命令树里每个可执行节点都绑定同一组变体；这里导出目标 × 来源的入口。
macro_rules! variants {
    ($($name:ident : $source:ident, $target:ident;)*) => {
        $(pub(crate) fn $name(
            global: &mut Global,
            region: &mut Region,
            id: PlayerId,
            parsed: &ParsedCommand,
        ) -> Result<(), CommandFail> {
            loot_command(global, region, id, parsed, Request {
                source: Source::$source,
                target: Target::$target,
            })
        })*
    };
}

variants! {
    give_loot: Loot, Give;
    give_mine: Mine, Give;
    give_kill: Kill, Give;
    give_fish: Fish, Give;
    insert_loot: Loot, Insert;
    insert_mine: Mine, Insert;
    insert_kill: Kill, Insert;
    insert_fish: Fish, Insert;
    spawn_loot: Loot, Spawn;
    spawn_mine: Mine, Spawn;
    spawn_kill: Kill, Spawn;
    spawn_fish: Fish, Spawn;
    replace_block_loot: Loot, ReplaceBlock;
    replace_block_mine: Mine, ReplaceBlock;
    replace_block_kill: Kill, ReplaceBlock;
    replace_block_fish: Fish, ReplaceBlock;
    replace_entity_loot: Loot, ReplaceEntity;
    replace_entity_mine: Mine, ReplaceEntity;
    replace_entity_kill: Kill, ReplaceEntity;
    replace_entity_fish: Fish, ReplaceEntity;
}

/// 官方 `LootCommand` 的四类来源里，Bamboo 现在只求 `loot` 和 `mine`。
/// `kill` / `fish` 要实体掉落表和钓鱼参数，暂不支持。
fn need_entity(request: &Request) -> Result<(), CommandFail> {
    match request.source {
        Source::Kill | Source::Fish => Err(fail("commands.drop.no_loot_table")),
        _ => Ok(()),
    }
}

fn loot_command(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    request: Request,
) -> Result<(), CommandFail> {
    need_entity(&request)?;
    let player = region
        .players
        .get(&id)
        .ok_or_else(|| fail("permissions.requires.player"))?;
    let source_position = parsed
        .get("pos")
        .and_then(Value::as_block_pos)
        .map(|coords| block_position(coords, player))
        .unwrap_or(BlockPos::containing(player.position()));
    let target_position = parsed
        .get("targetPos")
        .and_then(Value::as_block_pos)
        .map(|coords| block_position(coords, player));
    let drop_position = parsed
        .get("targetPos")
        .and_then(Value::as_vec3)
        .map(|coords| coords.position(player.position(), player.yaw, player.pitch))
        .unwrap_or(player.position());
    let stacks = evaluate(global, region, parsed, &request, source_position)?;
    let count: i32 = stacks.iter().map(|stack| stack.count).sum();
    match request.target {
        Target::Give => {
            let selector = parsed
                .get("players")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            let targets = select::players(region, id, players::level_of(region, id), selector)?;
            for target in targets {
                if let Some(player) = region.players.get_mut(&target) {
                    for stack in &stacks {
                        player.inventory.add(to_stack(stack), true);
                    }
                }
            }
        }
        Target::Insert | Target::ReplaceBlock => {
            let Some(pos) = target_position else {
                return Err(fail("command.failed"));
            };
            let slots = match request.target {
                Target::ReplaceBlock => {
                    let slot = slot_index(parsed)?;
                    let count = parsed.get("count").and_then(Value::as_integer).unwrap_or(1);
                    Some((slot, count.max(0) as usize))
                }
                _ => None,
            };
            distribute_container(region, pos, &stacks, slots)?;
        }
        Target::ReplaceEntity => {
            let selector = parsed
                .get("entities")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            let targets = select::entities(region, id, players::level_of(region, id), selector)?;
            let slot = slot_index(parsed)?;
            for target in targets {
                if let Some(player) = region.players.get_mut(&target) {
                    for (offset, stack) in stacks.iter().enumerate() {
                        if let Some(item) = player
                            .inventory
                            .slots
                            .get_mut((slot as usize).saturating_add(offset))
                        {
                            *item = if offset < stacks.len() {
                                to_stack(stack)
                            } else {
                                ItemStack::empty()
                            };
                        }
                    }
                }
            }
        }
        Target::Spawn => spawn_items(region, drop_position, &stacks),
    }
    let name = parsed.get("loot_table").and_then(Value::as_str);
    let key = match name {
        Some(_) => "commands.drop.success.multiple_with_table",
        None => "commands.drop.success.multiple",
    };
    let mut args = vec![count.into()];
    if let Some(name) = name {
        args.push(Component::text(name).into());
    }
    feedback::success(region, id, Component::translate(key, args), true);
    Ok(())
}

/// 官方 `LootCommand.dropInWorld`：每件物品一个 `ItemEntity`，位置就是
/// `targetPos`，`setDefaultPickUpDelay` 之后 `addFreshEntity`。速度的两次
/// `nextDouble` 由物品实体自己的随机源在生成时取（`Entity::ground_item`）。
fn spawn_items(region: &mut Region, position: Vec3, stacks: &[LootStack]) {
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    for loot in stacks {
        let mut entity = Entity::ground_item(position, to_stack(loot));
        if let EntityKind::Item(item) = &mut entity.kind {
            item.set_default_pickup_delay();
        }
        world.spawn_entity(entity);
    }
}

fn slot_index(parsed: &ParsedCommand) -> Result<i32, CommandFail> {
    let name = parsed
        .get("slot")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("argument.slot.invalid"))?;
    // 官方 `SlotArgument` 把 `container.N`、`hotbar.N`、`inventory.N` 等映射到
    // `Inventory` 的下标；这里用简化的数字解析，不支持的名字直接失败。
    let suffix = name.rsplit_once('.').map_or(name, |(_, suffix)| suffix);
    suffix
        .parse::<i32>()
        .ok()
        .filter(|index| (0..41).contains(index))
        .ok_or_else(|| fail("argument.slot.invalid"))
}

fn evaluate(
    global: &Global,
    region: &Region,
    parsed: &ParsedCommand,
    request: &Request,
    pos: BlockPos,
) -> Result<Vec<LootStack>, CommandFail> {
    let fallback = region.random.source();
    let sequences = &global.state().sequences;
    match request.source {
        Source::Loot => {
            let name = parsed
                .get("loot_table")
                .and_then(Value::as_str)
                .ok_or_else(|| fail("command.failed"))?;
            let table = LootTable::from_name(name).map_err(|_| fail("command.failed"))?;
            let params = LootParams::new(0.0).with_origin(Position::new(
                f64::from(pos.x),
                f64::from(pos.y),
                f64::from(pos.z),
            ));
            table
                .get_random_items_with_seed(&params, 0, Some(sequences), fallback)
                .map_err(|_| fail("command.failed"))
        }
        Source::Mine => {
            // 方块掉落表按注册表查，用官方同样的一组参数求值（不带破坏者/工具）。
            let state = region
                .chunks
                .block(pos)
                .map(BlockState)
                .unwrap_or(BlockState(blocks::AIR));
            let Some(table_name) = mc_bamboo_data::loot::block_loot_table(state.block().name)
            else {
                return Err(fail_with(
                    "commands.drop.no_loot_table.block",
                    [state.block().name.into()],
                ));
            };
            let table = LootTable::from_name(table_name).map_err(|_| fail("command.failed"))?;
            let mut params = LootParams::new(0.0).with_origin(Position::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ));
            params.block_state = Some(state);
            table
                .get_random_items_with_seed(&params, 0, Some(sequences), fallback)
                .map_err(|_| fail("command.failed"))
        }
        Source::Kill | Source::Fish => unreachable!("need_entity 已经挡住"),
    }
}

fn to_stack(loot: &LootStack) -> ItemStack {
    // 战利品函数写下的组件还是 JSON；物品组件编码器接上之前先只带物品和数量。
    let _ = &loot.components;
    ItemStack::new(loot.item, loot.count)
}

fn distribute_container(
    region: &mut Region,
    pos: BlockPos,
    stacks: &[LootStack],
    slots: Option<(i32, usize)>,
) -> Result<(), CommandFail> {
    let state = region
        .chunks
        .block(pos)
        .map(BlockState)
        .and_then(kind_of)
        .ok_or_else(|| fail("command.failed"))?;
    let entry = crate::container::entry_in(&mut region.containers, pos, state);
    let start = slots.map_or(0, |(slot, _)| slot.max(0) as usize);
    let limit = slots.map_or(entry.slots.len(), |(_, count)| count);
    for (offset, stack) in stacks.iter().take(limit).enumerate() {
        if let Some(slot) = entry.slots.get_mut(start + offset) {
            *slot = to_stack(stack);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::PlayerId,
        player::Player,
        region::Region,
    };

    fn world() -> (Global, Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, -59.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.insert_player(player);
        (Global::default(), region, id)
    }

    #[test]
    fn give_from_a_loot_table_adds_items() {
        let (mut global, mut region, id) = world();
        let before: i32 = region.players[&id]
            .inventory
            .slots
            .iter()
            .map(|stack| stack.count)
            .sum();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "loot give Kitten loot minecraft:blocks/diamond_ore",
            None,
        );
        let after: i32 = region.players[&id]
            .inventory
            .slots
            .iter()
            .map(|stack| stack.count)
            .sum();
        assert!(
            after > before,
            "loot give 应该产出物品: {before} -> {after}"
        );
    }

    #[test]
    fn mine_uses_the_blocks_drop_table() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y();
        command::on_command(
            &mut global,
            &mut region,
            id,
            &format!("loot give Kitten mine 2 {} 2", y - 1),
            None,
        );
        let after: i32 = region.players[&id]
            .inventory
            .slots
            .iter()
            .map(|stack| stack.count)
            .sum();
        assert!(after > 0);
    }

    #[test]
    fn spawn_creates_ground_items_at_the_requested_position() {
        let (mut global, mut region, id) = world();
        let before = region.entities.iter().count();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "loot spawn 4 -59 6 loot minecraft:blocks/diamond_ore",
            None,
        );
        let items: Vec<_> = region
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, mc_bamboo_blocks::entity::EntityKind::Item(_)))
            .collect();
        assert_eq!(items.len(), before + 1, "spawn 应该生成一件物品实体");
        let item = items[0];
        // `Vec3Argument.vec3()` 把整数坐标居中到方块中心。
        assert_eq!(item.position, mc_bamboo_core::Vec3::new(4.5, -59.0, 6.5));
        match &item.kind {
            mc_bamboo_blocks::entity::EntityKind::Item(ground) => {
                assert_eq!(ground.pickup_delay, 10);
                assert!(!ground.stack.is_empty());
            }
            _ => panic!("不是物品实体"),
        }
        assert!(
            region.players[&id]
                .inventory
                .slots
                .iter()
                .all(|stack| stack.is_empty())
        );
    }

    #[test]
    fn unsupported_sources_return_a_failure_without_changing_the_inventory() {
        let (mut global, mut region, id) = world();
        for text in [
            "loot give Kitten kill Kitten",
            "loot give Kitten fish minecraft:gameplay/fishing 8 -59 8",
        ] {
            command::on_command(&mut global, &mut region, id, text, None);
        }
        assert!(
            region.players[&id]
                .inventory
                .slots
                .iter()
                .all(|stack| stack.is_empty())
        );
        assert_eq!(region.entities.iter().count(), 0);
    }

    #[test]
    fn tree_and_permission_follow_the_report() {
        assert!(
            commands()
                .graph
                .parse("loot give Kitten loot minecraft:blocks/stone", 2)
                .is_ok()
        );
        assert!(
            commands()
                .graph
                .parse("loot give Kitten loot minecraft:blocks/stone", 0)
                .is_err()
        );
        assert!(
            commands()
                .graph
                .parse("loot insert 8 -59 8 mine 8 -59 8 mainhand", 2)
                .is_ok()
        );
        assert!(
            commands()
                .graph
                .parse(
                    "loot replace block 8 -59 8 container.0 2 loot minecraft:blocks/stone",
                    2
                )
                .is_ok()
        );
        assert!(
            commands()
                .graph
                .parse("loot spawn 8 -59 8 kill Kitten", 2)
                .is_ok()
        );
    }
}
