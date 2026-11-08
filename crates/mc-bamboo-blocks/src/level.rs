//! 这个 crate 和世界存储的边界。

use std::collections::{HashMap, HashSet};

use mc_bamboo_core::{
    BlockPos, ChunkPos, LightLayer,
    sign::{SignContent, SignTextSlot},
};

use crate::{random::LevelRandom, shape::Aabb, state::BlockState};

/// 世界的存储和世界外的一切。`mc-bamboo-game` 和 oracle 各有一个实现。
pub trait Level {
    /// 世界外返回 `void_air`。
    fn block_state(&self, pos: BlockPos) -> BlockState;

    /// 写入一格，不做任何连锁。高度超出世界时什么也不做，返回 `false`。
    fn write_block(&mut self, pos: BlockPos, state: BlockState) -> bool;

    /// 这些盒子（世界坐标）和所有挡路的实体都没有正体积重叠。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool;

    /// 世界最低的一层，`getMinY`。
    fn min_y(&self) -> i32;

    /// 世界最高的一层（含），`getMaxY`。
    fn max_y(&self) -> i32;

    /// `Level.random` 和 `randValue`，方块逻辑的随机数都从这里取。
    fn random(&self) -> &LevelRandom;

    /// `LevelChunkSection.isRandomlyTicking`：这个区块里纵坐标是 `section_y`（方块的 `y >> 4`）的区块段里
    /// 有会随机刻的方块或者流体，也就是有状态的 `is_randomly_ticking` 或者 `is_fluid_randomly_ticking` 为真。
    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool;

    /// `ChunkMap.anyPlayerCloseEnoughTo`：有不是旁观者的玩家，脚到这一格最小的角不到 `max_distance` 格。
    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool;

    /// `getBrightness(layer, pos)`：这一格的天空光或者方块光，0 到 15。
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32;

    /// `getBlockEntity(pos) instanceof SignBlockEntity` 之后读到的内容。
    ///
    /// 还没写过字的告示牌返回 [`SignContent::BLANK`]，官方也是按状态新建一个空的方块实体。
    /// 不是告示牌方块时返回 `None`。
    fn sign(&self, pos: BlockPos) -> Option<SignContent>;

    /// 改写告示牌的内容，对应 `SignBlockEntity` 里那几个字段。不是告示牌方块时返回 `false`。
    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool;

    /// 红石用到的、方块状态装不下的世界数据。
    fn redstone(&self) -> &RedstoneMemory;

    /// 见 [`Level::redstone`]。
    fn redstone_mut(&mut self) -> &mut RedstoneMemory;
}

/// 红石用到的、方块状态装不下的世界数据，活得比一次操作长。
///
/// 官方把这些放在方块实体（比较器的输出、雕纹书架最近动过的格子、唱片机在不在放）
/// 和按世界分组的静态表（火把最近的开关）里。
#[derive(Debug, Default)]
pub struct RedstoneMemory {
    /// `ComparatorBlockEntity.outputSignal`。没有记录的格子是 0，和空的方块实体一样。
    comparator_outputs: HashMap<BlockPos, i32>,
    /// `ChiseledBookShelfBlockEntity.lastInteractedSlot`。没有记录的格子是 -1，也就是没动过。
    bookshelf_slots: HashMap<BlockPos, usize>,
    /// `JukeboxBlockEntity` 的 `songPlayer.isPlaying()`。唱片的内容不在范围里，只记哪几格在放。
    /// 世界生成时直接摆下的 `has_record=true` 唱机没有方块实体，也就没在放。
    jukebox_playing: HashSet<BlockPos>,
    /// `RedstoneTorchBlock.RECENT_TOGGLES` 里属于这个世界的那一项，按时间排好。
    torch_toggles: Vec<(BlockPos, i64)>,
}

impl RedstoneMemory {
    /// `ComparatorBlockEntity.getOutputSignal`。
    pub fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.comparator_outputs.get(&pos).copied().unwrap_or(0)
    }

    /// `ComparatorBlockEntity.setOutputSignal`。
    pub fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.comparator_outputs.insert(pos, signal);
    }

    /// `ChiseledBookShelfBlockEntity.getLastInteractedSlot`，没动过时是 `None`。
    pub fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.bookshelf_slots.get(&pos).copied()
    }

    /// 放进或者拿出一本书时记下是哪一格。
    pub fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.bookshelf_slots.insert(pos, slot);
    }

    /// `JukeboxSongPlayer.isPlaying`。
    pub fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.jukebox_playing.contains(&pos)
    }

    /// `JukeboxSongPlayer.play`、`stop` 之后记下这一格在不在放。
    pub fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        if playing {
            self.jukebox_playing.insert(pos);
        } else {
            self.jukebox_playing.remove(&pos);
        }
    }

    /// 这一格换了方块，官方把方块实体删掉，下次新建的是空的。
    pub fn forget_block_entity(&mut self, pos: BlockPos) {
        self.comparator_outputs.remove(&pos);
        self.bookshelf_slots.remove(&pos);
        self.jukebox_playing.remove(&pos);
    }

    /// 扔掉 `when` 之前的开关记录，官方在红石火把的计划刻开头做这件事。
    pub fn forget_torch_toggles_before(&mut self, when: i64) {
        let keep = self
            .torch_toggles
            .iter()
            .position(|&(_, toggled_at)| toggled_at >= when)
            .unwrap_or(self.torch_toggles.len());
        self.torch_toggles.drain(..keep);
    }

    /// 记下一次开关。
    pub fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.torch_toggles.push((pos, when));
    }

    /// 这一格还留着几次开关记录。
    pub fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.torch_toggles
            .iter()
            .filter(|&&(toggled, _)| toggled == pos)
            .count()
    }
}

/// 一次操作留下的、方块以外的结果，由调用方发给玩家。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// `levelEvent(2001, pos, state)`。`except_initiator` 表示不用发给发起者，它自己已经预测过了。
    Destroyed {
        pos: BlockPos,
        state: BlockState,
        except_initiator: bool,
    },
    /// `levelEvent(1502, pos, 0)`：红石火把开关得太频繁，烧坏时冒的那股烟。
    TorchBurnout { pos: BlockPos },
    /// 发起者用空桶舀起了东西，`ItemUtils.createFilledResult` 的创造模式分支：手里还是空桶，
    /// 物品栏里没有 `item` 时放一个进去。
    FilledBucket { item: i32 },
    /// `levelEvent(1505, pos, 15)`：骨粉起作用的粒子，发给附近所有人。
    BoneMealUsed { pos: BlockPos },
    /// 告示牌的内容变了，`SignBlockEntity.markUpdated` 里的 `sendBlockUpdated`：
    /// 方块状态没变，但要把方块和方块实体数据重新发给追踪这个区块的玩家。
    SignChanged { pos: BlockPos },
    /// 上蜡成功的光照效果，`levelEvent(null, 3003, pos, 0)`。
    SignWaxed { pos: BlockPos },
    /// `SignBlock.openTextEdit`：给这名玩家打开编辑器，`player.openTextEdit(sign, slot)`。
    OpenSignEditor { pos: BlockPos, slot: SignTextSlot },
}
