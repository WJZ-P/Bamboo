//! `/setblock` 和 `/fill`。放置标志、失败条件和邻居更新跟官方一样。
//!
//! `/setblock` and `/fill`. Placement flags, failure conditions, and neighbour updates match
//! the official commands.

use mc_bamboo_blocks::{
    BlockState, World,
    world::{
        UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, UPDATE_LIMIT, UPDATE_SKIP_ALL_SIDEEFFECTS,
        UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS,
    },
};
use mc_bamboo_command::{BlockPredicateInput, BlockStateInput, Coordinates, ParsedCommand};
use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::text::TranslateArg;

use crate::{
    command::feedback::{self, CommandFail, fail, fail_with},
    global::Global,
    inbound::PlayerId,
    interaction,
    region::Region,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FillMode {
    Replace,
    Outline,
    Hollow,
    Destroy,
}

#[derive(Clone, Copy)]
struct FillStyle {
    mode: FillMode,
    strict: bool,
    keep: bool,
}

pub(crate) fn setblock_replace(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    set_block(region, id, parsed, false, false, false)
}

pub(crate) fn setblock_destroy(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    set_block(region, id, parsed, true, false, false)
}

pub(crate) fn setblock_keep(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    set_block(region, id, parsed, false, true, false)
}

pub(crate) fn setblock_strict(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    set_block(region, id, parsed, false, false, true)
}

pub(crate) fn fill_replace(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Replace, false, false)
}

pub(crate) fn fill_keep(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Replace, false, true)
}

pub(crate) fn fill_outline(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Outline, false, false)
}

pub(crate) fn fill_hollow(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Hollow, false, false)
}

pub(crate) fn fill_destroy(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Destroy, false, false)
}

pub(crate) fn fill_strict(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    fill(region, id, parsed, FillMode::Replace, true, false)
}

fn set_block(
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    destroy: bool,
    keep: bool,
    strict: bool,
) -> Result<(), CommandFail> {
    let (pos, input) = {
        let player = source_player(region, id)?;
        let pos = block_position(required_block_pos(parsed, "pos")?, player);
        let input = required_state(parsed, "block")?.clone();
        (pos, input)
    };
    let (result, effects) = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let result = set_block_in(&mut world, pos, &input, destroy, keep, strict);
        let effects = world.into_effects();
        (result, effects)
    };
    interaction::send_effects(region, Some(id), &effects);
    result?;
    feedback::success(
        region,
        id,
        mc_bamboo_protocol::text::Component::translate(
            "commands.setblock.success",
            [TranslateArg::from(pos.x), pos.y.into(), pos.z.into()],
        ),
        true,
    );
    Ok(())
}

fn set_block_in(
    world: &mut World,
    pos: BlockPos,
    input: &BlockStateInput,
    destroy: bool,
    keep: bool,
    strict: bool,
) -> Result<(), CommandFail> {
    loaded(world, pos)?;
    if keep && !world.is_empty_block(pos) {
        return Err(fail("commands.setblock.failed"));
    }
    let place_needed = if destroy {
        world.destroy_block(pos, UPDATE_LIMIT);
        // 官方在破坏之后才看还要不要放：目标不是空气，或者格子里还剩东西。
        //
        // Official decides after the destroy: place unless the target is air and the cell is air.
        !input.state.is_air() || !world.block_state(pos).is_air()
    } else {
        true
    };
    let old = world.block_state(pos);
    if place_needed && !place(world, pos, input, place_flags(strict)) {
        return Err(fail("commands.setblock.failed"));
    }
    if !strict {
        update_neighbours(world, pos, old);
    }
    Ok(())
}

fn fill(
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    mode: FillMode,
    strict: bool,
    keep: bool,
) -> Result<(), CommandFail> {
    let (from, to, input, filter) = {
        let player = source_player(region, id)?;
        let from = block_position(required_block_pos(parsed, "from")?, player);
        let to = block_position(required_block_pos(parsed, "to")?, player);
        let input = required_state(parsed, "block")?.clone();
        let filter = parsed
            .get("filter")
            .and_then(|value| value.as_block_predicate())
            .cloned();
        (from, to, input, filter)
    };
    let (result, effects) = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let result = fill_in(
            &mut world,
            from,
            to,
            &input,
            filter.as_ref(),
            FillStyle { mode, strict, keep },
        );
        let effects = world.into_effects();
        (result, effects)
    };
    interaction::send_effects(region, Some(id), &effects);
    let count = result?;
    feedback::success(
        region,
        id,
        mc_bamboo_protocol::text::Component::translate("commands.fill.success", [count.into()]),
        true,
    );
    Ok(())
}

fn fill_in(
    world: &mut World,
    from: BlockPos,
    to: BlockPos,
    input: &BlockStateInput,
    filter: Option<&BlockPredicateInput>,
    style: FillStyle,
) -> Result<i32, CommandFail> {
    let FillStyle { mode, strict, keep } = style;
    loaded(world, from)?;
    loaded(world, to)?;
    let min_x = from.x.min(to.x);
    let min_y = from.y.min(to.y);
    let min_z = from.z.min(to.z);
    let max_x = from.x.max(to.x);
    let max_y = from.y.max(to.y);
    let max_z = from.z.max(to.z);
    let area = (i64::from(max_x - min_x) + 1)
        .saturating_mul(i64::from(max_y - min_y) + 1)
        .saturating_mul(i64::from(max_z - min_z) + 1);
    // `FillCommand` 用 `GameRules.MAX_BLOCK_MODIFICATIONS` 限制体积。
    //
    // `FillCommand` limits the volume with `GameRules.MAX_BLOCK_MODIFICATIONS`.
    let limit = i64::from(world.block_game_rules().max_block_modifications);
    if area > limit {
        return Err(fail_with(
            "commands.fill.toobig",
            [TranslateArg::from(limit), area.into()],
        ));
    }

    let air = BlockStateInput {
        state: BlockState::AIR,
        properties: Vec::new(),
    };
    let flags = place_flags(strict);
    let mut updates = Vec::new();
    let mut count = 0i32;
    // 官方 `BlockPos.betweenClosed`：x 最快，然后 y，然后 z。
    //
    // Official `BlockPos.betweenClosed`: x changes fastest, then y, then z.
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let pos = BlockPos::new(x, y, z);
                let current = world.block_state(pos);
                if keep && !current.is_air() {
                    continue;
                }
                if filter.is_some_and(|filter| !filter.test(current)) {
                    continue;
                }
                let old = current;
                let affected = mode == FillMode::Destroy && world.destroy_block(pos, UPDATE_LIMIT);
                let interior = x != min_x
                    && x != max_x
                    && y != min_y
                    && y != max_y
                    && z != min_z
                    && z != max_z;
                let placed = match mode {
                    FillMode::Outline if interior => None,
                    FillMode::Hollow if interior => Some(&air),
                    _ => Some(input),
                };
                let did_place = placed.is_some_and(|state| place(world, pos, state, flags));
                if did_place {
                    if !strict {
                        updates.push((pos, old));
                    }
                    count += 1;
                } else if affected {
                    count += 1;
                }
            }
        }
    }
    if !strict {
        for (pos, old) in updates {
            update_neighbours(world, pos, old);
        }
    }
    if count == 0 {
        return Err(fail("commands.fill.failed"));
    }
    Ok(count)
}

fn place(world: &mut World, pos: BlockPos, input: &BlockStateInput, flags: u32) -> bool {
    let mut state = if flags & UPDATE_KNOWN_SHAPE != 0 {
        input.state
    } else {
        world.update_from_neighbour_shapes(input.state, pos)
    };
    if state.is_air() {
        state = input.state;
    }
    if state != input.state {
        for (name, value) in &input.properties {
            if let Some(next) = state.try_with_text(name, value) {
                state = next;
            }
        }
    }
    world.set_block(pos, state, flags)
}

/// 官方 `ServerLevel.updateNeighboursOnBlockSet`。`setBlock` 的标志里没有 `UPDATE_NEIGHBORS`。
///
/// Official `ServerLevel.updateNeighboursOnBlockSet`. The `setBlock` flags omit `UPDATE_NEIGHBORS`.
pub(crate) fn update_neighbours(world: &mut World, pos: BlockPos, old: BlockState) {
    let state = world.block_state(pos);
    if !old.is_same_block(state) {
        old.affect_neighbors_after_removal(world, pos);
    }
    world.update_neighbors_at(pos, state.block());
    if state.has_analog_output_signal() {
        world.update_neighbour_for_output_signal(pos, state.block());
    }
}

fn place_flags(strict: bool) -> u32 {
    UPDATE_CLIENTS
        | if strict {
            UPDATE_SKIP_ALL_SIDEEFFECTS
        } else {
            UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS
        }
}

pub(crate) fn loaded(world: &World, pos: BlockPos) -> Result<(), CommandFail> {
    if !world.has_chunk_at(pos) {
        return Err(fail("argument.pos.unloaded"));
    }
    if !world.is_in_world_bounds(pos) {
        return Err(fail("argument.pos.outofworld"));
    }
    Ok(())
}

fn source_player(region: &Region, id: PlayerId) -> Result<&crate::player::Player, CommandFail> {
    region
        .players
        .get(&id)
        .ok_or_else(|| fail("permissions.requires.player"))
}

pub(crate) fn required_block_pos(
    parsed: &ParsedCommand,
    name: &str,
) -> Result<Coordinates, CommandFail> {
    parsed
        .get(name)
        .and_then(|value| value.as_block_pos())
        .ok_or_else(|| fail("command.failed"))
}

fn required_state<'a>(
    parsed: &'a ParsedCommand,
    name: &str,
) -> Result<&'a BlockStateInput, CommandFail> {
    parsed
        .get(name)
        .and_then(|value| value.as_block_state())
        .ok_or_else(|| fail("command.failed"))
}

pub(crate) fn block_position(coords: Coordinates, player: &crate::player::Player) -> BlockPos {
    BlockPos::containing(coords.position(player.position(), player.yaw, player.pitch))
}
