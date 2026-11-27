//! `/execute`：按官方 `ExecuteCommand` 改变执行上下文再执行链尾命令。
//!
//! `/execute`: change the execution context like official `ExecuteCommand`,
//! then run the chained command.

use mc_bamboo_command::{ChainStep, EntityAnchor, ParsedCommand, Value, wrap_degrees};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::{NbtPath, Tag};
use mc_bamboo_protocol::text::Component;

use crate::{
    command::{
        commands,
        data_source::{self, Kind, Source},
        feedback::{self, CommandFail, fail, fail_with},
        predicate_view,
        select::{self, Target},
        source,
    },
    global_port::GlobalPort,
    inbound::PlayerId,
    region::Region,
};

/// `/execute` 链上一步的最大体积（`ExecuteCommand.MAX_TEST_AREA`）。
const MAX_TEST_AREA: i64 = 32768;

/// `execute on` 的八种关系，顺序同官方 `createRelationOperations`。
///
/// The eight `execute on` relations, in the order of official `createRelationOperations`.
pub(crate) const RELATIONS: [&str; 8] = [
    "owner",
    "leasher",
    "target",
    "attacker",
    "vehicle",
    "controller",
    "origin",
    "passengers",
];

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

/// 一个条件算出来的东西。官方的条件分三种：只有真假的（`addConditional`）、
/// 数个数的（`createNumericConditionalHandler`）和 `blocks` 的「比对通过时的格数」。
/// 写在链中间时三种都只看 [`Verdict::holds`]，写在链尾时反馈各不相同。
///
/// What one condition evaluates to. Vanilla has three kinds: plain booleans
/// (`addConditional`), counts (`createNumericConditionalHandler`) and the matched block
/// count of `blocks`. Mid-chain all three reduce to [`Verdict::holds`]; as the tail of a
/// command each has its own feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Bool(bool),
    Count(i32),
    Region(Option<i32>),
}

impl Verdict {
    /// 链中间用的真假：数个数的是「大于零」，`blocks` 是「比对通过」。
    ///
    /// The mid-chain truth value: a count is "greater than zero", `blocks` is "matched".
    fn holds(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::Count(count) => count > 0,
            Self::Region(count) => count.is_some(),
        }
    }

    /// 条件写在链尾（后面没有 `run`）时的反馈：成功消息，或者要报的错误。
    ///
    /// The feedback when the condition ends the command (no `run`): the success
    /// message, or the error to report.
    fn tail(self, positive: bool) -> Result<Component, CommandFail> {
        let pass = || Component::translate("commands.execute.conditional.pass", []);
        let pass_count = |count: i32| {
            Component::translate("commands.execute.conditional.pass_count", [count.into()])
        };
        let failed = || fail("commands.execute.conditional.fail");
        let failed_count =
            |count: i32| fail_with("commands.execute.conditional.fail_count", [count.into()]);
        match (self, positive) {
            (Self::Bool(value), _) if value == positive => Ok(pass()),
            (Self::Bool(_), _) => Err(failed()),
            (Self::Count(count), true) if count > 0 => Ok(pass_count(count)),
            (Self::Count(_), true) => Err(failed()),
            (Self::Count(0), false) => Ok(pass()),
            (Self::Count(count), false) => Err(failed_count(count)),
            (Self::Region(Some(count)), true) => Ok(pass_count(count)),
            (Self::Region(None), true) => Err(failed()),
            (Self::Region(Some(count)), false) => Err(failed_count(count)),
            (Self::Region(None), false) => Ok(pass()),
        }
    }
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
    // 条件写在链尾、后面没有 `run`：官方走条件节点自己的 `executes`，对每个来源报一次。
    // A condition at the tail with no `run`: vanilla runs the node's own `executes`,
    // once per source.
    let tail = if run_line.is_none() {
        steps
            .iter()
            .rev()
            .find(|step| is_layer_head(parsed, step))
            .filter(|step| matches!(step.kind.as_str(), "if" | "unless"))
            .copied()
    } else {
        None
    };
    // 官方 `ChainModifiers.isForked`：链上出现过分叉节点以后，后面的错误都不再报给来源，
    // 出错的那个来源只是被丢掉（`CommandSourceStack.handleError` 的 `forked`）。
    // Official `ChainModifiers.isForked`: once a fork node has appeared in the chain, later
    // errors are no longer reported and the failing source is simply dropped (the
    // `forked` flag of `CommandSourceStack.handleError`).
    let mut forked = false;
    for step in steps {
        if matches!(step.kind.as_str(), "result" | "success") {
            stores.push(step);
        }
        if tail.is_some_and(|tail| std::ptr::eq(tail, step)) {
            for context in &contexts {
                report_tail(region, parsed, step, *context, forked);
            }
            return Ok(());
        }
        // 一层里第二个及以后的字面量（`if` 后面的 `biome`、`on` 后面的关系名）只是
        // 这一步的参数，由这一层的第一个字面量处理。
        // Later literals in a layer (`biome` after `if`, the relation after `on`) are
        // arguments of the step, handled by the layer's first literal.
        if !is_layer_head(parsed, step) && !matches!(step.kind.as_str(), "result" | "success") {
            continue;
        }
        if matches!(
            step.kind.as_str(),
            "store"
                | "result"
                | "success"
                | "score"
                | "bossbar"
                | "matches"
                | "="
                | "<"
                | "<="
                | ">"
                | ">="
        ) || (matches!(
            step.kind.as_str(),
            "block" | "entity" | "storage" | "int" | "short" | "long" | "float" | "double" | "byte"
        ) && !stores.is_empty())
        {
            continue;
        }
        forked |= is_fork(parsed, step);
        let mut next = Vec::new();
        for context in contexts {
            match apply_step(region, parsed, step, context, command_text) {
                Ok(Some(branches)) => next.extend(branches),
                Ok(None) => {}
                // 官方 `BuildContexts.execute`：分叉模式下出错的来源被丢掉，其余来源继续。
                // Official `BuildContexts.execute`: in forked mode a failing source is
                // dropped and the others carry on.
                Err(_) if forked => {}
                Err(failure) => return Err(failure),
            }
        }
        contexts = next;
        if contexts.is_empty() {
            // 官方 `command.execute.conditions.not_met`：条件失败不执行链尾命令。
            // Bamboo 的 `send_command_feedback` 关着时不发系统消息。
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
                    run_action(port, region, *context, line, parsed_inner.clone(), forked)
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
        apply_store(port, region, source_id, parsed, store, outcome)?;
    }
    Ok(())
}

/// 这一步是不是它那一层的第一个字面量。
///
/// Whether this step is the first literal of its layer.
fn is_layer_head(parsed: &ParsedCommand, step: &ChainStep) -> bool {
    parsed
        .chain
        .iter()
        .find(|other| other.level == step.level)
        .is_some_and(|head| std::ptr::eq(head, step))
}

/// 一层里第二个字面量：`if biome` 的 `biome`、`on vehicle` 的 `vehicle`。
///
/// The second literal of a layer: `biome` in `if biome`, `vehicle` in `on vehicle`.
fn layer_branch<'a>(parsed: &'a ParsedCommand, step: &ChainStep) -> Option<&'a str> {
    parsed
        .chain
        .iter()
        .filter(|other| other.level == step.level)
        .map(|other| other.kind.as_str())
        .nth(1)
}

/// 官方条件节点的 `executes`：通过发成功反馈，否则把错误报给来源。
///
/// The `executes` of an official condition node: success feedback when it passes,
/// otherwise the error is reported to the source.
fn report_tail(
    region: &mut Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
    forked: bool,
) {
    let outcome =
        evaluate(region, parsed, step, context).and_then(|verdict| verdict.tail(step.kind == "if"));
    let overridden = context_to_source(region, context);
    source::with_override(context.source, overridden, || match outcome {
        Ok(message) => feedback::success(region, context.source, message, false),
        Err(failure) if !forked => {
            feedback::report(region, context.source, &parsed.input, &failure);
        }
        Err(_) => {}
    });
}

/// 这一步在官方树里是不是 `fork` 节点：`as`、`at`、所有 `if` / `unless`、`on`、
/// `positioned as`、`rotated as`、`facing entity`。其余（`positioned <pos>`、`rotated <rot>`、
/// `facing <pos>`、`align`、`anchored`、`in`、`summon`、`store`）是普通重定向。
///
/// Whether this step is a `fork` node in the official tree: `as`, `at`, every `if` /
/// `unless`, `on`, `positioned as`, `rotated as` and `facing entity`. The rest are
/// plain redirects.
fn is_fork(parsed: &ParsedCommand, step: &ChainStep) -> bool {
    match step.kind.as_str() {
        "as" | "at" | "if" | "unless" | "on" => true,
        "positioned" | "rotated" => layer_branch(parsed, step) == Some("as"),
        "facing" => layer_branch(parsed, step) == Some("entity"),
        _ => false,
    }
}

/// 链尾命令通过共用的动作表执行；来源由 `source::with_override` 换成当前上下文。
/// 返回这个分支是不是执行成功。
fn run_action(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    context: Context,
    line: &str,
    parsed: ParsedCommand,
    forked: bool,
) -> bool {
    let leaf = parsed.chain.is_empty();
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
        // 官方 `ExecuteCommand.execute`：`sender.handleError(e, modifiers.isForked(), ...)`。
        // Official `ExecuteCommand.execute`: `sender.handleError(e, modifiers.isForked(), ...)`.
        if !forked {
            feedback::report(region, context.source, line, &failure);
        }
        false
    } else {
        if leaf {
            crate::minecart_command::record_success(context.source);
        }
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
        "on" => relation(context, layer_branch(parsed, step)),
        // 官方 `spawnEntityAndRedirect` 要走 `SummonCommand.createEntity`；还没接。
        // Official `spawnEntityAndRedirect` goes through `SummonCommand.createEntity`,
        // which is not wired in yet.
        "summon" => Err(fail("command.failed")),
        "data" | "block" | "entity" | "storage" if is_data_or_store_target(parsed, step.level) => {
            Ok(Some(vec![context]))
        }
        other => Err(fail_with_text(other)),
    }
}

/// 官方 `expandOneToOneEntityRelation` / `expandOneToManyEntityRelation`：来源没有实体时
/// 结果是空的，整条链在这里停下。有实体时要查主人、牵引者、目标、攻击者、载具、
/// 操控者、来源实体和乘客，这些关系还没有统一的查询入口，明确报错。
///
/// Official `expandOneToOneEntityRelation` / `expandOneToManyEntityRelation`: with no
/// source entity the result is empty and the chain stops here. With an entity the owner,
/// leasher, target, attacker, vehicle, controller, origin and passengers need lookups that
/// have no shared entry point yet, so it fails loudly.
fn relation(context: Context, name: Option<&str>) -> Result<Option<Vec<Context>>, CommandFail> {
    if name.is_none_or(|name| !RELATIONS.contains(&name)) {
        return Err(fail("command.failed"));
    }
    if context.entity.is_none() {
        return Ok(Some(Vec::new()));
    }
    Err(fail("command.failed"))
}

fn conditional(
    region: &mut Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Result<Option<Vec<Context>>, CommandFail> {
    let verdict = evaluate(region, parsed, step, context)?;
    if verdict.holds() != (step.kind == "if") {
        return Ok(Some(Vec::new()));
    }
    // 官方 `expect(..., ...)`：`if/unless entity` 用 `getOptionalEntities` 判真假，
    // 命中与否只保留原上下文一次，不为每个实体分叉。
    Ok(Some(vec![context]))
}

/// 官方 `BlockPosArgument.getLoadedBlockPos`：区块没加载报 `argument.pos.unloaded`，
/// 高度在世界外报 `argument.pos.outofworld`。
///
/// Official `BlockPosArgument.getLoadedBlockPos`: `argument.pos.unloaded` for an unloaded
/// chunk, `argument.pos.outofworld` for a height outside the world.
fn loaded_block(region: &Region, pos: BlockPos) -> Result<BlockPos, CommandFail> {
    if !region.chunks.is_loaded(pos.chunk()) {
        return Err(fail("argument.pos.unloaded"));
    }
    if region.chunks.block(pos).is_none() {
        return Err(fail("argument.pos.outofworld"));
    }
    Ok(pos)
}

/// `if biome <pos> <biome>`：`ResourceOrTagArgument.Result.test(level.getBiome(pos))`。
///
/// `if biome <pos> <biome>`: `ResourceOrTagArgument.Result.test(level.getBiome(pos))`.
fn biome_matches(region: &Region, pos: BlockPos, wanted: &str) -> Result<bool, CommandFail> {
    let pos = loaded_block(region, pos)?;
    let biome = predicate_view::biome_at(region, pos).ok_or_else(|| fail("command.failed"))?;
    Ok(match wanted.strip_prefix('#') {
        Some(tag) => mc_bamboo_loot::tag_ids("minecraft:worldgen/biome", tag)
            .is_some_and(|ids| ids.contains(&biome)),
        None => mc_bamboo_loot::biome_id(wanted) == Some(biome),
    })
}

/// 官方 `ExecuteCommand.isChunkLoaded`：区块在、状态到 `ENTITY_TICKING`、实体也已加载。
///
/// Official `ExecuteCommand.isChunkLoaded`: the chunk exists, has reached
/// `ENTITY_TICKING`, and its entities are loaded.
fn chunk_loaded(region: &Region, pos: BlockPos) -> bool {
    let chunk = pos.chunk();
    region.chunks.is_loaded(chunk) && region.tick_levels.chunk_ticks_entities(chunk)
}

/// 官方 `DimensionArgument.getDimension(c, "dimension") == source.getLevel()`。Bamboo 只有
/// 主世界：另外两个原版维度存在但不是当前世界，判 false；其它名字照官方报
/// `argument.dimension.invalid`。
///
/// Official `DimensionArgument.getDimension(c, "dimension") == source.getLevel()`. Bamboo
/// has only the overworld: the other two vanilla dimensions exist but are not the current
/// level, so they test false; any other name is `argument.dimension.invalid` as in vanilla.
fn dimension_matches(name: &str) -> Result<bool, CommandFail> {
    match name {
        crate::region::DIMENSION => Ok(true),
        "minecraft:the_nether" | "minecraft:the_end" => Ok(false),
        other => Err(fail_with(
            "argument.dimension.invalid",
            [other.to_owned().into()],
        )),
    }
}

/// 官方 `ExecuteCommand` 的 `biome`、`loaded`、`dimension`、`predicate`、`function`、
/// `stopwatch`、`items`、`slots` 八个条件；不是这几个时返回 `None`。
///
/// The `biome`, `loaded`, `dimension`, `predicate`, `function`, `stopwatch`, `items` and
/// `slots` conditions of official `ExecuteCommand`, or `None` for any other branch.
fn named_condition(
    region: &Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Option<Result<Verdict, CommandFail>> {
    let value = |name: &str| parsed.step_value(step, name);
    let text = |name: &str| {
        value(name)
            .and_then(Value::as_str)
            .ok_or_else(|| fail("command.failed"))
    };
    let position = |name: &str| block(value(name), context);
    Some(match layer_branch(parsed, step)? {
        "biome" => position("pos")
            .and_then(|pos| biome_matches(region, pos, text("biome")?))
            .map(Verdict::Bool),
        "loaded" => position("pos").map(|pos| Verdict::Bool(chunk_loaded(region, pos))),
        "dimension" => text("dimension")
            .and_then(dimension_matches)
            .map(Verdict::Bool),
        "predicate" => value("predicate")
            .and_then(Value::as_nbt)
            .ok_or_else(|| fail("command.failed"))
            .and_then(|tag| predicate_view::test(region, context.position, context.entity, tag))
            .map(Verdict::Bool),
        // Bamboo 没有数据包函数：官方 `FunctionArgument.getFunctions` 对不存在的函数或
        // 函数标签抛这两个错误。
        // Bamboo has no data-pack functions: official `FunctionArgument.getFunctions`
        // throws these two errors for a missing function or function tag.
        "function" => Err(match text("name") {
            Ok(name) if name.starts_with('#') => fail_with(
                "arguments.function.tag.unknown",
                [name[1..].to_owned().into()],
            ),
            Ok(name) => fail_with("arguments.function.unknown", [name.to_owned().into()]),
            Err(failure) => failure,
        }),
        // Bamboo 没有 `/stopwatch`，秒表表永远是空的：官方 `checkStopwatch` 报不存在。
        // Bamboo has no `/stopwatch`, so no stopwatch ever exists: official
        // `checkStopwatch` reports it as missing.
        "stopwatch" => Err(match text("id") {
            Ok(id) => fail_with("commands.stopwatch.does_not_exist", [id.to_owned().into()]),
            Err(failure) => failure,
        }),
        // 槽位来源（`SlotSourceArgument`）和物品访问器还没有实现。
        // Slot sources (`SlotSourceArgument`) and item accessors are not implemented.
        "items" | "slots" => Err(fail("command.failed")),
        _ => return None,
    })
}

/// 算出一个条件的结果，不管它写在链中间还是链尾。
///
/// Evaluate one condition, whether it sits mid-chain or at the tail.
fn evaluate(
    region: &mut Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Result<Verdict, CommandFail> {
    if let Some(verdict) = named_condition(region, parsed, step, context) {
        return verdict;
    }
    let value = |name: &str| parsed.step_value(step, name);
    let passed = if let Some(coords) = value("pos").and_then(Value::as_block_pos) {
        let pos = BlockPos::containing(source::position(coords, base_source(context)));
        if let Some(predicate) = value("block").and_then(Value::as_block_predicate) {
            let state = region
                .chunks
                .block(pos)
                .map(mc_bamboo_blocks::BlockState)
                .unwrap_or(mc_bamboo_blocks::BlockState(0));
            predicate.test(state)
        } else {
            region.chunks.block(pos).is_some()
        }
    } else if value("start").is_some() {
        let from = block(value("start"), context)?;
        let to = block(value("end"), context)?;
        let destination = block(value("destination"), context)?;
        let masked = has_literal(parsed, "masked", step.level);
        let volume = volume(from, to)?;
        // 官方 `BoundingBox.fromCorners(start, end)`：从两角的最小角开始数；目标盒的
        // 最小角就是 `destination`。
        // Official `BoundingBox.fromCorners(start, end)` counts from the minimum corner;
        // the destination box's minimum corner is `destination` itself.
        let corner = BlockPos::new(from.x.min(to.x), from.y.min(to.y), from.z.min(to.z));
        let mut count = Some(0);
        'outer: for x in 0..volume.0 {
            for y in 0..volume.1 {
                for z in 0..volume.2 {
                    let source_pos = corner.offset(x, y, z);
                    let target_pos = destination.offset(x, y, z);
                    let left = region.chunks.block(source_pos);
                    // 官方 `skipAir`：源方块是空气时不比较也不计数。
                    // Official `skipAir`: an air source block is neither compared nor counted.
                    if masked && left.is_some_and(|state| state == 0) {
                        continue;
                    }
                    if left != region.chunks.block(target_pos) {
                        count = None;
                        break 'outer;
                    }
                    count = count.map(|count| count + 1);
                }
            }
        }
        return Ok(Verdict::Region(count));
    } else if let Some(selector) = value("entities").and_then(Value::as_entity) {
        let targets = selection(region, context, selector)?;
        return Ok(Verdict::Count(
            i32::try_from(targets.len()).unwrap_or(i32::MAX),
        ));
    } else if !has_literal(parsed, "data", step.level)
        && let Some(holder) = value("target").and_then(Value::as_score_holder)
    {
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
        if let Some((min, max)) = value("range").and_then(Value::as_int_bounds) {
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
            let op = parsed
                .chain
                .iter()
                .filter(|link| link.level == step.level)
                .map(|link| link.kind.as_str())
                .find(|name| matches!(*name, "<" | "<=" | "=" | ">=" | ">"));
            match op {
                Some("<") => score < other_score,
                Some("<=") => score <= other_score,
                Some(">") => score > other_score,
                Some(">=") => score >= other_score,
                _ => score == other_score,
            }
        } else {
            return Err(fail("command.failed"));
        }
    } else if value("source").is_some() && has_literal(parsed, "data", step.level) {
        let source = nbt_source(region, parsed, step, context)?;
        let root = data_source::read_view(&region.global, region, &source).map_err(fail)?;
        let path_text = value("path")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("command.failed"))?;
        let path = NbtPath::parse(path_text).map_err(|_| fail("command.failed"))?;
        // 官方 `NbtPath.countMatching`。
        return Ok(Verdict::Count(
            i32::try_from(path.get(&root).len()).unwrap_or(i32::MAX),
        ));
    } else {
        return Err(fail("command.failed"));
    };
    Ok(Verdict::Bool(passed))
}

fn apply_store(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source_id: PlayerId,
    parsed: &ParsedCommand,
    store: &ChainStep,
    outcome: Outcome,
) -> Result<(), CommandFail> {
    let value = if store.kind == "result" {
        // 官方 `CommandResultCallback` 对每个分叉单独回调：链尾 `setblock`
        // 返回 1，最后一个成功分支把 1 写进目标。
        1.min(outcome.result)
    } else {
        // `success` 对每个分叉回调「本分支成功数 1」，同样是最后一次写 1。
        1.min(outcome.success)
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
    let GlobalPort::Direct(global) = port else {
        return Err(fail("command.failed"));
    };
    let layer = parsed
        .chain
        .iter()
        .find(|step| matches!(step.kind.as_str(), "block" | "entity" | "storage"))
        .ok_or_else(|| fail("command.failed"))?;
    let path_text = layer
        .arguments
        .iter()
        .find(|(name, _)| name == "path")
        .and_then(|(_, value)| Value::as_str(value))
        .ok_or_else(|| fail("command.failed"))?;
    let path = NbtPath::parse(path_text).map_err(|_| fail("command.failed"))?;
    let target = store_nbt_source(
        region,
        parsed,
        layer,
        Context {
            source: source_id,
            position: Vec3::default(),
            yaw: 0.0,
            pitch: 0.0,
            anchor: EntityAnchor::Feet,
            entity: None,
            permission: 0,
        },
    )?;
    let mut root = data_source::read(global, region, &target).map_err(fail)?;
    let number = Tag::Int(value);
    for leaf in path.get_mut(&mut root) {
        *leaf = number.clone();
    }
    data_source::write(global, region, &target, &root).map_err(fail)?;
    Ok(())
}

/// `store ... block|entity|storage` 的参数名是 `target`（官方 `ArgProvider.wrap`）。
fn store_nbt_source(
    region: &Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Result<Source, CommandFail> {
    let value = |name: &str| parsed.step_value(step, name);
    let target = value("target").ok_or_else(|| fail("command.failed"))?;
    let mut out = Source {
        kind: Kind::Storage,
        block: None,
        entity: None,
        storage: None,
    };
    if let Some(coords) = Value::as_block_pos(target) {
        out.kind = Kind::Block;
        out.block = Some(BlockPos::containing(source::position(
            coords,
            base_source(context),
        )));
    } else if let Some(selector) = Value::as_entity(target) {
        out.kind = Kind::Entity;
        out.entity = Some(
            select::targets_with(region, base_source(context), 4, selector)?
                .into_iter()
                .next()
                .ok_or_else(|| fail("argument.entity.notfound.entity"))?,
        );
    } else if let Some(id) = Value::as_str(target) {
        out.kind = Kind::Storage;
        out.storage = Some(id.to_owned());
    } else {
        return Err(fail("command.failed"));
    }
    Ok(out)
}

/// 官方 `if/unless data` 与 `store ... block|entity|storage` 的数据源解析。
fn nbt_source(
    region: &Region,
    parsed: &ParsedCommand,
    step: &ChainStep,
    context: Context,
) -> Result<Source, CommandFail> {
    let value = |name: &str| parsed.step_value(step, name);
    let mut out = Source {
        kind: Kind::Storage,
        block: None,
        entity: None,
        storage: None,
    };
    if let Some(coords) = value("source").and_then(Value::as_block_pos) {
        out.kind = Kind::Block;
        out.block = Some(BlockPos::containing(source::position(
            coords,
            base_source(context),
        )));
    } else if let Some(selector) = value("source").and_then(Value::as_entity) {
        out.kind = Kind::Entity;
        out.entity = Some(
            select::targets_with(region, base_source(context), 4, selector)?
                .into_iter()
                .next()
                .ok_or_else(|| fail("argument.entity.notfound.entity"))?,
        );
    } else if let Some(id) = value("source").and_then(Value::as_str) {
        out.kind = Kind::Storage;
        out.storage = Some(id.to_owned());
    } else {
        return Err(fail("command.failed"));
    }
    Ok(out)
}

/// `store ... block|entity|storage` 的层里那几个字面量只是参数前缀，不是执行步骤。
fn is_data_or_store_target(parsed: &ParsedCommand, level: usize) -> bool {
    parsed.chain.iter().any(|step| {
        step.level == level
            && matches!(
                step.kind.as_str(),
                "data" | "store" | "result" | "success" | "block" | "entity" | "storage"
            )
    })
}

fn has_literal(parsed: &ParsedCommand, name: &str, level: usize) -> bool {
    parsed
        .chain
        .iter()
        .any(|step| step.level == level && step.kind == name)
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
            Some(1),
            "官方每个分叉各自回调一次成功数 1，最后一次写入 1"
        );
    }

    #[test]
    fn store_result_storage_writes_the_command_result() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(
            &mut global,
            &mut region,
            id,
            "execute store result storage minecraft:test Value int 1 run setblock 4 -60 6 stone",
            None,
        );
        assert!(
            global
                .command_storage
                .get("minecraft:test")
                .to_pretty_snbt()
                .contains("1"),
            "store result 写回 NBT"
        );
    }

    #[test]
    fn if_data_checks_the_storage_path() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge storage minecraft:test {Value:1}",
            None,
        );
        global.publish(&mut region);
        on_command(
            &mut global,
            &mut region,
            id,
            "execute if data storage minecraft:test Value run setblock 4 -60 6 stone",
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
    fn unless_data_with_a_missing_path_still_runs() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(
            &mut global,
            &mut region,
            id,
            "execute unless data storage minecraft:test Missing run setblock 4 -60 6 stone",
            None,
        );
        assert_eq!(
            region
                .chunks
                .block(mc_bamboo_core::BlockPos::new(4, -60, 6)),
            Some(mc_bamboo_data::blocks::STONE)
        );
    }

    /// 跑一条命令，返回 (4, -60, 6) 有没有变成石头。
    ///
    /// Run one command and report whether (4, -60, 6) turned into stone.
    fn places_stone(line: &str) -> bool {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(&mut global, &mut region, id, line, None);
        region
            .chunks
            .block(mc_bamboo_core::BlockPos::new(4, -60, 6))
            == Some(mc_bamboo_data::blocks::STONE)
    }

    #[test]
    fn if_predicate_tests_the_source_entity_and_position() {
        const TAIL: &str = " run setblock 4 -60 6 stone";
        for (condition, expected) in [
            (
                r#"if predicate {type:"entity_properties",entity:"this",predicate:{entity_type:"player"}}"#,
                true,
            ),
            (
                r#"unless predicate {type:"entity_properties",entity:"this",predicate:{entity_type:"player"}}"#,
                false,
            ),
            (
                r#"if predicate {type:"entity_properties",entity:"this",predicate:{"type_specific/lightning":{}}}"#,
                false,
            ),
            (
                r#"if predicate {type:"location_check",predicate:{fluid:{fluids:"minecraft:empty"}}}"#,
                true,
            ),
            (
                r#"if predicate {type:"entity_scores",entity:"this",scores:{missing:1}}"#,
                false,
            ),
        ] {
            assert_eq!(
                places_stone(&format!("execute {condition}{TAIL}")),
                expected,
                "{condition}"
            );
        }
    }

    #[test]
    fn world_conditions_follow_the_single_overworld() {
        const TAIL: &str = " run setblock 4 -60 6 stone";
        for (condition, expected) in [
            ("if dimension minecraft:overworld", true),
            ("if dimension minecraft:the_nether", false),
            ("unless dimension minecraft:the_end", true),
            ("if loaded 4 -60 6", true),
            ("unless loaded 4 -60 6", false),
            ("if biome 4 -60 6 #minecraft:is_nether", false),
            // 分叉节点上的错误不报给来源，这一支只是被丢掉。
            // An error on a fork node is not reported; the branch is just dropped.
            ("if stopwatch minecraft:nope 0..", false),
            ("unless function minecraft:nope", false),
        ] {
            assert_eq!(
                places_stone(&format!("execute {condition}{TAIL}")),
                expected,
                "{condition}"
            );
        }
    }

    #[test]
    fn masked_blocks_skip_air_sources() {
        // 源区域全是空气：`masked` 一格都不比，照官方算比对通过（0 格）。
        // The source region is all air: `masked` compares nothing, which vanilla counts
        // as a match of zero blocks.
        assert!(places_stone(
            "execute if blocks 2 -60 2 3 -60 3 8 -60 8 masked run setblock 4 -60 6 stone"
        ));
        assert!(!places_stone(
            "execute unless blocks 2 -60 2 3 -60 3 8 -60 8 masked run setblock 4 -60 6 stone"
        ));
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
