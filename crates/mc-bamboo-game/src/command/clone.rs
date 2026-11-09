//! `/clone`。同一个世界里的复制，放置顺序、失败条件和邻居更新跟官方
//! `CloneCommands` 一致。方块实体数据（容器、方块上的物品组件、命令方块内容）
//! 也一起复制；官方的跨维度分支只有一个世界，按同维度的那份处理。

use std::collections::{BTreeMap, VecDeque};

use mc_bamboo_blocks::{BlockState, World};
use mc_bamboo_command::{BlockPredicateInput, ParsedCommand};
use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        blocks::{block_position, loaded, required_block_pos, update_neighbours},
        feedback::{self, CommandFail, fail, fail_with},
    },
    container::ContainerEntry,
    global::Global,
    inbound::PlayerId,
    interaction,
    region::Region,
};

/// 官方 `CloneCommands.Mode`。`Force` 和 `Move` 允许源和目标重叠。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloneMode {
    Force,
    Move,
    Normal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    Replace,
    Masked,
    Filtered,
}

struct Style {
    mode: CloneMode,
    filter: Filter,
    strict: bool,
}

/// 一格要复制的内容。官方 `CloneBlockInfo` 加上 Bamboo 的方块实体数据。
#[derive(Clone)]
struct BlockInfo {
    pos: BlockPos,
    state: BlockState,
    previous: BlockState,
    /// 源格子里有方块实体时，保存下来的数据。
    entity: Option<EntityData>,
}

/// Bamboo 的方块实体状态分散在几张表里，复制时整份搬过去。
#[derive(Clone)]
struct EntityData {
    container: Option<ContainerEntry>,
    block_items: Option<ComponentPatch>,
    command_block: Option<crate::command_block::CommandBlockData>,
}

/// 官方 `clone`：先收集，再清空目标区，再放，最后更新邻居。
fn clone_in(
    world: &mut World,
    entities: &mut EntityTables<'_>,
    from: BlockPos,
    to: BlockPos,
    destination: BlockPos,
    filter: Option<&BlockPredicateInput>,
    style: Style,
) -> Result<i32, CommandFail> {
    let min = BlockPos::new(from.x.min(to.x), from.y.min(to.y), from.z.min(to.z));
    let max = BlockPos::new(from.x.max(to.x), from.y.max(to.y), from.z.max(to.z));
    let span = BlockPos::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let dest_max = destination.offset(span.x, span.y, span.z);
    let dest_min = BlockPos::new(
        destination.x.min(dest_max.x),
        destination.y.min(dest_max.y),
        destination.z.min(dest_max.z),
    );
    let dest_max = BlockPos::new(
        destination.x.max(dest_max.x),
        destination.y.max(dest_max.y),
        destination.z.max(dest_max.z),
    );
    if style.mode == CloneMode::Normal && intersects(min, max, dest_min, dest_max) {
        return Err(fail("commands.clone.overlap"));
    }
    let area = i64::from(span.x)
        .saturating_add(1)
        .saturating_mul(i64::from(span.y).saturating_add(1))
        .saturating_mul(i64::from(span.z).saturating_add(1));
    let limit = i64::from(world.block_game_rules().max_block_modifications);
    if area > limit {
        return Err(fail_with(
            "commands.clone.toobig",
            [TranslateArg::from(limit), area.into()],
        ));
    }
    loaded(world, from)?;
    loaded(world, to)?;
    loaded(world, destination)?;
    loaded(world, dest_max)?;

    let offset = BlockPos::new(dest_min.x - min.x, dest_min.y - min.y, dest_min.z - min.z);
    // 官方按 z、y、x 三层循环收集，分成实心、有方块实体、其它三张表。
    let mut solid = Vec::new();
    let mut with_entity = Vec::new();
    let mut other = Vec::new();
    let mut sources = VecDeque::new();
    for z in min.z..=max.z {
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let pos = BlockPos::new(x, y, z);
                let state = world.block_state(pos);
                if !passes(filter, style.filter, state, pos) {
                    continue;
                }
                let target = pos.offset(offset.x, offset.y, offset.z);
                let previous = world.block_state(target);
                let entity = read_entity(entities, pos);
                let info = BlockInfo {
                    pos: target,
                    state,
                    previous,
                    entity: entity.clone(),
                };
                if entity.is_some() {
                    with_entity.push(info.clone());
                    sources.push_back(pos);
                } else if world.block_state(pos).is_solid_render() {
                    solid.push(info.clone());
                    sources.push_back(pos);
                } else {
                    other.push(info.clone());
                    sources.push_front(pos);
                }
            }
        }
    }

    // 官方 `clone` 的标志：strict 时是 816 | 2，普通是 2（只通知客户端）。
    let flags = if style.strict { 816 | 2 } else { 2 };
    let barrier = BlockState(mc_bamboo_data::blocks::BARRIER);
    let air = BlockState(mc_bamboo_data::blocks::AIR);
    // 官方 `Mode.MOVE` 的清空在放目标区之前：先把源格子换成屏障（避免旧方块
    // 掉落），再写成空气。这样源区和目标区重叠时，重叠格子上放好的新方块
    // 不会被之后的清空抹掉。
    if style.mode == CloneMode::Move {
        for pos in &sources {
            world.set_block(*pos, barrier, flags | 816);
        }
        let clear_flags = if style.strict { flags } else { 3 };
        for pos in &sources {
            world.set_block(*pos, air, clear_flags);
            remove_entity(entities, *pos);
        }
    }

    let mut blocks = solid;
    blocks.extend(with_entity.iter().cloned());
    blocks.extend(other);
    // 官方先用屏障清掉目标区（反序），再正序放新方块。
    for info in blocks.iter().rev() {
        world.set_block(info.pos, barrier, flags | 816);
        remove_entity(entities, info.pos);
    }
    let mut count = 0;
    for info in &blocks {
        if world.set_block(info.pos, info.state, flags) {
            count += 1;
        }
    }
    for info in &with_entity {
        if let Some(entity) = &info.entity {
            write_entity(entities, info.pos, entity);
        }
        // 官方为带方块实体的格子再放一次状态，触发方块实体的刷新。
        world.set_block(info.pos, info.state, flags);
    }
    if !style.strict {
        for info in blocks.iter().rev() {
            update_neighbours(world, info.pos, info.previous);
        }
    }
    if count == 0 {
        return Err(fail("commands.clone.failed"));
    }
    Ok(count)
}

fn intersects(a: BlockPos, b: BlockPos, c: BlockPos, d: BlockPos) -> bool {
    a.x <= d.x && c.x <= b.x && a.y <= d.y && c.y <= b.y && a.z <= d.z && c.z <= b.z
}

/// `replace` 全部通过；`masked` 跳过源格子的空气；`filtered` 用方块谓词。
fn passes(
    filter: Option<&BlockPredicateInput>,
    kind: Filter,
    state: BlockState,
    pos: BlockPos,
) -> bool {
    let _ = pos;
    match kind {
        Filter::Replace => true,
        Filter::Masked => !state.is_air(),
        Filter::Filtered => filter.is_some_and(|filter| filter.test(state)),
    }
}

/// 一个世界里三张方块实体表借到同一个 `World` 生命周期之外。
struct EntityTables<'a> {
    containers: &'a mut BTreeMap<BlockPos, ContainerEntry>,
    block_items: &'a mut BTreeMap<BlockPos, ComponentPatch>,
    command_blocks: &'a mut BTreeMap<BlockPos, crate::command_block::CommandBlockData>,
}

fn read_entity(tables: &EntityTables<'_>, pos: BlockPos) -> Option<EntityData> {
    let container = tables.containers.get(&pos).cloned();
    let block_items = tables.block_items.get(&pos).cloned();
    let command_block = tables.command_blocks.get(&pos).cloned();
    if container.is_none() && block_items.is_none() && command_block.is_none() {
        return None;
    }
    Some(EntityData {
        container,
        block_items,
        command_block,
    })
}

fn write_entity(tables: &mut EntityTables<'_>, pos: BlockPos, data: &EntityData) {
    match &data.container {
        Some(container) => {
            tables.containers.insert(pos, container.clone());
        }
        None => {
            tables.containers.remove(&pos);
        }
    }
    match &data.block_items {
        Some(patch) => {
            tables.block_items.insert(pos, patch.clone());
        }
        None => {
            tables.block_items.remove(&pos);
        }
    }
    match &data.command_block {
        Some(block) => {
            tables.command_blocks.insert(pos, block.clone());
        }
        None => {
            tables.command_blocks.remove(&pos);
        }
    }
}

fn remove_entity(tables: &mut EntityTables<'_>, pos: BlockPos) {
    tables.containers.remove(&pos);
    tables.block_items.remove(&pos);
    tables.command_blocks.remove(&pos);
}

/// 命令树里每个可执行节点的样式由 `Builder` 绑成单独的闭包；这里导出一组入口。
macro_rules! variants {
    ($($name:ident : $filter:ident, $mode:ident, $strict:expr;)*) => {
        $(pub(crate) fn $name(
            global: &mut Global,
            region: &mut Region,
            id: PlayerId,
            parsed: &ParsedCommand,
        ) -> Result<(), CommandFail> {
            clone_command(global, region, id, parsed, Style {
                filter: Filter::$filter,
                mode: CloneMode::$mode,
                strict: $strict,
            })
        })*
    };
}

variants! {
    replace_normal: Replace, Normal, false;
    replace_force: Replace, Force, false;
    replace_move: Replace, Move, false;
    masked_normal: Masked, Normal, false;
    masked_force: Masked, Force, false;
    masked_move: Masked, Move, false;
    filtered_normal: Filtered, Normal, false;
    filtered_force: Filtered, Force, false;
    filtered_move: Filtered, Move, false;
    strict_replace_normal: Replace, Normal, true;
    strict_replace_force: Replace, Force, true;
    strict_replace_move: Replace, Move, true;
    strict_masked_normal: Masked, Normal, true;
    strict_masked_force: Masked, Force, true;
    strict_masked_move: Masked, Move, true;
    strict_filtered_normal: Filtered, Normal, true;
    strict_filtered_force: Filtered, Force, true;
    strict_filtered_move: Filtered, Move, true;
}

fn clone_command(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    style: Style,
) -> Result<(), CommandFail> {
    let _ = global;
    let (from, to, destination, filter) = {
        let player = region
            .players
            .get(&id)
            .ok_or_else(|| fail("permissions.requires.player"))?;
        let from = block_position(required_block_pos(parsed, "begin")?, player);
        let to = block_position(required_block_pos(parsed, "end")?, player);
        let destination = block_position(required_block_pos(parsed, "destination")?, player);
        let filter = parsed
            .get("filter")
            .and_then(|value| value.as_block_predicate())
            .cloned();
        (from, to, destination, filter)
    };
    // 只有一个维度：命令树里的 `sourceDimension` / `targetDimension` 必须等于主世界。
    for name in ["sourceDimension", "targetDimension"] {
        if let Some(dimension) = parsed.get(name).and_then(|value| value.as_str())
            && dimension != crate::region::DIMENSION
        {
            return Err(fail("argument.dimension.invalid"));
        }
    }
    let (result, effects, containers, block_items, command_blocks) = {
        let mut containers = std::mem::take(&mut region.containers);
        let mut block_items = std::mem::take(&mut region.block_items);
        let mut command_blocks = std::mem::take(&mut region.command_blocks);
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut tables = EntityTables {
            containers: &mut containers,
            block_items: &mut block_items,
            command_blocks: &mut command_blocks,
        };
        let result = clone_in(
            &mut world,
            &mut tables,
            from,
            to,
            destination,
            filter.as_ref(),
            style,
        );
        let effects = world.into_effects();
        (result, effects, containers, block_items, command_blocks)
    };
    region.containers = containers;
    region.block_items = block_items;
    region.command_blocks = command_blocks;
    interaction::send_effects(region, Some(id), &effects);
    let count = result?;
    feedback::success(
        region,
        id,
        Component::translate("commands.clone.success", [count.into()]),
        true,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;
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

    fn run(global: &mut Global, region: &mut Region, id: PlayerId, command: &str) {
        command::on_command(global, region, id, command, None);
    }

    fn at(region: &Region, pos: BlockPos) -> u32 {
        region.chunks.block(pos).unwrap_or(0)
    }

    #[test]
    fn replace_copies_a_rectangle_and_force_allows_overlap() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y() + 1;
        run(
            &mut global,
            &mut region,
            id,
            &format!("fill 2 {y} 2 3 {y} 2 stone"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 5 {y} 2"),
        );
        assert_eq!(at(&region, BlockPos::new(5, y, 2)), blocks::STONE);
        assert_eq!(at(&region, BlockPos::new(6, y, 2)), blocks::STONE);
        // normal 模式不允许重叠，force 可以复制到原地。
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 2 {y} 2"),
        );
        assert_eq!(at(&region, BlockPos::new(5, y, 2)), blocks::STONE);
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 2 {y} 2 force"),
        );
        assert_eq!(at(&region, BlockPos::new(2, y, 2)), blocks::STONE);
    }

    #[test]
    fn masked_skips_air_and_filtered_uses_a_predicate() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y() + 1;
        run(
            &mut global,
            &mut region,
            id,
            &format!("setblock 2 {y} 2 stone"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 6 {y} 2 masked"),
        );
        assert_eq!(at(&region, BlockPos::new(6, y, 2)), blocks::STONE);
        assert_eq!(at(&region, BlockPos::new(7, y, 2)), blocks::AIR);
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 8 {y} 2 filtered air"),
        );
        assert_eq!(at(&region, BlockPos::new(8, y, 2)), blocks::AIR);
        assert_eq!(at(&region, BlockPos::new(9, y, 2)), blocks::AIR);
    }

    #[test]
    fn move_clears_the_source() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y() + 1;
        run(
            &mut global,
            &mut region,
            id,
            &format!("setblock 2 {y} 2 stone"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 2 {y} 2 4 {y} 2 replace move"),
        );
        let source_state = at(&region, BlockPos::new(2, y, 2));
        let target_state = at(&region, BlockPos::new(4, y, 2));
        assert_eq!(
            source_state,
            blocks::AIR,
            "source={source_state} target={target_state}"
        );
        assert_eq!(target_state, blocks::STONE);
    }

    /// 官方 `CloneCommands.clone` 的 `Mode.MOVE` 允许源区和目标区重叠。
    ///
    /// 手算：源区 x=2,3,4（z=2 固定），方块依次是 stone、dirt、stone；
    /// 目标起点 x=3，所以三格的映射是 2→3、3→4、4→5。
    /// 官方顺序是「先收集三格的旧状态（此时 x=3 仍是 dirt），再把源区
    /// 2,3,4 全部屏障→空气，最后把收集到的状态写到 3,4,5」。
    /// 因此结果应为 x=2 空气、x=3 stone、x=4 dirt、x=5 stone。
    /// 如果清空源区放在放完目标区之后，x=3、x=4 会被错误地抹成空气。
    #[test]
    fn overlapping_move_keeps_the_blocks_placed_after_clearing() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y() + 1;
        run(
            &mut global,
            &mut region,
            id,
            &format!("setblock 2 {y} 2 stone"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("setblock 3 {y} 2 dirt"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("setblock 4 {y} 2 stone"),
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 4 {y} 2 3 {y} 2 replace move"),
        );
        let states: Vec<u32> = (2..=5)
            .map(|x| at(&region, BlockPos::new(x, y, 2)))
            .collect();
        assert_eq!(
            states,
            vec![blocks::AIR, blocks::STONE, blocks::DIRT, blocks::STONE]
        );
    }

    #[test]
    fn area_limit_and_overlap_follow_the_official_errors() {
        let (mut global, mut region, id) = world();
        let y = mc_bamboo_world::flat::surface_y() + 1;
        // 正常范围里重叠会失败。
        assert!(
            commands()
                .graph
                .parse(&format!("clone 2 {y} 2 3 {y} 2 2 {y} 2"), 2)
                .is_ok()
        );
        // 把上限降到 1，两个格子就超了。
        run(
            &mut global,
            &mut region,
            id,
            "gamerule max_block_modifications 1",
        );
        run(
            &mut global,
            &mut region,
            id,
            &format!("clone 2 {y} 2 3 {y} 2 6 {y} 2"),
        );
        assert_eq!(at(&region, BlockPos::new(6, y, 2)), blocks::AIR);
    }
}
