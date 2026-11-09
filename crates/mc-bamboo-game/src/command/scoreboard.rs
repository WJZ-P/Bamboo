//! `/scoreboard`。
//!
//! 语法和反馈按官方 `ScoreboardCommand`：等级 2，`CommandResponseTracker` 按目标数量
//! 选 single/multiple 的翻译键，目标只有被显示出来才开始广播包。

use mc_bamboo_command::{ObjectiveCriteria, ParsedCommand, ScoreHolderSpec, Value, permission};
use mc_bamboo_protocol::{
    components::text::component_from_checked_tag,
    packet::play::NumberFormat,
    text::{Component, TranslateArg},
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
    scoreboard::{Holder, ScoreFailure},
};

/// 一次命令里解析出的记分板名字。
///
/// The score holders parsed from one command.
struct Holders {
    names: Vec<String>,
}

impl Holders {
    fn len(&self) -> usize {
        self.names.len()
    }

    fn first_name(&self) -> &str {
        self.names.first().map(String::as_str).unwrap_or_default()
    }
}

/// 官方 `CommandResponseTracker` 的反馈。`total` 是 `track` 累加的值。
///
/// Official `CommandResponseTracker` feedback. `total` is the sum of tracked values.
struct Response {
    count: usize,
    nonzero: usize,
    total: i32,
}

impl Response {
    fn new() -> Self {
        Self {
            count: 0,
            nonzero: 0,
            total: 0,
        }
    }

    fn track(&mut self, value: i32) {
        self.count += 1;
        self.total = self.total.wrapping_add(value);
        if value != 0 {
            self.nonzero += 1;
        }
    }
}

fn args<const N: usize>(values: [TranslateArg; N]) -> Vec<TranslateArg> {
    values.into()
}

fn translate(key: &'static str, values: impl IntoIterator<Item = TranslateArg>) -> Component {
    Component::translate(key, values)
}

/// 把一次成功消息发出去。`broadcast` 对应官方的 `sendSuccess(..., true/false)`。
fn ok(
    region: &mut Region,
    id: PlayerId,
    key: &'static str,
    values: impl IntoIterator<Item = TranslateArg>,
    broadcast: bool,
) {
    feedback::success(region, id, translate(key, values), broadcast);
}

fn goal(global: &Global, parsed: &ParsedCommand, name: &str) -> Result<String, CommandFail> {
    let name = parsed
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    if !global.scoreboard().objective_exists(name) {
        return Err(fail_with(
            "arguments.objective.notFound",
            args([name.into()]),
        ));
    }
    Ok(name.to_owned())
}

fn writable(global: &Global, parsed: &ParsedCommand, name: &str) -> Result<String, CommandFail> {
    let objective = goal(global, parsed, name)?;
    if global.scoreboard().is_read_only(&objective) {
        return Err(fail_with(
            "arguments.objective.readonly",
            args([objective.into()]),
        ));
    }
    Ok(objective)
}

fn component(parsed: &ParsedCommand, name: &str) -> Result<Component, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_component_tag)
        .cloned()
        .map(component_from_checked_tag)
        .ok_or_else(|| fail("command.failed"))
}

/// 把 SNBT 参数转成数字格式。`blank` 是空格式，`styled` 用样式标签，`fixed` 用组件。
fn styled(parsed: &ParsedCommand) -> Result<NumberFormat, CommandFail> {
    parsed
        .get("style")
        .and_then(Value::as_style_tag)
        .cloned()
        .map(NumberFormat::Styled)
        .ok_or_else(|| fail("command.failed"))
}

fn fixed(parsed: &ParsedCommand) -> Result<NumberFormat, CommandFail> {
    component(parsed, "contents")
        .map(NumberFormat::Fixed)
        .map_err(|_| fail("command.failed"))
}

fn holders(
    global: &Global,
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
) -> Result<Holders, CommandFail> {
    let spec = parsed
        .get(name)
        .and_then(Value::as_score_holder)
        .ok_or_else(|| fail("command.failed"))?;
    let level = region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL);
    let names = match spec {
        ScoreHolderSpec::Wildcard => global.scoreboard().tracked_players(),
        ScoreHolderSpec::Text(text) => vec![text.clone()],
        ScoreHolderSpec::Selector(selector) => {
            let ids = select::with_scoreboard(region, global.scoreboard(), id, level, selector)?;
            ids.iter()
                .filter_map(|player| region.players.get(player).map(|entry| entry.name.clone()))
                .collect()
        }
    };
    if names.is_empty() {
        return Err(fail("argument.scoreHolder.empty"));
    }
    Ok(Holders { names })
}

fn holder_of(global: &Global, name: &str) -> Holder {
    // 显示自动更新只在在线玩家身上生效，官方 `ScoreHolder.getFeedbackDisplayName`。
    let auto = global.scoreboard().auto_updates(name);
    Holder {
        name: name.to_owned(),
        display: auto.then(|| global.scoreboard().format_player_name(name)),
    }
}

fn display_for(global: &Global, name: &str) -> Component {
    global.scoreboard().format_player_name(name)
}

fn objective_name(global: &Global, name: &str) -> Result<Component, CommandFail> {
    global
        .scoreboard()
        .formatted_objective(name)
        .ok_or_else(|| fail("command.failed"))
}

// ---------------------------------------------------------------------------
// objectives
// ---------------------------------------------------------------------------

pub(crate) fn objectives_list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    _parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let board = global.scoreboard();
    let count = board.objective_count();
    if count == 0 {
        ok(
            region,
            id,
            "commands.scoreboard.objectives.list.empty",
            [],
            false,
        );
    } else {
        let list = crate::scoreboard::format_list(&board.objective_entries());
        ok(
            region,
            id,
            "commands.scoreboard.objectives.list.success",
            args([(count as i32).into(), list.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn objectives_add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = parsed
        .get("objective")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    let criteria = parsed
        .get("criteria")
        .and_then(Value::as_criteria)
        .ok_or_else(|| fail("command.failed"))?;
    let display = match parsed.get("displayName") {
        Some(_) => component(parsed, "displayName")?,
        None => Component::text(name),
    };
    if global.scoreboard().objective_exists(name) {
        return Err(fail("commands.scoreboard.objectives.add.duplicate"));
    }
    add(global, name, criteria, display);
    let name_component = objective_name(global, name)?;
    ok(
        region,
        id,
        "commands.scoreboard.objectives.add.success",
        args([name_component.into()]),
        true,
    );
    Ok(())
}

fn add(global: &mut Global, name: &str, criteria: &ObjectiveCriteria, display: Component) {
    global.scoreboard_mut().add_objective(
        name,
        &criteria.name,
        criteria.read_only,
        criteria.hearts,
        display,
    );
}

pub(crate) fn objectives_remove(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = goal(global, parsed, "objective")?;
    let formatted = objective_name(global, &name)?;
    global.scoreboard_mut().remove_objective(&name, region);
    ok(
        region,
        id,
        "commands.scoreboard.objectives.remove.success",
        args([formatted.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn objectives_setdisplay(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let slot = parsed
        .get("slot")
        .and_then(Value::as_display_slot)
        .ok_or_else(|| fail("command.failed"))?;
    let current = global.scoreboard().display_at(slot.0).map(str::to_owned);
    let next = match parsed.get("objective") {
        Some(_) => Some(goal(global, parsed, "objective")?),
        None => None,
    };
    if next.is_none() && current.is_none() {
        return Err(fail("commands.scoreboard.objectives.display.alreadyEmpty"));
    }
    if next.as_deref() == current.as_deref() {
        return Err(fail("commands.scoreboard.objectives.display.alreadySet"));
    }
    let shown = match &next {
        Some(name) => global
            .scoreboard()
            .raw_objective_display(name)
            .cloned()
            .ok_or_else(|| fail("command.failed"))?,
        None => Component::text(""),
    };
    global
        .scoreboard_mut()
        .set_display(slot.0, next.as_deref(), region);
    match next {
        Some(_) => ok(
            region,
            id,
            "commands.scoreboard.objectives.display.set",
            args([slot.name().into(), shown.into()]),
            true,
        ),
        None => ok(
            region,
            id,
            "commands.scoreboard.objectives.display.cleared",
            args([slot.name().into()]),
            true,
        ),
    }
    Ok(())
}

pub(crate) fn objectives_displayname(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = goal(global, parsed, "objective")?;
    let display = component(parsed, "displayName")?;
    if global
        .scoreboard_mut()
        .set_objective_display(&name, display, region)
    {
        let formatted = objective_name(global, &name)?;
        ok(
            region,
            id,
            "commands.scoreboard.objectives.modify.displayname",
            args([name.into(), formatted.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn objectives_render_hearts(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    set_render(global, region, id, parsed, true)
}

pub(crate) fn objectives_render_integer(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    set_render(global, region, id, parsed, false)
}

fn set_render(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    hearts: bool,
) -> Result<(), CommandFail> {
    let name = goal(global, parsed, "objective")?;
    if global.scoreboard_mut().set_render(&name, hearts, region) {
        let formatted = objective_name(global, &name)?;
        ok(
            region,
            id,
            "commands.scoreboard.objectives.modify.rendertype",
            args([formatted.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn objectives_autoupdate(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = goal(global, parsed, "objective")?;
    let enabled = parsed
        .get("value")
        .and_then(Value::as_bool)
        .ok_or_else(|| fail("command.failed"))?;
    if global
        .scoreboard_mut()
        .set_auto_update(&name, enabled, region)
    {
        let formatted = objective_name(global, &name)?;
        let key = if enabled {
            "commands.scoreboard.objectives.modify.displayAutoUpdate.enable"
        } else {
            "commands.scoreboard.objectives.modify.displayAutoUpdate.disable"
        };
        ok(region, id, key, args([name.into(), formatted.into()]), true);
    }
    Ok(())
}

pub(crate) fn objectives_numberformat(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    format: Option<NumberFormat>,
) -> Result<(), CommandFail> {
    let name = goal(global, parsed, "objective")?;
    let set = format.is_some();
    global
        .scoreboard_mut()
        .set_objective_format(&name, format, region);
    let key = if set {
        "commands.scoreboard.objectives.modify.objectiveFormat.set"
    } else {
        "commands.scoreboard.objectives.modify.objectiveFormat.clear"
    };
    ok(region, id, key, args([name.into()]), true);
    Ok(())
}

pub(crate) fn objectives_format_blank(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    objectives_numberformat(global, region, id, parsed, Some(NumberFormat::Blank))
}

pub(crate) fn objectives_format_styled(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let format = styled(parsed)?;
    objectives_numberformat(global, region, id, parsed, Some(format))
}

pub(crate) fn objectives_format_fixed(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let format = fixed(parsed)?;
    objectives_numberformat(global, region, id, parsed, Some(format))
}

pub(crate) fn objectives_format_default(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    objectives_numberformat(global, region, id, parsed, None)
}

// ---------------------------------------------------------------------------
// players
// ---------------------------------------------------------------------------

pub(crate) fn players_list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    _parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let names = global.scoreboard().tracked_players();
    if names.is_empty() {
        ok(
            region,
            id,
            "commands.scoreboard.players.list.empty",
            [],
            false,
        );
    } else {
        let list = crate::scoreboard::format_green_names(&names);
        ok(
            region,
            id,
            "commands.scoreboard.players.list.success",
            args([(names.len() as i32).into(), list.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn players_list_scores(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let holder = holders(global, region, id, parsed, "target")?;
    let name = holder.first_name().to_owned();
    let display = display_for(global, &name);
    let lines = global.scoreboard().player_score_lines(&name);
    if lines.is_empty() {
        ok(
            region,
            id,
            "commands.scoreboard.players.list.entity.empty",
            args([display.into()]),
            false,
        );
    } else {
        ok(
            region,
            id,
            "commands.scoreboard.players.list.entity.success",
            args([display.into(), (lines.len() as i32).into()]),
            false,
        );
        for (objective, value) in lines {
            ok(
                region,
                id,
                "commands.scoreboard.players.list.entity.entry",
                args([objective.into(), value.into()]),
                false,
            );
        }
    }
    Ok(())
}

pub(crate) fn players_get(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let holder = holders(global, region, id, parsed, "target")?;
    let name = holder.first_name().to_owned();
    let objective = goal(global, parsed, "objective")?;
    let formatted = objective_name(global, &objective)?;
    let Some(value) = global.scoreboard().score_value(&name, &objective) else {
        return Err(fail_with(
            "commands.scoreboard.players.get.null",
            args([objective.into(), display_for(global, &name).into()]),
        ));
    };
    ok(
        region,
        id,
        "commands.scoreboard.players.get.success",
        args([
            display_for(global, &name).into(),
            value.into(),
            formatted.into(),
        ]),
        false,
    );
    Ok(())
}

fn amount(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))
}

/// 一次 set/add/remove 的动作参数。`shown` 是反馈里的数字（remove 也是正数），
/// `delta` 是真正加到分数上的值。
///
/// Parameters for one set/add/remove action. `shown` is the number in the feedback
/// (positive even for remove), `delta` is what is added to the score.
struct ScoreAction {
    single: &'static str,
    multiple: &'static str,
    shown: i32,
    delta: i32,
    add: bool,
}

impl ScoreAction {
    fn set(value: i32) -> Self {
        Self {
            single: "commands.scoreboard.players.set.success.single",
            multiple: "commands.scoreboard.players.set.success.multiple",
            shown: value,
            delta: value,
            add: false,
        }
    }

    fn add(amount: i32) -> Self {
        Self {
            single: "commands.scoreboard.players.add.success.single",
            multiple: "commands.scoreboard.players.add.success.multiple",
            shown: amount,
            delta: amount,
            add: true,
        }
    }

    fn remove(amount: i32) -> Self {
        Self {
            single: "commands.scoreboard.players.remove.success.single",
            multiple: "commands.scoreboard.players.remove.success.multiple",
            shown: amount,
            delta: amount.wrapping_neg(),
            add: true,
        }
    }

    /// 官方 `ScoreboardCommand` 的 single/multiple 反馈参数顺序：
    /// set 是 (目标, [名字], 值)，add/remove 是 (值, 目标, [名字], 新分数) / (值, 目标, 数量)。
    ///
    /// Official `ScoreboardCommand` feedback argument order: set is
    /// (objective, [holder], value), add/remove is
    /// (amount, objective, [holder], new score) / (amount, objective, count).
    fn feedback(
        &self,
        formatted: Component,
        holder: Component,
        new_value: i32,
        count: i32,
        one: bool,
    ) -> Vec<TranslateArg> {
        if self.add {
            if one {
                vec![
                    self.shown.into(),
                    formatted.into(),
                    holder.into(),
                    new_value.into(),
                ]
            } else {
                vec![self.shown.into(), formatted.into(), count.into()]
            }
        } else if one {
            vec![formatted.into(), holder.into(), self.shown.into()]
        } else {
            vec![formatted.into(), count.into(), self.shown.into()]
        }
    }
}

fn write_score(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    action: ScoreAction,
) -> Result<(), CommandFail> {
    let objective = writable(global, parsed, "objective")?;
    let holders = holders(global, region, id, parsed, "targets")?;
    let formatted = objective_name(global, &objective)?;
    let count = holders.len() as i32;
    let mut total_value: i32 = 0;
    for name in &holders.names {
        let holder = holder_of(global, name);
        let value = if action.add {
            global
                .scoreboard_mut()
                .add_score(&holder, &objective, action.delta, region)
        } else {
            global
                .scoreboard_mut()
                .set_score(&holder, &objective, action.delta, region);
            action.delta
        };
        total_value = total_value.wrapping_add(value);
    }
    // 单目标时官方用 `totalValue`（这里就是那一个新分数），多目标时用数量。
    //
    // Official single-target feedback uses `totalValue` (here just the one new score),
    // while multiple-target feedback uses the count.
    let one = holders.len() == 1;
    let values = action.feedback(
        formatted,
        display_for(global, holders.first_name()),
        total_value,
        count,
        one,
    );
    ok(
        region,
        id,
        if one { action.single } else { action.multiple },
        values,
        true,
    );
    Ok(())
}

pub(crate) fn players_set(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let value = amount(parsed, "score")?;
    write_score(global, region, id, parsed, ScoreAction::set(value))
}

pub(crate) fn players_add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let value = amount(parsed, "score")?;
    write_score(global, region, id, parsed, ScoreAction::add(value))
}

pub(crate) fn players_remove(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let value = amount(parsed, "score")?;
    write_score(global, region, id, parsed, ScoreAction::remove(value))
}

pub(crate) fn players_reset(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let holders = holders(global, region, id, parsed, "targets")?;
    let one = holders.len() == 1;
    let first = display_for(global, holders.first_name());
    if parsed.get("objective").is_some() {
        let objective = goal(global, parsed, "objective")?;
        let formatted = objective_name(global, &objective)?;
        for name in &holders.names {
            global
                .scoreboard_mut()
                .reset_score(name, &objective, region);
        }
        if one {
            ok(
                region,
                id,
                "commands.scoreboard.players.reset.specific.single",
                args([formatted.into(), first.into()]),
                true,
            );
        } else {
            ok(
                region,
                id,
                "commands.scoreboard.players.reset.specific.multiple",
                args([formatted.into(), (holders.len() as i32).into()]),
                true,
            );
        }
    } else {
        for name in &holders.names {
            global.scoreboard_mut().reset_player(name, region);
        }
        if one {
            ok(
                region,
                id,
                "commands.scoreboard.players.reset.all.single",
                args([first.into()]),
                true,
            );
        } else {
            ok(
                region,
                id,
                "commands.scoreboard.players.reset.all.multiple",
                args([(holders.len() as i32).into()]),
                true,
            );
        }
    }
    Ok(())
}

pub(crate) fn players_enable(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let objective = goal(global, parsed, "objective")?;
    if !global.scoreboard().is_trigger(&objective) {
        return Err(fail("commands.scoreboard.players.enable.invalid"));
    }
    let formatted = objective_name(global, &objective)?;
    let holders = holders(global, region, id, parsed, "targets")?;
    let mut response = Response::new();
    for name in &holders.names {
        let holder = holder_of(global, name);
        let changed = global.scoreboard_mut().unlock(&holder, &objective, region);
        response.track(i32::from(changed));
    }
    if response.nonzero == 0 {
        return Err(fail("commands.scoreboard.players.enable.failed"));
    }
    if response.nonzero == 1 {
        let name = holders
            .names
            .iter()
            .find(|name| global.scoreboard().score_locked(name, &objective) == Some(false))
            .cloned()
            .unwrap_or_else(|| holders.first_name().to_owned());
        ok(
            region,
            id,
            "commands.scoreboard.players.enable.success.single",
            args([formatted.into(), display_for(global, &name).into()]),
            true,
        );
    } else {
        ok(
            region,
            id,
            "commands.scoreboard.players.enable.success.multiple",
            args([formatted.into(), (response.nonzero as i32).into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn players_operation(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let target_objective = writable(global, parsed, "targetObjective")?;
    let source_objective = goal(global, parsed, "sourceObjective")?;
    let operation = parsed
        .get("operation")
        .and_then(Value::as_operation)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = holders(global, region, id, parsed, "targets")?;
    let sources = holders(global, region, id, parsed, "source")?;
    let formatted = objective_name(global, &target_objective)?;
    let mut response = Response::new();
    for target in &targets.names {
        let target_holder = holder_of(global, target);
        for source in &sources.names {
            let source_holder = holder_of(global, source);
            match global.scoreboard_mut().operate(
                &target_holder,
                &target_objective,
                operation,
                &source_holder,
                &source_objective,
                region,
            ) {
                Ok(_) => {}
                Err(ScoreFailure::ReadOnly) => {
                    return Err(fail_with(
                        "arguments.objective.readonly",
                        args([target_objective.into()]),
                    ));
                }
                Err(ScoreFailure::DivideByZero) => {
                    return Err(fail("arguments.operation.div0"));
                }
                Err(ScoreFailure::Overflow) => return Err(fail("command.failed")),
            }
        }
        let value = global
            .scoreboard()
            .score_value(target, &target_objective)
            .unwrap_or(0);
        response.track(value);
    }
    if response.count == 1 {
        ok(
            region,
            id,
            "commands.scoreboard.players.operation.success.single",
            args([
                formatted.into(),
                display_for(global, targets.first_name()).into(),
                response.total.into(),
            ]),
            true,
        );
    } else {
        ok(
            region,
            id,
            "commands.scoreboard.players.operation.success.multiple",
            args([formatted.into(), (response.count as i32).into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn players_display_name(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let objective = goal(global, parsed, "objective")?;
    let holders = holders(global, region, id, parsed, "targets")?;
    let formatted = objective_name(global, &objective)?;
    let is_clear = parsed.get("name").is_none();
    let display = if is_clear {
        None
    } else {
        Some(component(parsed, "name")?)
    };
    for name in &holders.names {
        let holder = holder_of(global, name);
        global
            .scoreboard_mut()
            .set_score_display(&holder, &objective, display.clone(), region);
    }
    let one = holders.len() == 1;
    if let Some(display) = display {
        if one {
            ok(
                region,
                id,
                "commands.scoreboard.players.display.name.set.success.single",
                args([
                    display.into(),
                    display_for(global, holders.first_name()).into(),
                    formatted.into(),
                ]),
                true,
            );
        } else {
            ok(
                region,
                id,
                "commands.scoreboard.players.display.name.set.success.multiple",
                args([
                    display.into(),
                    (holders.len() as i32).into(),
                    formatted.into(),
                ]),
                true,
            );
        }
    } else if one {
        ok(
            region,
            id,
            "commands.scoreboard.players.display.name.clear.success.single",
            args([
                display_for(global, holders.first_name()).into(),
                formatted.into(),
            ]),
            true,
        );
    } else {
        ok(
            region,
            id,
            "commands.scoreboard.players.display.name.clear.success.multiple",
            args([(holders.len() as i32).into(), formatted.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn players_display_clear(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    players_display_name(global, region, id, parsed)
}

pub(crate) fn players_display_numberformat(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    format: Option<NumberFormat>,
) -> Result<(), CommandFail> {
    let objective = goal(global, parsed, "objective")?;
    let holders = holders(global, region, id, parsed, "targets")?;
    let formatted = objective_name(global, &objective)?;
    for name in &holders.names {
        let holder = holder_of(global, name);
        global
            .scoreboard_mut()
            .set_score_format(&holder, &objective, format.clone(), region);
    }
    let set = format.is_some();
    let one = holders.len() == 1;
    let key = match (set, one) {
        (true, true) => "commands.scoreboard.players.display.numberFormat.set.success.single",
        (true, false) => "commands.scoreboard.players.display.numberFormat.set.success.multiple",
        (false, true) => "commands.scoreboard.players.display.numberFormat.clear.success.single",
        (false, false) => "commands.scoreboard.players.display.numberFormat.clear.success.multiple",
    };
    let values = if one {
        vec![
            display_for(global, holders.first_name()).into(),
            formatted.into(),
        ]
    } else {
        vec![(holders.len() as i32).into(), formatted.into()]
    };
    ok(region, id, key, values, true);
    Ok(())
}

pub(crate) fn players_format_clear(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    players_display_numberformat(global, region, id, parsed, None)
}

pub(crate) fn players_format_blank(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    players_display_numberformat(global, region, id, parsed, Some(NumberFormat::Blank))
}

pub(crate) fn players_format_styled(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let format = styled(parsed)?;
    players_display_numberformat(global, region, id, parsed, Some(format))
}

pub(crate) fn players_format_fixed(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let format = fixed(parsed)?;
    players_display_numberformat(global, region, id, parsed, Some(format))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::packets::play::clientbound;

    use crate::{
        command::commands,
        world::tests::{Harness, frames_with_id},
    };

    /// 没有 `message` 参数，签名检查不会拦下命令。
    ///
    /// There is no `message` argument, so signed-command checks do not block these.
    fn run(server: &mut Harness, client: &crate::world::tests::Client, text: &str) {
        server.request(
            client,
            mc_bamboo_protocol::packet::play::PlayRequest::ChatCommand {
                command: text.to_owned(),
                signed: None,
            },
        );
        server.tick();
    }

    fn operator(name: &str) -> (Harness, crate::world::tests::Client) {
        let mut server = Harness::new(2);
        let client = server.join_and_settle(name);
        server
            .region()
            .players
            .get_mut(&client.id)
            .expect("玩家应该在世界里")
            .permission_level = 2;
        client.take_frames();
        (server, client)
    }

    fn system_text(client: &crate::world::tests::Client) -> Vec<Vec<u8>> {
        frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT)
            .into_iter()
            .map(|frame| frame.body.clone())
            .collect()
    }

    fn has_key(bodies: &[Vec<u8>], key: &str) -> bool {
        bodies.iter().any(|body| {
            body.windows(key.len())
                .any(|window| window == key.as_bytes())
        })
    }

    #[test]
    fn command_tree_matches_the_official_shapes() {
        let graph = &commands().graph;
        for text in [
            "scoreboard objectives list",
            "scoreboard objectives add kills dummy",
            "scoreboard objectives add kills dummy {text:\"Kills\"}",
            "scoreboard objectives remove kills",
            "scoreboard objectives setdisplay sidebar",
            "scoreboard objectives setdisplay sidebar kills",
            "scoreboard objectives modify kills displayname {text:\"K\"}",
            "scoreboard objectives modify kills rendertype hearts",
            "scoreboard objectives modify kills displayautoupdate true",
            "scoreboard objectives modify kills numberformat",
            "scoreboard objectives modify kills numberformat blank",
            "scoreboard objectives modify kills numberformat styled {color:\"red\"}",
            "scoreboard objectives modify kills numberformat fixed {text:\"x\"}",
            "scoreboard players list",
            "scoreboard players list Kitten",
            "scoreboard players get Kitten kills",
            "scoreboard players set Kitten kills 4",
            "scoreboard players add Kitten kills 2",
            "scoreboard players remove Kitten kills 2",
            "scoreboard players reset Kitten",
            "scoreboard players reset Kitten kills",
            "scoreboard players enable Kitten triggers",
            "scoreboard players operation Kitten kills += Other kills",
            "scoreboard players display name Kitten kills {text:\"K\"}",
            "scoreboard players display numberformat Kitten kills blank",
        ] {
            assert!(graph.parse(text, 2).is_ok(), "{text}");
            assert!(graph.parse(text, 1).is_err(), "{text} at level 1");
        }
    }

    #[test]
    fn objectives_add_and_list_report_the_official_keys() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        let bodies = system_text(&client);
        assert!(has_key(
            &bodies,
            "commands.scoreboard.objectives.add.success"
        ));
        assert_eq!(server.global().scoreboard().objective_count(), 1);

        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.objectives.add.duplicate"
        ));

        run(&mut server, &client, "scoreboard objectives list");
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.objectives.list.success"
        ));
    }

    #[test]
    fn display_tracking_sends_the_official_packet_order() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        run(
            &mut server,
            &client,
            "scoreboard players set Kitten kills 7",
        );
        client.take_frames();

        run(
            &mut server,
            &client,
            "scoreboard objectives setdisplay sidebar kills",
        );
        let frames = client.take_frames();
        let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
        // 官方 `startTrackingObjective` 的顺序：set_objective、set_display_objective、set_score。
        let start = ids
            .iter()
            .position(|id| *id == clientbound::SET_OBJECTIVE)
            .expect("开始跟踪时先发目标");
        assert_eq!(
            &ids[start..start + 3],
            &[
                clientbound::SET_OBJECTIVE,
                clientbound::SET_DISPLAY_OBJECTIVE,
                clientbound::SET_SCORE,
            ]
        );
        assert!(frames.iter().any(|frame| frame.id == clientbound::SET_SCORE
            && frame.body.windows(6).any(|w| w == b"Kitten")));
    }

    #[test]
    fn set_and_get_report_the_value_and_read_only_criteria_fail() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        run(
            &mut server,
            &client,
            "scoreboard players set Kitten kills 5",
        );
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.players.set.success.single"
        ));
        run(&mut server, &client, "scoreboard players get Kitten kills");
        let bodies = system_text(&client);
        assert!(has_key(&bodies, "commands.scoreboard.players.get.success"));

        run(&mut server, &client, "scoreboard objectives add hp health");
        run(&mut server, &client, "scoreboard players set Kitten hp 3");
        assert!(has_key(
            &system_text(&client),
            "arguments.objective.readonly"
        ));
    }

    #[test]
    fn enable_requires_a_trigger_and_rejects_an_enabled_one() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, "scoreboard objectives add t trigger");
        run(&mut server, &client, "scoreboard players set Kitten t 1");
        run(&mut server, &client, "scoreboard players enable Kitten t");
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.players.enable.success.single"
        ));
        run(&mut server, &client, "scoreboard players enable Kitten t");
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.players.enable.failed"
        ));
        run(
            &mut server,
            &client,
            "scoreboard players enable Kitten kills",
        );
        assert!(has_key(
            &system_text(&client),
            "arguments.objective.notFound"
        ));
    }

    #[test]
    fn join_replays_tracked_objectives_and_teams() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        run(
            &mut server,
            &client,
            "scoreboard players set Kitten kills 7",
        );
        run(
            &mut server,
            &client,
            "scoreboard objectives setdisplay sidebar kills",
        );
        client.take_frames();

        let newcomer = server.join("Alex");
        server.tick();
        let frames = newcomer.take_frames();
        let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
        assert!(ids.contains(&clientbound::SET_OBJECTIVE));
        assert!(ids.contains(&clientbound::SET_DISPLAY_OBJECTIVE));
        assert!(ids.contains(&clientbound::SET_SCORE));
    }

    #[test]
    fn display_and_number_format_commands_change_state() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        run(
            &mut server,
            &client,
            r#"scoreboard objectives modify kills displayname {text:"K"}"#,
        );
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.objectives.modify.displayname"
        ));
        run(
            &mut server,
            &client,
            "scoreboard objectives modify kills rendertype hearts",
        );
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.objectives.modify.rendertype"
        ));
        run(
            &mut server,
            &client,
            "scoreboard players display numberformat Kitten kills blank",
        );
        assert!(has_key(
            &system_text(&client),
            "commands.scoreboard.players.display.numberFormat.set.success.single"
        ));
    }

    /// 官方 `ServerPlayer.doTick`：六个只读准则只在进服后第一刻写一次。
    ///
    /// Official `ServerPlayer.doTick`: the six read-only criteria are written once after join.
    #[test]
    fn read_only_criteria_are_written_once_on_the_first_tick() {
        let (mut server, client) = operator("Op");
        for (name, criteria, expected) in [
            ("hp", "health", 20),
            ("food", "food", 20),
            ("air", "air", 300),
            ("armor", "armor", 0),
            ("xp", "xp", 0),
            ("level", "level", 0),
        ] {
            run(
                &mut server,
                &client,
                &format!("scoreboard objectives add {name} {criteria}"),
            );
            // 玩家已经进服，第一刻早已过去，后来创建的目标不会补写。
            //
            // The player has already joined, so a later objective gets no write.
            assert_eq!(
                server.global().scoreboard().score_value("Op", name),
                None,
                "{criteria}"
            );
            let rejoin_name = format!("Join{criteria}");
            let newcomer = server.join(&rejoin_name);
            server.tick();
            assert_eq!(
                server.global().scoreboard().score_value(&rejoin_name, name),
                Some(expected),
                "{criteria}"
            );
            newcomer.take_frames();
        }
    }

    #[test]
    fn read_only_criteria_do_not_reappear_after_reset_or_a_new_objective() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add level level",
        );
        // 新建目标不补写。
        assert_eq!(
            server.global().scoreboard().score_value("Op", "level"),
            None
        );
        let newcomer = server.join("Newcomer");
        server.tick();
        assert_eq!(
            server
                .global()
                .scoreboard()
                .score_value("Newcomer", "level"),
            Some(0)
        );
        newcomer.take_frames();
        run(
            &mut server,
            &client,
            "scoreboard players reset Newcomer level",
        );
        server.tick();
        assert_eq!(
            server
                .global()
                .scoreboard()
                .score_value("Newcomer", "level"),
            None,
            "reset 之后不会写回来"
        );
        // 只读准则不能被命令改写。
        run(
            &mut server,
            &client,
            "scoreboard players set Newcomer level 1",
        );
        assert!(has_key(
            &system_text(&client),
            "arguments.objective.readonly"
        ));
    }

    /// 官方 `Scoreboard.entityRemoved` 对 `Player` 直接返回：离线不广播 reset_score。
    ///
    /// Official `Scoreboard.entityRemoved` returns for players: no reset_score on disconnect.
    #[test]
    fn leaving_does_not_clear_scores_or_broadcast_reset_score() {
        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        run(
            &mut server,
            &client,
            "scoreboard players set Kitten kills 7",
        );
        run(
            &mut server,
            &client,
            "scoreboard objectives setdisplay sidebar kills",
        );
        client.take_frames();
        server.leave(&client);
        server.tick();
        assert_eq!(
            server.global().scoreboard().score_value("Kitten", "kills"),
            Some(7)
        );
        // `client` 的链路已移除，用一个旁观者确认没有任何广播包。
        let watcher = server.join("Watcher");
        server.tick();
        watcher.take_frames();
        server.leave(&watcher);
        server.tick();
        assert!(frames_with_id(&client.take_frames(), clientbound::RESET_SCORE).is_empty());
    }

    /// 官方 `ScoreboardCommand` 的反馈参数顺序：add/remove 先写数量，set 先写目标。
    ///
    /// Official `ScoreboardCommand` feedback argument order: add/remove puts the
    /// amount first, set puts the objective first.
    #[test]
    fn score_feedback_arguments_follow_the_official_order() {
        use mc_bamboo_nbt::Tag;
        use mc_bamboo_protocol::components::cursor::Cursor;

        fn with_values(body: &[u8]) -> Vec<Tag> {
            let tag = Cursor::new(body).nbt_tag().expect("系统消息是合法组件");
            let Tag::Compound(entries) = tag else {
                panic!("系统消息是复合标签");
            };
            let Some((_, Tag::List(values))) = entries.iter().find(|(key, _)| key == "with") else {
                panic!("翻译组件没有 with");
            };
            values.clone()
        }

        fn number_text(tag: &Tag, expected: i32) -> bool {
            match tag {
                Tag::Int(value) => *value == expected,
                Tag::Compound(entries) => entries.iter().any(|(key, value)| {
                    key == "text"
                        && matches!(value, Tag::String(text) if text == &expected.to_string())
                }),
                _ => false,
            }
        }

        let (mut server, client) = operator("Op");
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        client.take_frames();
        run(
            &mut server,
            &client,
            "scoreboard players set Kitten kills 5",
        );
        let bodies = system_text(&client);
        let values = with_values(&bodies[0]);
        // set.single = (目标, 名字, 值)；目标是方括号组件，值是整数或文本组件。
        assert_eq!(values.len(), 3);
        assert!(matches!(values[2], Tag::Int(5) | Tag::Compound(_)));
        assert!(matches!(values[0], Tag::Compound(_)), "目标在最前");

        client.take_frames();
        run(
            &mut server,
            &client,
            "scoreboard players add Kitten kills 3",
        );
        let bodies = system_text(&client);
        let values = with_values(&bodies[0]);
        // add.single = (数量, 目标, 名字, 新分数)。
        assert_eq!(values.len(), 4);
        // 混类型时 `unify_elements` 把整数包成 {text:"3"}。
        assert!(number_text(&values[0], 3), "数量在最先");
        assert!(matches!(values[1], Tag::Compound(_)), "目标第二");
        assert!(number_text(&values[3], 8), "新分数在最后");

        client.take_frames();
        run(
            &mut server,
            &client,
            "scoreboard players remove Kitten kills 2",
        );
        let bodies = system_text(&client);
        let values = with_values(&bodies[0]);
        assert_eq!(values.len(), 4);
        assert!(number_text(&values[0], 2), "remove 的数量是正数");
        assert!(number_text(&values[3], 6));
        assert_eq!(
            server.global().scoreboard().score_value("Kitten", "kills"),
            Some(6)
        );
    }

    #[test]
    fn feedback_respects_send_command_feedback() {
        let (mut server, client) = operator("Op");
        server.region().send_command_feedback = false;
        run(
            &mut server,
            &client,
            "scoreboard objectives add kills dummy",
        );
        assert!(system_text(&client).is_empty());
        assert_eq!(server.global().scoreboard().objective_count(), 1);
    }
}
