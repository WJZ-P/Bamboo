//! `/playsound`：给指定玩家播放一个音效。
//!
//! 官方 `PlaySoundCommand.playSound` 用 `SoundEvent.createVariableRangeEvent(id)` 加
//! `Holder.direct` 构造内联音效，所以线上总是「ID 0 + 资源名 + 没有固定范围」，
//! 即使名字在 `sound_event` 注册表里也一样。范围按音量算：16 或 16×音量；
//! 玩家超出范围时，如果 `minVolume > 0`，把音源挪到玩家附近两格，音量换成 `minVolume`。

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_data::{SoundSource, sounds};
use mc_bamboo_protocol::{packet::play::Sound, text::Component};

use crate::{
    command::{
        CommandFn, effects,
        feedback::{CommandFail, fail},
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// `SoundSource.values()` 的顺序，命令树里的字面量顺序也照它。
pub(crate) const SOURCES: [(&str, SoundSource); 11] = [
    ("master", sounds::MASTER),
    ("music", sounds::MUSIC),
    ("record", sounds::RECORDS),
    ("weather", sounds::WEATHER),
    ("block", sounds::BLOCKS),
    ("hostile", sounds::HOSTILE),
    ("neutral", sounds::NEUTRAL),
    ("player", sounds::PLAYERS),
    ("ambient", sounds::AMBIENT),
    ("voice", sounds::VOICE),
    ("ui", sounds::UI),
];

/// `Component.translationArg(Identifier)` 在 NBT 里就是字面字符串。
fn sound_argument(name: &str) -> Component {
    Component::text(name)
}

pub(crate) fn run(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    source: SoundSource,
) -> Result<(), CommandFail> {
    let _ = global;
    let name = parsed
        .get("sound")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?
        .to_owned();
    let origin = effects::coordinates(
        region,
        id,
        parsed,
        "pos",
        effects::source_position(region, id),
    )?;
    let volume = optional_float(parsed, "volume", 1.0)?;
    let pitch = optional_float(parsed, "pitch", 1.0)?;
    let min_volume = optional_float(parsed, "minVolume", 0.0)?;
    let targets = match parsed.get("targets") {
        Some(_) => effects::targets(region, id, parsed)?,
        None => region
            .players
            .contains_key(&id)
            .then_some(vec![id])
            .unwrap_or_default(),
    };
    // 官方 `SoundEvent.getRange`：可变范围时 volume>1 是 16*volume，否则 16。
    let range = if volume > 1.0 { 16.0 * volume } else { 16.0 };
    let range_squared = f64::from(range * range);
    let seed = region.sound_seed_generator.source().next_long();
    let mut delivered = Vec::with_capacity(targets.len());
    for target in targets {
        let Some(player) = region.players.get(&target) else {
            continue;
        };
        let position = player.position();
        let dx = origin.x - position.x;
        let dy = origin.y - position.y;
        let dz = origin.z - position.z;
        let squared = dx * dx + dy * dy + dz * dz;
        let (x, y, z, heard_volume) = if squared > range_squared {
            if min_volume <= 0.0 {
                continue;
            }
            // 音源挪到玩家朝原音源方向两格处，音量换成 minVolume。
            let distance = squared.sqrt();
            (
                position.x + dx / distance * 2.0,
                position.y + dy / distance * 2.0,
                position.z + dz / distance * 2.0,
                min_volume,
            )
        } else {
            (origin.x, origin.y, origin.z, volume)
        };
        let packet = Sound {
            sound: 0,
            inline: Some(name.clone()),
            source,
            x,
            y,
            z,
            volume: heard_volume,
            pitch,
            seed,
        };
        region.links.send(target, &packet);
        delivered.push(target);
    }
    if delivered.is_empty() {
        return Err(fail("commands.playsound.failed"));
    }
    effects::report(
        region,
        id,
        &delivered,
        "commands.playsound.success.single",
        "commands.playsound.success.multiple",
        Some(sound_argument(&name)),
    );
    Ok(())
}

fn optional_float(parsed: &ParsedCommand, name: &str, default: f32) -> Result<f32, CommandFail> {
    match parsed.get(name) {
        Some(value) => value.as_float().ok_or_else(|| fail("command.failed")),
        None => Ok(default),
    }
}

/// 树里挂的自定义补全：`minecraft:available_sounds`。
pub(crate) const SUGGESTION: &str = "minecraft:available_sounds";

/// 官方补全来自 `SharedSuggestionProvider.getAvailableSounds()`，也就是 `sound_event` 注册表。
pub(crate) fn suggest_sounds(_arguments: &[(String, Value)], lower: &str, out: &mut Vec<String>) {
    for event in mc_bamboo_data::SOUND_EVENTS {
        mc_bamboo_command::argument::suggest_identifier(event.name, lower, out);
    }
}

/// 按 `SoundSource.values()` 排列的可执行函数，供命令树注册。
pub(crate) const ACTIONS: [CommandFn; 11] = [
    master, music, records, weather, blocks, hostile, neutral, players, ambient, voice, ui,
];

/// 每个来源字面量一个可执行函数。
macro_rules! source_fn {
    ($name:ident, $source:expr) => {
        pub(crate) fn $name(
            global: &mut Global,
            region: &mut Region,
            id: PlayerId,
            parsed: &ParsedCommand,
        ) -> Result<(), CommandFail> {
            run(global, region, id, parsed, $source)
        }
    };
}

source_fn!(master, sounds::MASTER);
source_fn!(music, sounds::MUSIC);
source_fn!(records, sounds::RECORDS);
source_fn!(weather, sounds::WEATHER);
source_fn!(blocks, sounds::BLOCKS);
source_fn!(hostile, sounds::HOSTILE);
source_fn!(neutral, sounds::NEUTRAL);
source_fn!(players, sounds::PLAYERS);
source_fn!(ambient, sounds::AMBIENT);
source_fn!(voice, sounds::VOICE);
source_fn!(ui, sounds::UI);

/// 只写了 `sound` 的那一层：官方用 `MASTER`，目标只有执行者。
pub(crate) fn sound_only(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    run(global, region, id, parsed, sounds::MASTER)
}
