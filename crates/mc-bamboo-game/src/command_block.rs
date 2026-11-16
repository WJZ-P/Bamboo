//! 命令方块的放置、打开和保存。
//!
//! 执行、连锁和计划刻都不做。记录只活在内存里，重启就没了。
//!
//! Placing, opening and saving command blocks.
//!
//! Execution, chains and scheduled ticks are not implemented. The record lives only in
//! memory and disappears on restart.

use std::collections::BTreeMap;

use mc_bamboo_blocks::{BlockState, World, world::UPDATE_CLIENTS};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, registries::block_entity_type};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    packet::play::{BlockEntityData, CommandBlockMode},
    text::Component,
};

use crate::{inbound::PlayerId, region::Region};

/// `BaseCommandBlock` 加上 `CommandBlockEntity` 里我们保存的字段。
///
/// The `BaseCommandBlock` fields plus the `CommandBlockEntity` fields we store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBlockData {
    pub(crate) command: String,
    pub(crate) track_output: bool,
    pub(crate) automatic: bool,
    success_count: i32,
    powered: bool,
    condition_met: bool,
    update_last_execution: bool,
}

impl CommandBlockData {
    /// 存档解码用：按官方缺省值新建一条命令方块记录。
    pub fn from_archive(data: ArchiveCommandBlock) -> Self {
        Self {
            command: data.command,
            track_output: data.track_output,
            automatic: data.automatic,
            success_count: data.success_count,
            powered: data.powered,
            condition_met: data.condition_met,
            update_last_execution: data.update_last_execution,
        }
    }

    /// 存档编码用：全部字段的只读快照。
    pub fn archive(&self) -> ArchiveCommandBlock {
        ArchiveCommandBlock {
            command: self.command.clone(),
            track_output: self.track_output,
            automatic: self.automatic,
            success_count: self.success_count,
            powered: self.powered,
            condition_met: self.condition_met,
            update_last_execution: self.update_last_execution,
        }
    }

    /// 方块实体刚创建时的字段。`trackOutput` 默认真，重复型的 `automatic` 是真。
    ///
    /// Fields of a freshly created block entity. `trackOutput` defaults to true, and a
    /// repeating block's `automatic` is true.
    fn fresh(state: BlockState) -> Self {
        Self {
            command: String::new(),
            track_output: true,
            automatic: repeats(state),
            success_count: 0,
            powered: false,
            condition_met: false,
            update_last_execution: true,
        }
    }

    /// `saveCustomOnly`：`BaseCommandBlock.save` 再加三个布尔。空的自定义名和上次输出不写。
    ///
    /// `saveCustomOnly`: `BaseCommandBlock.save` plus the three booleans. An absent custom
    /// name and last output are omitted.
    fn network_tag(&self) -> Vec<u8> {
        let tag = Tag::Compound(vec![
            ("Command".to_owned(), Tag::String(self.command.clone())),
            ("SuccessCount".to_owned(), Tag::Int(self.success_count)),
            ("TrackOutput".to_owned(), Tag::Byte(byte(self.track_output))),
            (
                "UpdateLastExecution".to_owned(),
                Tag::Byte(byte(self.update_last_execution)),
            ),
            ("powered".to_owned(), Tag::Byte(byte(self.powered))),
            (
                "conditionMet".to_owned(),
                Tag::Byte(byte(self.condition_met)),
            ),
            ("auto".to_owned(), Tag::Byte(byte(self.automatic))),
        ]);
        let mut bytes = Vec::new();
        tag.write_network(&mut bytes);
        bytes
    }
}

/// `CommandBlockData` 的存档快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandBlock {
    pub command: String,
    pub track_output: bool,
    pub automatic: bool,
    pub success_count: i32,
    pub powered: bool,
    pub condition_met: bool,
    pub update_last_execution: bool,
}

fn byte(value: bool) -> i8 {
    i8::from(value)
}

/// 三种命令方块。
///
/// The three command blocks.
pub(crate) fn is_command_block(state: BlockState) -> bool {
    state.is(blocks::COMMAND_BLOCK)
        || state.is(blocks::REPEATING_COMMAND_BLOCK)
        || state.is(blocks::CHAIN_COMMAND_BLOCK)
}

fn repeats(state: BlockState) -> bool {
    state.is(blocks::REPEATING_COMMAND_BLOCK)
}

/// `CommandBlock.setPlacedBy`：物品没有 `block_entity_data` 时，`trackOutput` 取
/// `send_command_feedback`，`automatic` 取方块构造参数。
///
/// `CommandBlock.setPlacedBy`: without `block_entity_data`, `trackOutput` comes from
/// `send_command_feedback` and `automatic` comes from the block's constructor argument.
pub(crate) fn on_placed(region: &mut Region, pos: BlockPos) {
    let Some(state) = state_at(region, pos) else {
        return;
    };
    if !is_command_block(state) {
        return;
    }
    let track_output = region.send_command_feedback;
    region
        .command_blocks
        .entry(pos)
        .or_insert_with(|| CommandBlockData {
            track_output,
            ..CommandBlockData::fresh(state)
        });
}

/// `ServerPlayer.openCommandBlock`：发 `block_entity_data`。界面由客户端自己打开。
///
/// `ServerPlayer.openCommandBlock` sends `block_entity_data`. The client opens the screen.
pub(crate) fn open(region: &mut Region, id: PlayerId, pos: BlockPos) {
    let Some(state) = state_at(region, pos) else {
        return;
    };
    if !is_command_block(state) {
        return;
    }
    let bytes = region
        .command_blocks
        .entry(pos)
        .or_insert_with(|| CommandBlockData::fresh(state))
        .network_tag();
    region.links.send(
        id,
        &BlockEntityData {
            pos,
            kind: block_entity_type::COMMAND_BLOCK,
            data: &bytes,
        },
    );
}

/// `set_command_block` 里要写回去的字段。
///
/// The fields `set_command_block` writes back.
pub(crate) struct CommandBlockEdit {
    pub(crate) pos: BlockPos,
    pub(crate) command: String,
    pub(crate) mode: CommandBlockMode,
    pub(crate) track_output: bool,
    pub(crate) conditional: bool,
    pub(crate) automatic: bool,
}

/// `handleSetCommandBlock`。没有权限时发 `advMode.notAllowed`。不执行命令。
///
/// `handleSetCommandBlock`. Without permission this sends `advMode.notAllowed`.
/// The command is not executed.
pub(crate) fn on_set(region: &mut Region, id: PlayerId, edit: CommandBlockEdit) {
    let CommandBlockEdit {
        pos,
        command,
        mode,
        track_output,
        conditional,
        automatic,
    } = edit;
    let Some(allowed) = region.players.get(&id).map(player_can_use) else {
        return;
    };
    // `canUseGameMasterBlocks`，不是单独的权限等级。
    //
    // `canUseGameMasterBlocks`, not the permission level on its own.
    if !allowed {
        let content = Component::translate("advMode.notAllowed", []);
        // 官方 `handleSetCommandBlock` 走 `player.sendSystemMessage`。
        //
        // Official `handleSetCommandBlock` goes through `player.sendSystemMessage`.
        crate::chat::send_system_message(region, id, &content, false);
        return;
    }
    let Some(current) = state_at(region, pos) else {
        return;
    };
    if !is_command_block(current) {
        return;
    }
    let facing = current.property_text("facing").unwrap_or("north");
    let next = state_for(mode, facing, conditional);
    {
        let record = region
            .command_blocks
            .entry(pos)
            .or_insert_with(|| CommandBlockData::fresh(current));
        record.command = command.clone();
        record.track_output = track_output;
        record.automatic = automatic;
        record.success_count = 0;
    }
    if next != current {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        // 官方 `setBlock(..., 2)`，也就是 `UPDATE_CLIENTS`。
        //
        // Official `setBlock(..., 2)`, which is `UPDATE_CLIENTS`.
        world.set_block(pos, next, UPDATE_CLIENTS);
    }
    if command.is_empty() {
        return;
    }
    // `ServerLevel.isCommandBlockEnabled` 在 26.3 只看 `command_blocks_work`。
    //
    // `ServerLevel.isCommandBlockEnabled` in 26.3 only reads `command_blocks_work`.
    let key = if region.command_blocks_work {
        "advMode.setCommand.success"
    } else {
        "advMode.setCommand.disabled"
    };
    let content = Component::translate(key, [command.into()]);
    // 官方 `handleSetCommandBlock` 走 `player.sendSystemMessage`。
    //
    // Official `handleSetCommandBlock` goes through `player.sendSystemMessage`.
    crate::chat::send_system_message(region, id, &content, false);
}

fn state_for(mode: CommandBlockMode, facing: &str, conditional: bool) -> BlockState {
    let base = match mode {
        CommandBlockMode::Sequence => blocks::CHAIN_COMMAND_BLOCK,
        CommandBlockMode::Auto => blocks::REPEATING_COMMAND_BLOCK,
        CommandBlockMode::Redstone => blocks::COMMAND_BLOCK,
    };
    let state = BlockState(base);
    let state = state.try_with_text("facing", facing).unwrap_or(state);
    let flag = if conditional { "true" } else { "false" };
    state.try_with_text("conditional", flag).unwrap_or(state)
}

fn state_at(region: &Region, pos: BlockPos) -> Option<BlockState> {
    region.chunks.block(pos).map(BlockState)
}

fn player_can_use(player: &crate::player::Player) -> bool {
    player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative
        && player.permission_level >= mc_bamboo_command::permission::GAMEMASTERS
}

/// 换成别的方块时丢掉记录。三种命令方块之间切换要留着。
///
/// Drop the record when the block becomes something else. Switching among the three
/// command blocks keeps it.
pub(crate) fn forget_if_replaced(
    command_blocks: &mut BTreeMap<BlockPos, CommandBlockData>,
    pos: BlockPos,
    state: BlockState,
) {
    if !is_command_block(state) {
        command_blocks.remove(&pos);
    }
}
