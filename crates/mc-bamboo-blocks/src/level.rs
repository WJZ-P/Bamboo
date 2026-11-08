//! 这个 crate 和世界存储的边界。

use std::collections::{HashMap, HashSet};

use mc_bamboo_core::{
    BlockPos, ChunkPos, LightLayer,
    sign::{SignContent, SignTextSlot},
};
use mc_bamboo_data::SoundSource;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::{BlockEntities, BlockEvents},
    container::Container,
    entity::Entities,
    hopper::HopperEntity,
    random::LevelRandom,
    shape::Aabb,
    state::{BlockState, NoteBlockInstrument},
};

/// 世界的存储和世界外的一切。`mc-bamboo-game` 和 oracle 各有一个实现。
pub trait Level {
    /// 世界外返回 `void_air`。
    fn block_state(&self, pos: BlockPos) -> BlockState;

    /// 写入一格，不做任何连锁。高度超出世界时什么也不做，返回 `false`。
    ///
    /// `flags` 是 [`set_block`](crate::World::set_block) 的那一组，存储只需要看
    /// [`UPDATE_CLIENTS`](crate::world::UPDATE_CLIENTS)：官方只在带这个标记时才把改动发给客户端。
    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool;

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

    /// `getBlockEntity(pos) instanceof SkullBlockEntity` 时它的 `getNoteBlockSound`。
    ///
    /// 官方把物品上的 `note_block_sound` 组件搬进头颅方块实体，音符盒响的时候按它找自定义
    /// 音效，见 [`Effect::NoteBlockPlayed`]。不是头颅方块实体、或者头颅没带音效时返回
    /// `None`，官方对应 `null`。
    fn note_block_sound(&self, pos: BlockPos) -> Option<String>;

    /// 这一格的方块实体和它们的 ticker。
    fn block_entities(&self) -> &BlockEntities;

    /// 见 [`Level::block_entities`]。
    fn block_entities_mut(&mut self) -> &mut BlockEntities;

    /// `Level.blockEvents`：`level.blockEvent` 排进来的事件。
    fn block_events(&mut self) -> &mut BlockEvents;

    /// 红石用到的、方块状态装不下的世界数据。
    fn redstone(&self) -> &RedstoneMemory;

    /// 见 [`Level::redstone`]。
    fn redstone_mut(&mut self) -> &mut RedstoneMemory;

    /// 这个世界里的实体。实体由世界线程拥有，见 `docs/folia.md`。
    ///
    /// Entities in this world. The world thread owns them; see `docs/folia.md`.
    fn entities(&self) -> &Entities;

    /// 见 [`Level::entities`]。
    ///
    /// See [`Level::entities`].
    fn entities_mut(&mut self) -> &mut Entities;

    /// 这一格的物品栏，对应官方的 `getBlockContainer`（摆在方块里的那个）。
    ///
    /// 双箱已经按官方 `CompoundContainer` 的顺序合并成一张表。这一格不是容器方块时
    /// 不调用 `f`，返回 `false`。
    /// 漏斗取放物品时官方会忽略压住箱子的方块（`getContainerAt` 传 `ignoreBeingBlocked=true`），
    /// 所以这里的容器是能取到的那个，和比较器看到的可能不同。
    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 这一格自己那张物品栏里每格有多少个物品，双箱只算这一半。
    ///
    /// 官方的 `BlockEntity.preRemoveSideEffects` 掉落的是方块实体自己的 `getItems`，
    /// 所以它和 [`Level::with_container`] 不同：不合并双箱，也不管压住箱子的方块。
    /// 没有物品表时返回空表，调用方按空槽处理。
    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        let _ = pos;
        Vec::new()
    }

    /// 比较器从这一格的容器读到的模拟信号，官方 `getRedstoneSignalFromBlockEntity`。
    ///
    /// 双箱按合并后的 54 格算，和被压住读不到时是 0，见 `ChestBlock.getAnalogOutputSignal`。
    fn container_signal(&self, pos: BlockPos) -> i32 {
        let _ = pos;
        0
    }

    /// 这一格容器背后的方块位置：单箱是这一格，双箱是两半，见 `CompoundContainer.setChanged`。
    ///
    /// 和 [`Level::with_container`] 一样按能取到的那条规则算，被压住的箱子也算能取到。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        vec![pos]
    }

    /// 带着这一格的漏斗方块实体运行一段代码。这一格没有漏斗实体时不调用 `f`，返回 `false`。
    ///
    /// 实体正在 tick 时它已经从方块实体表里拿出来了，这时也取不到，和官方
    /// `getBlockEntity(pos)` 在 tick 期间找不到自己一样。
    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 雕纹书架、唱片机、饰纹陶罐和合成器的槽位表。没有这张表时不调用 `f`。
    ///
    /// Slot list for a chiseled bookshelf, jukebox, decorated pot or crafter. `f` is not called
    /// when the list is absent.
    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let _ = (pos, len, f);
        false
    }

    /// 这一格容器现在的槽位。没有容器时是空的。
    ///
    /// The container slots at this position, or an empty list when there is no container.
    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        let _ = pos;
        Vec::new()
    }

    /// 删掉这一格的容器槽位。破坏唱片机时 `setTheItem(EMPTY)` 在通知邻居之前清空。
    ///
    /// Drop the container slots at this position. Breaking a jukebox runs `setTheItem(EMPTY)`
    /// before neighbors are told.
    fn clear_container(&mut self, pos: BlockPos) {
        let _ = pos;
    }
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
    /// `JukeboxBlockEntity` 的 `songPlayer.isPlaying()`。唱片在容器槽位里。
    /// 场景里直接摆下的 `has_record=true` 唱机没有经过 `setTheItem`，所以没在放。
    ///
    /// `JukeboxSongPlayer.isPlaying`. The disc lives in the container slot.
    /// A scene jukebox placed with `has_record=true` never went through `setTheItem`, so it is
    /// not playing.
    jukebox_playing: HashSet<BlockPos>,
    /// `CrafterBlockEntity` 的禁用槽。位 `n` 为 1 表示第 `n` 格禁用。没有记录时全是 0。
    ///
    /// Disabled slots of a `CrafterBlockEntity`. Bit `n` set means slot `n` is disabled.
    /// Missing positions are all enabled.
    crafter_disabled: HashMap<BlockPos, u16>,
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

    /// `CrafterBlockEntity.isSlotDisabled` 的九个位。
    ///
    /// The nine bits behind `CrafterBlockEntity.isSlotDisabled`.
    pub fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.crafter_disabled.get(&pos).copied().unwrap_or(0)
    }

    /// 写下合成器的禁用槽。全是 0 时不留记录。
    ///
    /// Store a crafter's disabled slots. An all-zero mask is not recorded.
    pub fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        if mask == 0 {
            self.crafter_disabled.remove(&pos);
        } else {
            self.crafter_disabled.insert(pos, mask);
        }
    }

    /// 这一格换了方块，官方把方块实体删掉，下次新建的是空的。
    pub fn forget_block_entity(&mut self, pos: BlockPos) {
        self.comparator_outputs.remove(&pos);
        self.bookshelf_slots.remove(&pos);
        self.jukebox_playing.remove(&pos);
        self.crafter_disabled.remove(&pos);
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// `playSound` / `playSeededSound`：坐标乘 8 截断后发给范围内的玩家。
    Sound {
        /// `sound_event` 注册表 ID。
        sound: i32,
        source: SoundSource,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        /// 官方传的 `except` 是发起这次操作的玩家。
        except_initiator: bool,
        /// 显式给的种子。音符盒用 `Level.random.nextLong()`，其余由网络层另取。
        seed: Option<i64>,
    },
    /// `levelEvent`：客户端自己放粒子和声音。传播范围是 64 格，坐标是方块整数坐标。
    LevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
        except_initiator: bool,
    },
    /// 音符盒响了，`NoteBlock.triggerEvent`。`pitch` 是官方的 `getPitchFromNote(note)`，
    /// `seed` 是官方传给 `playSeededSound` 的那个随机数。
    ///
    /// 上面是带 `note_block_sound` 的头颅时，官方播放的是那个音效，不按乐器查表。
    /// 名字在这一次操作的自定义音效表里，[`NoteBlockPlayed::custom`] 是下标。
    NoteBlockPlayed {
        pos: BlockPos,
        instrument: NoteBlockInstrument,
        note: i32,
        pitch: f32,
        seed: i64,
        /// 自定义音效在 [`intern_custom_sound`] 那张表里的下标。`None` 用乐器表。
        custom: Option<u32>,
    },
    /// `ServerLevel.runBlockEvents` 里的 `ClientboundBlockEventPacket`：方块事件处理成功了，
    /// 把这一项发给附近 64 格内的玩家。`block` 是方块的注册表 ID，官方包里的第三个字段。
    BlockEvent {
        pos: BlockPos,
        block: usize,
        param_a: i32,
        param_b: i32,
    },
    /// `levelEvent(1500, pos, data)`：堆肥桶收进一个能堆肥的物品。
    /// `success` 为真时数据是 1（等级升了），否则是 0。客户端自己播粒子和声音。
    ComposterFill { pos: BlockPos, success: bool },
}

/// 自定义音效的名字。`Effect` 要能按值拷贝，名字放在这里，效果里只留下标。
///
/// 一次结算会把好几个世界的效果拼到一起，这张表不能挂在某一个世界上。
static CUSTOM_SOUNDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 记下一个 `SoundEvent.createVariableRangeEvent` 的标识符，返回下标。
pub fn intern_custom_sound(name: &str) -> u32 {
    let mut table = CUSTOM_SOUNDS.lock().expect("custom sound table");
    if let Some(index) = table.iter().position(|existing| existing == name) {
        return u32::try_from(index).expect("custom sound index");
    }
    let index = u32::try_from(table.len()).expect("custom sound index");
    table.push(name.to_owned());
    index
}

/// 按下标取回自定义音效的标识符。
pub fn custom_sound_name(index: u32) -> String {
    CUSTOM_SOUNDS
        .lock()
        .expect("custom sound table")
        .get(index as usize)
        .cloned()
        .expect("custom sound index")
}
