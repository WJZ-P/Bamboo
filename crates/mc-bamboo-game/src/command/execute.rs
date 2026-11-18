//! `/execute`：按官方 `ExecuteCommand` 改变执行上下文再执行链尾命令。
//!
//! `/execute`: change the execution context like official `ExecuteCommand`,
//! then run the chained command.

use mc_bamboo_command::{ChainStep, EntityAnchor, ParsedCommand, Value, wrap_degrees};
use mc_bamboo_core::{BlockPos, Vec3};

use crate::{
    command::{
        commands,
        feedback::{self, CommandFail, fail},
        select::{self, Target},
        source,
    },
    global_port::GlobalPort,
    inbound::PlayerId,
    region::Region,
};

/// `/execute` 链上一步的最大体积（`ExecuteCommand.MAX_TEST_AREA`）。
const MAX_TEST_AREA: i64 = 32768;

/// 一次执行上下文。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Context {
    source: PlayerId,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    anchor: EntityAnchor,
    entity: Option<Target>,
    permission: u8,
}

/// 链尾命令执行完以后要写回的结果。
#[derive(Debug, Clone, Copy, Default)]
struct Outcome {
    result: i32,
    success: i32,
}

/// 执行一条 `/execute`。`ParsedCommand.chain` 是从外到内的各层重定向上下文。
pub(crate) fn execute_global(
    global: &mut crate::global::Global,
    region: &mut Region,
    source_id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let mut port = GlobalPort::Direct(global);
    execute(&mut port, region, source_id, parsed)
}

fn execute(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source_id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let base = context_source(region, source_id);
    let mut contexts = vec![Context {
        source: source_id,
        position: base.position,
        yaw: base.yaw,
        pitch: base.pitch,
        anchor: EntityAnchor::Feet,
        entity: base.entity,
        permission: source::level(region, source_id),
    }];
    let mut stores: Vec<&ChainStep> = Vec::new();
    let run_line = parsed.run_command.as_deref();
    let command_text = parsed.input.as_str();
    let steps = parsed
        .chain
        .iter()
        .take_while(|step| step.kind != "run")
        .collect::<Vec<_>>();
    for step in steps {
        if matches!(step.kind.as_str(), "result" | "success") {
            stores.push(step);
        }
        if matches!(
            step.kind.as_str(),
            "store" | "result" | "success" | "score" | "bossbar" | "block" | "entity" | "storage"
        ) {
            continue;
        }
        let mut next = Vec::new();
        for context in contexts {
            if let Some(branches) = apply_step(region, parsed, step, context, command_text)? {
                next.extend(branches);
            }
        }
        contexts = next;
        if contexts.is_empty() {
            // 官方 `command.execute.conditions.not_met`：条件失败不执行链尾命令。
            return Ok(());
        }
    }
    let Some(line) = run_line else {
        return Err(fail("command.unknown.command"));
    };
    let mut outcome = Outcome::default();
    for context in &contexts {
        let inner = commands().graph.parse(line, context.permission);
        match inner {
            Ok(parsed_inner) => {
                let overridden = context_to_source(region, *context);
                let ran = source::with_override(context.source, overridden, || {
                    run_action(port, region, *context, line, parsed_inner.clone())
                });
                if ran {
                    outcome.success += 1;
                    outcome.result += 1;
                }
            }
            Err(failure) => feedback::report(
                region,
                source_id,
                line,
                &feedback::fail_parse(line, &failure.error, failure.cursor),
            ),
        }
    }
    for store in stores {
        apply_store(port, region, parsed, store, outcome)?;
    }
    Ok(())
}

/// 链尾命令通过共用的动作表执行；来源由 `source::with_override` 换成当前上下文。
/// 返回这个分支是不是执行成功。
fn run_action(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    context: Context,
    line: &str,
    parsed: ParsedCommand,
) -> bool {
    let action = commands()
        .actions
        .get(parsed.action)
        .and_then(|action| *action);
    let outcome = match action {
        Some(crate::command::Action::Island(action)) => {
            action(port, region, context.source, &parsed)
        }
        Some(crate::command::Action::Global(action)) => {
            match port {
                GlobalPort::Direct(global) => action(global, region, context.source, &parsed),
                GlobalPort::Queued(_) => {
                    // 排队实现把整条全局动作连同当前上下文一起交回主控。
                    // The queued port submits the whole action with this context.
                    port.submit(
                        region,
                        crate::global_port::GlobalEffect::Command {
                            source: context.source,
                            command: line.to_owned(),
                            parsed,
                            action,
                        },
                    );
                    Ok(())
                }
            }
        }
        None => Err(fail("command.failed")),
    };
    if let Err(failure) = outcome {
        feedback::report(region, context.source, line, &failure);
        false
    } else {
        true
    }
}

/// 执行链上的一步；`None` 表示这一分支被条件挡掉。
fn apply_step(
    region: &mut Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
    _command_text: &str,
) -> Result<Option<Vec<Context>>, CommandFail> {
    let value = |name: &str| parsed.step_value(step, name);
    match step.kind.as_str() {
        "as" => {
            let selector = value("targets")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            let targets = selection(region, context, selector)?;
            Ok(Some(
                targets
                    .into_iter()
                    .map(|entity| Context {
                        entity: Some(entity),
                        ..context
                    })
                    .collect(),
            ))
        }
        "at" | "positioned" | "rotated" if value("targets").is_some() => {
            let selector = value("targets")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            let targets = selection(region, context, selector)?;
            let mut out = Vec::new();
            for target in targets {
                if let Some(pose) = target_pose(region, target) {
                    let mut changed = context;
                    if step.kind != "rotated" {
                        changed.position = pose.0;
                    }
                    if step.kind != "positioned" {
                        changed.yaw = pose.1;
                        changed.pitch = pose.2;
                    }
                    out.push(changed);
                }
            }
            Ok(Some(out))
        }
        "positioned" => {
            let coords = value("pos")
                .and_then(Value::as_vec3)
                .ok_or_else(|| fail("command.failed"))?;
            let mut changed = context;
            changed.position = source::position(coords, base_source(context));
            Ok(Some(vec![changed]))
        }
        "rotated" => {
            let rotation = value("rot")
                .and_then(Value::as_rotation)
                .ok_or_else(|| fail("command.failed"))?;
            let (yaw, pitch) = rotation.resolve(context.yaw, context.pitch);
            Ok(Some(vec![Context {
                yaw: wrap_degrees(yaw),
                pitch: pitch.clamp(-90.0, 90.0),
                ..context
            }]))
        }
        "facing" => {
            let to = if let Some(coords) = value("pos").and_then(Value::as_vec3) {
                source::position(coords, base_source(context))
            } else {
                let selector = step
                    .arguments
                    .iter()
                    .find(|(name, _)| name == "targets")
                    .and_then(|(_, value)| Value::as_entity(value))
                    .ok_or_else(|| fail("command.failed"))?;
                let anchor = step
                    .arguments
                    .iter()
                    .find(|(name, _)| name == "anchor")
                    .and_then(|(_, value)| Value::as_entity_anchor(value))
                    .unwrap_or(context.anchor);
                let targets = selection(region, context, selector)?;
                let Some(target) = targets.first().copied() else {
                    return Ok(Some(Vec::new()));
                };
                let (position, _yaw, pitch) = target_pose(region, target).unwrap_or_default();
                if anchor == EntityAnchor::Eyes {
                    Vec3::new(position.x, position.y + 1.62, position.z)
                } else {
                    let _ = pitch;
                    position
                }
            };
            let (yaw, pitch) = look_angles(context.position, to);
            Ok(Some(vec![Context {
                yaw,
                pitch,
                ..context
            }]))
        }
        "align" => {
            let axes = value("axes")
                .and_then(Value::as_str)
                .ok_or_else(|| fail("command.failed"))?;
            let mut changed = context;
            for axis in axes.chars() {
                let coordinate = match axis {
                    'x' => &mut changed.position.x,
                    'y' => &mut changed.position.y,
                    _ => &mut changed.position.z,
                };
                *coordinate = coordinate.floor();
            }
            Ok(Some(vec![changed]))
        }
        "anchored" => {
            let anchor = value("anchor")
                .and_then(Value::as_entity_anchor)
                .ok_or_else(|| fail("command.failed"))?;
            Ok(Some(vec![Context { anchor, ..context }]))
        }
        "in" => {
            let dimension = value("dimension")
                .and_then(Value::as_str)
                .ok_or_else(|| fail("command.failed"))?;
            if dimension != crate::region::DIMENSION {
                return Err(fail("argument.dimension.invalid"));
            }
            Ok(Some(vec![context]))
        }
        "if" | "unless" => conditional(region, parsed, step, context),
        other => Err(fail_with_text(other)),
    }
}

fn conditional(
    region: &mut Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Result<Option<Vec<Context>>, CommandFail> {
    let positive = step.kind == "if";
    let value = |name: &str| parsed.step_value(step, name);
    let (passed, branches) = if let Some(coords) = value("pos").and_then(Value::as_block_pos) {
        let pos = BlockPos::containing(source::position(coords, base_source(context)));
        if let Some(predicate) = value("block").and_then(Value::as_block_predicate) {
            let state = region
                .chunks
                .block(pos)
                .map(mc_bamboo_blocks::BlockState)
                .unwrap_or(mc_bamboo_blocks::BlockState(0));
            (predicate.test(state), Vec::new())
        } else {
            (region.chunks.block(pos).is_some(), Vec::new())
        }
    } else if value("start").is_some() {
        let from = block(value("start"), context)?;
        let to = block(value("end"), context)?;
        let destination = block(value("destination"), context)?;
        let masked = step
            .arguments
            .iter()
            .any(|(_, value)| matches!(value.as_str(), Some("masked")));
        let volume = volume(from, to)?;
        let mut equal = true;
        'outer: for x in 0..volume.0 {
            for y in 0..volume.1 {
                for z in 0..volume.2 {
                    let offset = BlockPos::new(x, y, z);
                    let source_pos = from.offset(offset.x, offset.y, offset.z);
                    let target_pos = destination.offset(offset.x, offset.y, offset.z);
                    let left = region.chunks.block(source_pos);
                    let right = region.chunks.block(target_pos);
                    let same = left == right || (masked && left.is_some_and(|state| state == 0));
                    if !same {
                        equal = false;
                        break 'outer;
                    }
                }
            }
        }
        (equal, Vec::new())
    } else if let Some(selector) = value("entities").and_then(Value::as_entity) {
        let targets = selection(region, context, selector)?;
        (targets.is_empty(), targets)
    } else if let Some(holder) = value("target").and_then(Value::as_score_holder) {
        let objective = value("targetObjective")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("command.failed"))?;
        let board = &region.global.scoreboard;
        let score = match holder {
            mc_bamboo_command::ScoreHolderSpec::Text(name) => {
                board.score_value(name, objective).unwrap_or(0)
            }
            mc_bamboo_command::ScoreHolderSpec::Wildcard => 0,
            mc_bamboo_command::ScoreHolderSpec::Selector(_) => {
                return Err(fail("command.failed"));
            }
        };
        let passed = if let Some((min, max)) = value("range").and_then(Value::as_int_bounds) {
            score >= min.unwrap_or(i32::MIN) && score <= max.unwrap_or(i32::MAX)
        } else if let Some(other) = value("source").and_then(Value::as_score_holder) {
            let other_objective = value("sourceObjective")
                .and_then(Value::as_str)
                .ok_or_else(|| fail("command.failed"))?;
            let other_score = match other {
                mc_bamboo_command::ScoreHolderSpec::Text(name) => {
                    board.score_value(name, other_objective).unwrap_or(0)
                }
                mc_bamboo_command::ScoreHolderSpec::Wildcard => 0,
                mc_bamboo_command::ScoreHolderSpec::Selector(_) => {
                    return Err(fail("command.failed"));
                }
            };
            let op = step
                .arguments
                .iter()
                .find(|(name, _)| matches!(name.as_str(), "<" | "<=" | "=" | ">=" | ">"))
                .map(|(name, _)| name.as_str());
            match op {
                Some("<") => score < other_score,
                Some("<=") => score <= other_score,
                Some(">") => score > other_score,
                Some(">=") => score >= other_score,
                _ => score == other_score,
            }
        } else {
            return Err(fail("command.failed"));
        };
        (passed, Vec::new())
    } else {
        // `predicate` 的求值依赖战利品表数据源；本期明确报错，不静默判假。
        return Err(fail("command.failed"));
    };
    let keep = passed == positive;
    if !keep {
        return Ok(None);
    }
    if branches.is_empty() || !positive {
        return Ok(Some(vec![context]));
    }
    Ok(Some(
        branches
            .into_iter()
            .map(|entity| Context {
                entity: Some(entity),
                ..context
            })
            .collect(),
    ))
}

fn apply_store(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    parsed: &ParsedCommand,
    store: &ChainStep,
    outcome: Outcome,
) -> Result<(), CommandFail> {
    let value = if store.kind == "result" {
        outcome.result
    } else {
        outcome.success
    };
    let score_layer = parsed.chain.iter().find(|step| step.kind == "score");
    if let Some(layer) = score_layer {
        let GlobalPort::Direct(global) = port else {
            return Err(fail("command.failed"));
        };
        let holder = layer
            .arguments
            .iter()
            .find(|(name, _)| name == "targets" || name == "target")
            .and_then(|(_, value)| Value::as_score_holder(value))
            .ok_or_else(|| fail("command.failed"))?;
        let objective = layer
            .arguments
            .iter()
            .find(|(name, _)| name == "objective")
            .and_then(|(_, value)| Value::as_str(value))
            .ok_or_else(|| fail("command.failed"))?;
        let names = match holder {
            mc_bamboo_command::ScoreHolderSpec::Wildcard => global.scoreboard().tracked_players(),
            mc_bamboo_command::ScoreHolderSpec::Text(text) => vec![text.clone()],
            mc_bamboo_command::ScoreHolderSpec::Selector(_) => {
                return Err(fail("command.failed"));
            }
        };
        for name in names {
            global.scoreboard_mut().set_score(
                &crate::scoreboard::Holder {
                    name,
                    display: None,
                },
                objective,
                value,
                region,
            );
        }
        global.publish(region);
        return Ok(());
    }
    let bossbar = parsed
        .chain
        .iter()
        .skip_while(|step| step.level <= store.level)
        .find(|step| step.kind == "bossbar");
    if let Some(layer) = bossbar {
        if !matches!(port, GlobalPort::Direct(_)) {
            return Err(fail("command.failed"));
        }
        let id = layer
            .arguments
            .iter()
            .find(|(name, _)| name == "id")
            .and_then(|(_, value)| Value::as_str(value))
            .ok_or_else(|| fail("command.failed"))?;
        let _ = id;
        return Ok(());
    }
    // `store ... block|entity|storage` 要 NBT 路径；第三期接 `/data` 时复用。
    Err(fail("command.failed"))
}

fn fail_with_text(_text: &str) -> CommandFail {
    fail("command.failed")
}

/// 把执行上下文翻译成动作会读到的 `Source`。
fn context_to_source(region: &Region, context: Context) -> source::Source {
    let mut out = source::of(region, context.source);
    out.position = context.position;
    out.yaw = context.yaw;
    out.pitch = context.pitch;
    out.entity = context.entity;
    out
}

/// 当前上下文的 `source::Source` 形状，只给坐标解析用。
fn context_source(region: &Region, id: PlayerId) -> source::Source {
    source::of(region, id)
}

fn base_source(context: Context) -> source::Source {
    source::Source {
        position: context.position,
        yaw: context.yaw,
        pitch: context.pitch,
        entity: context.entity,
    }
}

fn selection(
    region: &Region,
    context: Context,
    selector: &mc_bamboo_command::EntitySelector,
) -> Result<Vec<Target>, CommandFail> {
    // 选择器从当前上下文的位置和执行者出发；`select::targets` 目前按来源 ID 取上下文，
    // 所以这里用一个临时入口逐项过滤，避免把当前上下文写回玩家表。
    let temporary = source::Source {
        position: context.position,
        yaw: context.yaw,
        pitch: context.pitch,
        entity: context.entity,
    };
    select::targets_with(region, temporary, context.permission, selector)
}

fn target_pose(region: &Region, target: Target) -> Option<(Vec3, f32, f32)> {
    match target {
        Target::Player(id) => {
            let player = region.players.get(&id)?;
            Some((player.position(), player.yaw, player.pitch))
        }
        Target::World(uid) => {
            let public = region.entities.public_at(uid)?;
            let pitch = match &public.kind {
                mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.x_rot,
                _ => 0.0,
            };
            Some((public.position, public.yaw, pitch))
        }
    }
}

fn block(value: Option<&Value>, context: Context) -> Result<BlockPos, CommandFail> {
    let coords = value
        .and_then(Value::as_block_pos)
        .ok_or_else(|| fail("command.failed"))?;
    Ok(BlockPos::containing(source::position(
        coords,
        base_source(context),
    )))
}

fn volume(from: BlockPos, to: BlockPos) -> Result<(i32, i32, i32), CommandFail> {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    let dz = i64::from(to.z) - i64::from(from.z);
    let size = (dx.abs() + 1)
        .saturating_mul(dy.abs() + 1)
        .saturating_mul(dz.abs() + 1);
    if size > MAX_TEST_AREA {
        return Err(fail("command.execute.blocks.toobig"));
    }
    Ok((
        (dx.unsigned_abs() + 1) as i32,
        (dy.unsigned_abs() + 1) as i32,
        (dz.unsigned_abs() + 1) as i32,
    ))
}

fn look_angles(from: Vec3, to: Vec3) -> (f32, f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = wrap_degrees((-(dz.atan2(dx) * 180.0 / std::f64::consts::PI)) as f32 - 90.0);
    let pitch = wrap_degrees((-(dy.atan2(horizontal) * 180.0 / std::f64::consts::PI)) as f32);
    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use crate::command::{commands, on_command, tests::world_with};

    #[test]
    fn execute_chain_is_parsed_layer_by_layer() {
        let graph = &commands().graph;
        for line in [
            "execute as @e at @s run tp ~ ~1 ~",
            "execute positioned 8 0 8 run setblock 8 -60 8 stone",
            "execute if block 8 -60 8 stone run say yes",
        ] {
            let parsed = graph
                .parse(line, 2)
                .unwrap_or_else(|error| panic!("{line}: {error:?}"));
            assert!(parsed.run_command.is_some(), "{line}");
            assert!(parsed.chain.iter().any(|step| step.kind == "run"), "{line}");
        }
    }

    #[test]
    fn chain_layers_have_their_own_arguments() {
        let parsed = commands()
            .graph
            .parse("execute positioned 4 1 6 run setblock 4 -60 6 stone", 2)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let positioned = parsed
            .chain
            .iter()
            .find(|step| step.kind == "positioned")
            .expect("positioned 层");
        assert!(parsed.step_value(positioned, "pos").is_some());
        let setblock = parsed
            .chain
            .iter()
            .find(|step| step.kind == "setblock")
            .expect("setblock 层");
        assert!(parsed.step_value(setblock, "pos").is_some());
    }

    #[test]
    fn positioned_run_places_a_block_from_the_new_position() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(
            &mut global,
            &mut region,
            id,
            "execute positioned 4 -60 6 run setblock ~ ~ ~ stone",
            None,
        );
        assert_eq!(
            region
                .chunks
                .block(mc_bamboo_core::BlockPos::new(4, -60, 6)),
            Some(mc_bamboo_data::blocks::STONE)
        );
    }

    #[test]
    fn if_block_false_does_not_run_the_tail() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        let pos = mc_bamboo_core::BlockPos::new(4, -60, 6);
        on_command(
            &mut global,
            &mut region,
            id,
            "execute if block 4 -60 6 stone run setblock 4 -60 6 stone",
            None,
        );
        assert_ne!(
            region.chunks.block(pos),
            Some(mc_bamboo_data::blocks::STONE)
        );
    }

    #[test]
    fn store_success_score_writes_after_all_branches() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        let mut player = crate::player::Player::new(
            crate::inbound::PlayerId(2),
            uuid::Uuid::from_u128(2),
            "Mochi".to_owned(),
            mc_bamboo_protocol::packet::play::GameMode::Creative,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            mc_bamboo_core::Vec3::new(10.5, -60.0, 8.5),
        );
        player.permission_level = 2;
        region.insert_player(player);
        on_command(
            &mut global,
            &mut region,
            id,
            "scoreboard objectives add result dummy",
            None,
        );
        on_command(
            &mut global,
            &mut region,
            id,
            "execute store success score #total result as @a run say hi",
            None,
        );
        assert_eq!(
            global.scoreboard().score_value("#total", "result"),
            Some(2),
            "两个分支都成功"
        );
    }

    #[test]
    fn as_and_at_fork_over_entities() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        let cow_a = region.entities.spawn(mc_bamboo_blocks::entity::Entity::cow(
            mc_bamboo_core::Vec3::new(2.5, -60.0, 2.5),
        ));
        let cow_b = region.entities.spawn(mc_bamboo_blocks::entity::Entity::cow(
            mc_bamboo_core::Vec3::new(6.5, -60.0, 6.5),
        ));
        on_command(
            &mut global,
            &mut region,
            id,
            "execute as @e[type=cow] at @s run setblock ~ ~ ~ stone",
            None,
        );
        for uid in [cow_a, cow_b] {
            let pos = mc_bamboo_core::BlockPos::containing(
                region.entities.public_at(uid).expect("牛还在").position,
            );
            assert_eq!(
                region.chunks.block(pos),
                Some(mc_bamboo_data::blocks::STONE),
                "每头牛脚下都放了一块石头"
            );
        }
    }
}
