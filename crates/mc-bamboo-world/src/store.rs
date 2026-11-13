//! 区块表：所有区块读写的唯一入口。
//!
//! 超平坦世界的区块内容都相同，所以只存一个模板，第一次改动一个区块时才给它一份自己的拷贝。
//! 光照也跟着区块走：改方块时记下光照要检查的地方，[`run_light_updates`](ChunkStore::run_light_updates) 时一轮轮地算。
//! 以后加 ticket 和卸载时，也只需要改这一个地方。
//!
//! 每个区块有一个版本号 [`ChunkRevision`]，区块和它那一列光照每被写一次就换一个新值，区块包的缓存靠它判断内容变没变。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Deref,
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{block_of_state, facts, registries::block_entity_type, state_facts};

use crate::{
    MIN_Y, WORLD_HEIGHT,
    chunk::{Chunk, HeightmapKind, Section},
    column::{ChunkColumn, LightPending},
    flat,
    light::{Chunks, LevelLight, LightChanges, SectionPos, write_block},
    ticket::ChunkHasherBuilder,
};

/// 世界里的全部区块。
///
/// 改过的区块一直留在内存里，直到服务端关闭。现在没有存档，卸载就等于丢掉玩家的改动。
#[derive(Debug, Clone)]
pub struct ChunkStore {
    template: Chunk,
    edited: EditedChunks,
    light: LevelLight,
    /// `None` = 无限超平坦（测试/oracle 专用退化实现）；`Some` = 已加载模式。
    loaded: Option<HashSet<ChunkPos, ChunkHasherBuilder>>,
    /// 每个插过盒子的位置的原样起点。没记过的位置按模板算。
    pristine: ChunkMap<PristineMark>,
    /// 方块或光照版本变过的区块。存档每刻只看这个集合，不扫全部已加载区块。
    changed: HashSet<ChunkPos, ChunkHasherBuilder>,
}

/// 按区块坐标索引的表，用和票表一样的便宜哈希。读方块的热路径每次都查它。
pub type ChunkMap<V> = HashMap<ChunkPos, V, ChunkHasherBuilder>;

/// 改过的区块和它们的版本号。
///
/// 读的时候就是一张普通的表。写只能经过 [`chunk_mut`](Self::chunk_mut)，它顺便换一个新的版本号：
/// 这里故意不实现 `DerefMut`，以后加新的写法时绕不过去，区块包的缓存也就不会漏掉修改。
#[derive(Debug, Clone, Default)]
struct EditedChunks {
    chunks: ChunkMap<Chunk>,
    revisions: ChunkMap<u64>,
    /// 发出去的最后一个版本号。
    last_revision: u64,
    /// 版本变过、还没被存档取走的区块。
    changed: HashSet<ChunkPos, ChunkHasherBuilder>,
}

impl EditedChunks {
    /// 要改这个区块了，第一次改时从模板复制一份。
    fn chunk_mut(&mut self, pos: ChunkPos, template: &Chunk) -> &mut Chunk {
        self.bump_revision(pos);
        self.chunks.entry(pos).or_insert_with(|| template.clone())
    }

    /// 给这个区块换一个从没用过的版本号。
    fn bump_revision(&mut self, pos: ChunkPos) {
        self.last_revision += 1;
        self.revisions.insert(pos, self.last_revision);
        self.changed.insert(pos);
    }

    /// 没改过的区块是 0。
    fn revision(&self, pos: ChunkPos) -> u64 {
        self.revisions.get(&pos).copied().unwrap_or_default()
    }
}

impl Deref for EditedChunks {
    type Target = ChunkMap<Chunk>;

    fn deref(&self) -> &Self::Target {
        &self.chunks
    }
}

/// 一个区块的版本号，见 [`ChunkStore::revision`]。只用来比较是否相等。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChunkRevision {
    chunk: u64,
    /// 天空光和方块光。
    light: [u64; 2],
}

/// 放进这个位置的盒子「还是不是生成器的原样输出」的起点：盒子的来源标记，
/// 加上放进全部动作做完之后这个位置的版本号。取出时只要版本号没变，
/// 就说明这段时间里没有任何写路径碰过它，不需要逐项比较内容。
#[derive(Debug, Clone, Copy, Default)]
struct PristineMark {
    /// 盒子是不是生成器输出（`from_parts` 读盘/解码来的是 `false`）。
    /// 没插入过盒子的位置按「模板，原样」算：无限超平坦世界就是这个情况。
    from_generator: bool,
    revision: ChunkRevision,
}

impl PristineMark {
    /// 从没插入过盒子的位置：内容就是模板，版本号从 0 起。
    const TEMPLATE: Self = Self {
        from_generator: true,
        revision: ChunkRevision {
            chunk: 0,
            light: [0, 0],
        },
    };
}

impl ChunkStore {
    /// 用 `template` 作为所有没改过的区块的内容，并算好它的光照。
    pub fn new(mut template: Chunk) -> Self {
        template.initialize_light_sources();
        let light = LevelLight::for_template(&template);
        Self {
            template,
            edited: EditedChunks::default(),
            light,
            loaded: None,
            pristine: ChunkMap::default(),
            changed: HashSet::default(),
        }
    }

    /// 超平坦世界。**无限模式**：每个区块位置都隐式存在，读成模板。
    /// 这是测试和 oracle 用的退化实现，正式运行的 `World` 用 [`loaded`](Self::loaded)。
    pub fn flat() -> Self {
        Self::new(flat::generate_chunk())
    }

    /// 已加载模式：只有 [`load`](Self::load) 过的区块存在，别的位置读到「未加载」。
    ///
    /// 和无限模式的区别在光照存储：没加载的位置没有光照柱，方块变化引起的邻居计数
    /// 更新不会给它建柱；加载进来时由 `rebind_area` 按当时的方块重算。
    pub fn loaded() -> Self {
        let mut store = Self::new(flat::generate_chunk());
        store.loaded = Some(HashSet::default());
        store.light.set_loaded(Some(HashSet::default()));
        store.pristine.clear();
        store
    }

    /// 是不是已加载模式。
    pub fn is_loaded_mode(&self) -> bool {
        self.loaded.is_some()
    }

    /// 这个区块位置有没有加载。无限模式恒为真。
    /// `loaded` 用便宜哈希：读方块、写方块、读光照每次都问它。
    pub fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.loaded.as_ref().is_none_or(|set| set.contains(&pos))
    }

    /// 已加载的区块数；无限模式返回 `None`。
    pub fn loaded_count(&self) -> Option<usize> {
        self.loaded.as_ref().map(HashSet::len)
    }

    /// 这个区块是不是还保持生成器原样。
    pub fn is_pristine(&self, pos: ChunkPos) -> bool {
        let mark = self
            .pristine
            .get(&pos)
            .copied()
            .unwrap_or(PristineMark::TEMPLATE);
        mark.from_generator && mark.revision == self.revision(pos)
    }

    /// 把区块放回「版本变过」集合，供下一刻重新判脏。
    pub fn mark_changed(&mut self, pos: ChunkPos) {
        self.changed.insert(pos);
    }

    /// 取走「方块或光照版本变过」的区块集合，给存档每刻判脏用。
    /// 只和真正改动过的区块数成正比，不扫全部已加载区块。
    pub fn take_changed_chunks(&mut self) -> HashSet<ChunkPos, ChunkHasherBuilder> {
        std::mem::take(&mut self.changed)
    }

    /// 每个已加载区块的坐标、当前版本号，以及「是不是还保持生成器原样」。
    /// 运行中的匀速存档用它判断哪些已加载区块脏了（改过且版本号和上次落盘不同）；
    /// 原样区块照官方不存。无限模式没有已加载集合，返回空。
    ///
    /// For each loaded chunk: its position, current revision, and whether it is
    /// still the untouched generator output. Paced run-time saving uses this to
    /// find dirty loaded chunks (edited and revision differs from the last save);
    /// pristine chunks are not saved, like vanilla. Empty in infinite-flat mode.
    pub fn loaded_states(&self) -> Vec<(ChunkPos, ChunkRevision, bool)> {
        let Some(loaded) = &self.loaded else {
            return Vec::new();
        };
        loaded
            .iter()
            .map(|&pos| {
                let revision = self.revision(pos);
                let mark = self
                    .pristine
                    .get(&pos)
                    .copied()
                    .unwrap_or(PristineMark::TEMPLATE);
                let pristine = mark.from_generator && mark.revision == revision;
                (pos, revision, pristine)
            })
            .collect()
    }

    /// 把一个区块束装进已加载集合。调用方保证位置原本没有内容。
    /// 只有已加载模式能调用；无限模式忽略。
    ///
    /// 先登记已加载再 `insert_column`：后者重算 3×3 段状态时就能按
    /// 「这一柱已经在了」算，不需要 `mark_loaded` 再重算第二遍。
    pub fn load(&mut self, pos: ChunkPos, column: Box<crate::column::ChunkColumn>) {
        if self.loaded.is_none() {
            return;
        }
        self.loaded.as_mut().expect("已加载模式").insert(pos);
        self.light.add_loaded(pos);
        self.insert_column(pos, column)
            .unwrap_or_else(|_| unreachable!("交付前光照已经算完"));
    }

    /// 只把 `pos` 登记为已加载，不搬盒子。
    ///
    /// 给「先 `insert_column` 装好内容、再登记已加载」的交付路径用；
    /// 交付只此一处，且它只调一次 `rebind_area`（在 `insert_column` 里）。
    pub fn mark_loaded(&mut self, pos: ChunkPos) {
        if let Some(set) = self.loaded.as_mut() {
            set.insert(pos);
        }
        self.light.add_loaded(pos);
    }

    /// 把区块从已加载集合里整个取走，返回盒子。只有已加载模式能调用。
    pub fn unload(
        &mut self,
        pos: ChunkPos,
    ) -> Result<Box<crate::column::ChunkColumn>, crate::column::LightPending> {
        if !self.is_loaded(pos) {
            return Err(crate::column::LightPending);
        }
        if !self.ready_to_move_columns() {
            return Err(crate::column::LightPending);
        }
        // 先把盒子从引擎里取出来（这时柱子还在，`take_column` 能拿到真实数据），
        // 再从已加载集合里注销：注销会真的删掉光照柱。
        // `take_column` 里的 `rebind_area` 按「这一柱还在」重算邻居计数，
        // 注销后由下一次 `rebind_area` 修正（加载/卸载成对发生时净效果正确）。
        let column = self.take_column(pos);
        let _ = self.finish_unload(pos);
        column
    }

    /// 盒子已由 [`Self::take_column`] 取走之后，注销这个位置的已加载状态和光照柱。
    ///
    /// 一次收回只需要取一次盒子；`unload` 是「取盒子 + 注销」的便捷入口，
    /// region 批量收回时用这条路径，避免再克隆一份模板柱做丢弃。
    pub fn finish_unload(&mut self, pos: ChunkPos) -> Result<(), crate::column::LightPending> {
        if !self.is_loaded(pos) || !self.ready_to_move_columns() {
            return Err(crate::column::LightPending);
        }
        if let Some(set) = self.loaded.as_mut() {
            set.remove(&pos);
        }
        // `take_column` 已经把这一柱从 `edited` 表里取走了。
        self.light.remove_loaded(pos);
        // 这一柱整个离开了世界，别留着一个指向旧版本号的原样起点。
        self.pristine.remove(&pos);
        // 注销之后再重算一遍 3×3：这时 `pos` 已经不算已加载，
        // 邻居段的计数按「那个位置没有方块」修正。
        let Self {
            template,
            edited,
            loaded,
            light: engine,
            ..
        } = self;
        engine.rebind_area(pos, Self::chunks_view(template, edited, loaded.as_ref()));
        Ok(())
    }

    /// 所有没改过的区块共用的内容。
    pub fn template(&self) -> &Chunk {
        &self.template
    }

    /// 改过的区块。这个位置没改过时返回 `None`，内容就是 [`template`](Self::template)。
    pub fn edited(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.edited.get(&pos)
    }

    /// 所有改过的区块，按坐标顺序。给算阶段一次性扫「有没有漏斗」这类只读事实用。
    pub fn edited_chunks(&self) -> impl Iterator<Item = (ChunkPos, &Chunk)> + '_ {
        self.edited.iter().map(|(&pos, chunk)| (pos, chunk))
    }

    /// 读方块的热路径用的一次查表：已加载模式下每个装进来的区块都在
    /// `edited` 表里（`insert_column` 会放进，模板柱子也在），没加载的位置没有键。
    /// 无限模式下没有键的位置读共享模板。
    #[inline]
    fn loaded_chunk(&self, pos: ChunkPos) -> Option<&Chunk> {
        if self.loaded.is_none() {
            Some(self.edited.get(&pos).unwrap_or(&self.template))
        } else {
            self.edited.get(&pos)
        }
    }

    /// 一个区块现在的内容。
    pub fn chunk(&self, pos: ChunkPos) -> &Chunk {
        self.edited.get(&pos).unwrap_or(&self.template)
    }

    /// 这个区块的方块和光照都和模板一样，可以共用模板的编码。
    pub fn is_template(&self, pos: ChunkPos) -> bool {
        !self.edited.contains_key(&pos) && !self.light.is_materialized(pos)
    }

    /// 这个区块现在的版本号。方块、高度图、方块实体、告示牌和这一列的光照，任何一样被写过都会换成
    /// 没用过的新值。所以两次取到的值相同，这个区块编出来的区块包就一个字节都不差。
    pub fn revision(&self, pos: ChunkPos) -> ChunkRevision {
        ChunkRevision {
            chunk: self.edited.revision(pos),
            light: self.light.revision(pos),
        }
    }

    /// 读取方块状态。高度超出世界、或者已加载模式下这个区块没加载时返回 `None`。
    /// 坐标可以直接来自客户端。
    pub fn block(&self, pos: BlockPos) -> Option<u32> {
        if !is_inside_world(pos.y) {
            return None;
        }
        Some(
            self.loaded_chunk(pos.chunk())?
                .block(local(pos.x), pos.y, local(pos.z)),
        )
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`，读区块里一直维护着的高度图。
    /// 高度超出世界时返回 `None`。
    pub fn heightmap_y(&self, pos: BlockPos) -> Option<i32> {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return None;
        }
        Some(self.chunk(pos.chunk()).heightmap_y(
            HeightmapKind::MotionBlocking,
            local(pos.x),
            local(pos.z),
        ))
    }

    /// 这一格所在 quart 位置的群系注册表 ID。高度超出世界时返回 `None`。
    pub fn biome_at(&self, pos: BlockPos) -> Option<u32> {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return None;
        }
        Some(
            self.chunk(pos.chunk())
                .biome_at(local(pos.x), pos.y, local(pos.z)),
        )
    }

    /// 区块 `pos` 里哪些区块段有会随机刻的方块：第 i 位对应从世界底部数起的第 i 段。
    /// 没加载的区块一段也没有。和逐段问 [`Self::is_randomly_ticking_section`] 的答案相同，
    /// 只查一次区块表、只读区块里的一个字。
    pub fn randomly_ticking_sections(&self, pos: ChunkPos) -> u32 {
        if !self.is_loaded(pos) {
            return 0;
        }
        self.edited
            .get(&pos)
            .unwrap_or(&self.template)
            .randomly_ticking_sections()
    }

    /// 区块 `pos` 里纵坐标是 `section_y`（方块的 `y >> 4`）的区块段有会随机刻的方块。
    /// 超出世界高度的区块段没有方块。
    pub fn is_randomly_ticking_section(&self, pos: ChunkPos, section_y: i32) -> bool {
        if !self.is_loaded(pos) {
            return false;
        }
        let chunk = self.edited.get(&pos).unwrap_or(&self.template);
        usize::try_from(section_y - (MIN_Y >> 4))
            .ok()
            .and_then(|index| chunk.sections().get(index))
            .is_some_and(Section::is_randomly_ticking)
    }

    /// `section_y`（方块的 `y >> 4`）这一段的整段方块状态哈希，红石缓存的键读它。
    /// 区块没加载、段超出世界高度时返回 `None`。
    pub fn section_hash(&self, pos: ChunkPos, section_y: i32) -> Option<u128> {
        if !self.is_loaded(pos) {
            return None;
        }
        self.edited
            .get(&pos)
            .unwrap_or(&self.template)
            .section_hash(section_y)
    }

    /// 设置方块状态，返回方块是否真的变了。高度超出世界时什么也不做。
    ///
    /// 和官方的 `LevelChunk.setBlockState` 一样：区块段变空或者变满时登记一下，
    /// 光照性质变了时更新这一列的天空光源，再记下这一格要检查。光照要等 [`run_light_updates`](Self::run_light_updates) 才算。
    pub fn set_block(&mut self, pos: BlockPos, state: u32) -> bool {
        let Some(previous) = self.block(pos) else {
            return false;
        };
        let was_empty = self.chunk(pos.chunk()).is_section_empty(pos.y);
        if previous == state || (was_empty && is_air(state)) {
            return false;
        }
        let current = self.edited.get(&pos.chunk()).unwrap_or(&self.template);
        self.light.block_changing(current, pos, state);
        let chunk = self.edited.chunk_mut(pos.chunk(), &self.template);
        let light_changed = write_block(chunk, pos, state);
        let is_empty = chunk.is_section_empty(pos.y);

        if was_empty != is_empty {
            self.light
                .update_section_status(SectionPos::of(pos), is_empty);
        }
        if light_changed {
            self.light.check_block(pos);
        }
        true
    }

    /// 这一格的告示牌内容，对应官方 `getBlockEntity` 拿到 `SignBlockEntity` 后读的字段。
    ///
    /// 告示牌方块还没写过字时返回空白内容：官方也是按状态临时建一个空的方块实体。
    /// 不是告示牌方块时返回 `None`。
    pub fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return None;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        let chunk = self.chunk(pos.chunk());
        match chunk.sign(x, pos.y, z) {
            Some(content) => Some(content.clone()),
            None => is_sign(chunk.block(x, pos.y, z)).then(SignContent::default),
        }
    }

    /// 改写告示牌内容。这一格不是告示牌方块时什么也不做，返回 `false`。
    pub fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return false;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        if !is_sign(self.chunk(pos.chunk()).block(x, pos.y, z)) {
            return false;
        }
        let chunk = self.edited.chunk_mut(pos.chunk(), &self.template);
        chunk.set_sign(x, pos.y, z, content);
        true
    }

    /// 这一格的方块实体要发给客户端的 NBT，官方 `BlockEntity.getUpdateTag` 的结果。
    ///
    /// 内容由 `mc-bamboo-game` 算好写进来：这一层不认识物品组件，只负责跟着方块存和丢。
    /// 告示牌不走这里，它的内容在 [`sign`](Self::sign)。
    pub fn update_tag(&self, pos: BlockPos) -> Option<&[u8]> {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return None;
        }
        self.chunk(pos.chunk())
            .update_tag(local(pos.x), pos.y, local(pos.z))
    }

    /// 改写这一格方块实体要发给客户端的 NBT。`None` 表示这一格没有要发的内容。
    ///
    /// 这一格没有方块实体时什么也不做，返回 `false`。
    pub fn set_update_tag(&mut self, pos: BlockPos, tag: Option<Vec<u8>>) -> bool {
        if !is_inside_world(pos.y) || !self.is_loaded(pos.chunk()) {
            return false;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        if block_of_state(self.chunk(pos.chunk()).block(x, pos.y, z))
            .is_none_or(|block| block.block_entity_type.is_none())
        {
            return false;
        }
        // 内容没变就不动区块：版本号不变，区块包的缓存也不用重新编码。
        if self.update_tag(pos) == tag.as_deref() {
            return true;
        }
        let chunk = self.edited.chunk_mut(pos.chunk(), &self.template);
        chunk.set_update_tag(x, pos.y, z, tag);
        true
    }

    /// 正在被某名玩家编辑的告示牌和它们的编辑器。
    pub fn sign_editors(&self) -> impl Iterator<Item = (BlockPos, [u8; 16])> + '_ {
        self.edited.iter().flat_map(|(chunk_pos, chunk)| {
            chunk.signs().filter_map(move |(x, y, z, content)| {
                content.editor.map(|editor| {
                    let pos =
                        BlockPos::new(chunk_pos.x * 16 + x as i32, y, chunk_pos.z * 16 + z as i32);
                    (pos, editor)
                })
            })
        })
    }

    /// 算攒着的光照，官方的光照线程在两个 tick 之间做这件事：攒着的任务排成一轮，再一轮轮地算。
    ///
    /// 工作量（任务数加上传播的步数）到了 `budget` 就停，哪怕一轮算到一半，下次接着算。
    /// 一轮算完才换上新的光照，[`light_value`](Self::light_value) 读到的总是上一轮算完的结果。返回这次的工作量。
    pub fn run_light_updates(&mut self, budget: usize) -> usize {
        let Self {
            template,
            edited,
            loaded,
            light: engine,
            ..
        } = self;
        engine.run_updates(Self::chunks_view(template, edited, loaded.as_ref()), budget)
    }

    /// 光照引擎读方块时的视图，带已加载集合。
    ///
    /// 借用拆开成三个字段，调用方可以同时可变借 `self.light`。
    fn chunks_view<'a>(
        template: &'a Chunk,
        edited: &'a ChunkMap<Chunk>,
        loaded: Option<&'a HashSet<ChunkPos, ChunkHasherBuilder>>,
    ) -> Chunks<'a> {
        let mut chunks = Chunks::new(template, edited);
        if let Some(set) = loaded {
            chunks = chunks.with_loaded(set);
        }
        chunks
    }

    /// 把攒着的光照全部算完。
    pub fn finish_light_updates(&mut self) {
        self.run_light_updates(usize::MAX);
    }

    /// 还有没算完的光照。
    pub fn has_pending_light_updates(&self) -> bool {
        self.light.has_pending_updates()
    }

    /// 上次取走以后光照变了的区块和段，发 `light_update` 用。
    pub fn take_light_changes(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        self.light.take_changes()
    }

    /// 世界的光照。
    pub fn light(&self) -> &LevelLight {
        &self.light
    }

    /// `getBrightness`：一格的天空光或者方块光。
    pub fn light_value(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        if !self.is_loaded(pos.chunk()) {
            // 没加载的位置读成超平坦稳态的值：天空光 15、方块光 0。
            return match layer {
                LightLayer::Sky => 15,
                LightLayer::Block => 0,
            };
        }
        self.light.light_value(layer, pos)
    }

    /// 现在能不能搬区块束，见 [`take_column`](Self::take_column) 和 [`insert_column`](Self::insert_column)。
    /// 调用方可以先问，再决定要不要把光照跑完。
    pub fn ready_to_move_columns(&self) -> bool {
        self.light.ready_to_move_columns()
    }

    /// 只读快照：克隆 `pos` 这一柱的方块和光照，各表分毫不动（补 11）。
    /// 存档编码需要的是一份 `ChunkColumn`，这里按 `take_column` 同样的方式组装。
    /// Read-only clone of this column's blocks and light (item 11). Save
    /// encoding needs a `ChunkColumn`; assemble it exactly like `take_column`.
    pub fn column_clone(&self, pos: ChunkPos) -> Option<crate::column::ChunkColumn> {
        if !self.is_loaded(pos) {
            return None;
        }
        let chunk = self
            .edited
            .chunks
            .get(&pos)
            .cloned()
            .unwrap_or_else(|| self.template.clone());
        let light = self.light.column_clone(pos);
        let mark = self
            .pristine
            .get(&pos)
            .copied()
            .unwrap_or(PristineMark::TEMPLATE);
        let pristine = mark.from_generator && mark.revision == self.revision(pos);
        Some(crate::column::ChunkColumn::new(chunk, light, pristine))
    }

    /// 把 `pos` 这个区块整个拿走。之后这个位置读起来和没改过的区块一样（模板）。
    ///
    /// 区块束里只有一份光照，所以只允许在光照全部算完、没有算到一半的轮次时取。
    /// 搬区块束之前调用方要先用 [`finish_light_updates`](Self::finish_light_updates) 把光照跑完，
    /// 也可以先用 [`ready_to_move_columns`](Self::ready_to_move_columns) 问一声；这里不会偷偷跑。
    /// 还没发给客户端的光照变化里属于这个区块的条目一并丢掉，发区块包是调用方的事。
    ///
    /// 柱里的 `sectionStates` 记着相邻段有没有方块。[`rebind_area`](crate::light::LevelLight::rebind_area)
    /// 会在取走和放入后按目标世界此刻的方块重算这一柱和 8 个邻居柱的状态，
    /// 所以单独把一柱放到一圈不同的邻居中间也正确。
    pub fn take_column(&mut self, pos: ChunkPos) -> Result<Box<ChunkColumn>, LightPending> {
        if !self.light.ready_to_move_columns() {
            return Err(LightPending);
        }
        // 在取出动作（下面的 rebind_area、bump_revision）之前问版本号：
        // 放置时记下的版本号和现在的相同，就说明这段时间没有任何写路径碰过它。
        let mark = self
            .pristine
            .get(&pos)
            .copied()
            .unwrap_or(PristineMark::TEMPLATE);
        let pristine = mark.from_generator && mark.revision == self.revision(pos);
        let chunk = self
            .edited
            .chunks
            .remove(&pos)
            .unwrap_or_else(|| self.template.clone());
        // 注销这一柱的段状态：拿走之后这个位置回到模板（无限平坦世界里方块不变，
        // 这一步是恒等写入；以后按区块加载时这里才是真的注销）。
        let column_light = self.light.take_column(pos);
        let Self {
            template,
            edited,
            loaded,
            light: engine,
            ..
        } = self;
        engine.rebind_area(pos, Self::chunks_view(template, edited, loaded.as_ref()));
        self.edited.bump_revision(pos);
        Ok(Box::new(ChunkColumn::new(chunk, column_light, pristine)))
    }

    /// 把一个区块束放到 `pos`。这个位置原来的内容被整个换掉。
    ///
    /// 两层光照同时放进引擎和给别人读的那份，之后引擎能接着正常工作。
    /// 版本号从本 store 自己的计数器取，不沿用盒子里带来的值，区块包的缓存因此会失效。
    /// 放入不产生发给客户端的光照变化。
    ///
    /// 前提和 [`take_column`](Self::take_column) 一样：搬区块束之前调用方要先把光照跑完
    /// （[`finish_light_updates`](Self::finish_light_updates)），也可以先用
    /// [`ready_to_move_columns`](Self::ready_to_move_columns) 问一声。不满足时 store 不变，
    /// 盒子原样还给调用方，不会丢。
    ///
    /// 邻居段计数按目标世界重算，适用范围和 [`take_column`](Self::take_column) 一样。
    pub fn insert_column(
        &mut self,
        pos: ChunkPos,
        column: Box<ChunkColumn>,
    ) -> Result<(), (LightPending, Box<ChunkColumn>)> {
        if !self.light.ready_to_move_columns() {
            return Err((LightPending, column));
        }
        let from_generator = column.untouched();
        let (chunk, column_light) = column.into_parts();
        self.edited.chunks.insert(pos, chunk);
        self.edited.bump_revision(pos);
        self.light.insert_column(pos, column_light);
        // 登记这一柱的段状态：按目标世界此刻的方块重算 3×3 邻域的邻居计数，
        // 单独把一柱插进一圈不同的邻居中间也成立。
        let Self {
            template,
            edited,
            loaded,
            light: engine,
            ..
        } = self;
        engine.rebind_area(pos, Self::chunks_view(template, edited, loaded.as_ref()));
        // 放进来的全部动作（含 rebind_area 补建的数据层）都做完之后才记起点，
        // 否则一柱原样盒子刚放进来就会被自己的段状态重算判成「改过」。
        let revision = self.revision(pos);
        self.pristine.insert(
            pos,
            PristineMark {
                from_generator,
                revision,
            },
        );
        Ok(())
    }
}

const fn is_inside_world(y: i32) -> bool {
    y >= MIN_Y && y < MIN_Y + WORLD_HEIGHT
}

/// `isAir`。
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// 这个方块的方块实体是告示牌，站着、挂着、贴在墙上的都算。
fn is_sign(state: u32) -> bool {
    matches!(
        block_of_state(state).and_then(|block| block.block_entity_type),
        Some(block_entity_type::SIGN | block_entity_type::HANGING_SIGN)
    )
}

/// 世界坐标在区块里的下标。`&` 对负数也是对的，`-1` 是区块里的第 15 格。
const fn local(coordinate: i32) -> usize {
    (coordinate & 15) as usize
}

#[cfg(test)]
mod loaded_tests;
#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    #[test]
    fn untouched_chunks_read_the_template() {
        let store = ChunkStore::flat();

        assert_eq!(store.block(BlockPos::new(0, -64, 0)), Some(blocks::BEDROCK));
        assert_eq!(
            store.block(BlockPos::new(-1000, -61, 7777)),
            Some(blocks::GRASS_BLOCK)
        );
        assert_eq!(store.block(BlockPos::new(5, -60, 5)), Some(blocks::AIR));
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn the_grass_section_ticks_randomly() {
        let mut store = ChunkStore::flat();
        let chunk = ChunkPos::new(3, -2);
        assert!(store.is_randomly_ticking_section(chunk, -4));
        assert!(!store.is_randomly_ticking_section(chunk, -3));
        assert!(!store.is_randomly_ticking_section(chunk, -5));
        assert!(!store.is_randomly_ticking_section(chunk, 20));

        // 整个区块一次问出来的位图：第 i 位是从世界底部数起的第 i 段。
        assert_eq!(store.randomly_ticking_sections(chunk), 1);

        store.set_block(BlockPos::new(48, 0, -32), blocks::WHEAT);
        assert!(store.is_randomly_ticking_section(chunk, 0));
        assert_eq!(store.randomly_ticking_sections(chunk), 1 | 1 << 4);
        // 别的区块不受影响。
        assert_eq!(store.randomly_ticking_sections(ChunkPos::new(4, -2)), 1);

        // 最后一个会随机刻的方块拿走，那一位跟着清掉。
        store.set_block(BlockPos::new(48, 0, -32), blocks::AIR);
        assert_eq!(store.randomly_ticking_sections(chunk), 1);
    }

    #[test]
    fn the_ticking_section_mask_agrees_with_asking_each_section() {
        let mut store = ChunkStore::flat();
        let chunk = ChunkPos::new(-1, 5);
        let ticking = [blocks::WHEAT, blocks::GRASS_BLOCK, blocks::WATER];
        let plain = [blocks::AIR, blocks::STONE, blocks::DIRT];
        // 固定的线性同余序列，够把各段各种组合都走到。
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        for _ in 0..2000 {
            let pos = BlockPos::new(
                chunk.x * 16 + next(2) as i32,
                MIN_Y + next(384) as i32,
                chunk.z * 16 + next(2) as i32,
            );
            let state = if next(3) == 0 {
                ticking[next(3) as usize]
            } else {
                plain[next(3) as usize]
            };
            store.set_block(pos, state);
            let asked = (0..24)
                .filter(|&index| store.is_randomly_ticking_section(chunk, (MIN_Y >> 4) + index))
                .fold(0_u32, |mask, index| mask | 1 << index);
            assert_eq!(store.randomly_ticking_sections(chunk), asked);
        }
    }

    #[test]
    fn heights_outside_the_world_have_no_block() {
        let mut store = ChunkStore::flat();

        for y in [-65, 320, i32::MIN, i32::MAX] {
            let pos = BlockPos::new(0, y, 0);
            assert_eq!(store.block(pos), None);
            assert!(!store.set_block(pos, blocks::STONE));
        }
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
        assert_eq!(store.block(BlockPos::new(0, 319, 0)), Some(blocks::AIR));
    }

    #[test]
    fn first_write_copies_the_template_for_that_chunk_only() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(-1, -60, 16);

        assert!(store.set_block(pos, blocks::STONE));

        assert_eq!(store.block(pos), Some(blocks::STONE));
        let chunk = store.edited(ChunkPos::new(-1, 1)).expect("这个区块改过了");
        assert_eq!(chunk.block(15, -60, 0), blocks::STONE);
        // 拷贝保留模板的其余内容，模板本身不变。
        assert_eq!(chunk.block(0, -61, 0), blocks::GRASS_BLOCK);
        assert_eq!(store.template().block(15, -60, 0), blocks::AIR);
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn writing_the_same_state_is_a_no_op() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -61, 3);

        assert!(!store.set_block(pos, blocks::GRASS_BLOCK));
        assert!(store.edited(pos.chunk()).is_none());

        assert!(store.set_block(pos, blocks::STONE));
        assert!(!store.set_block(pos, blocks::STONE));
    }

    #[test]
    fn air_into_an_empty_section_is_a_no_op() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, 100, 3);

        assert!(!store.set_block(pos, blocks::CAVE_AIR));
        assert!(store.is_template(pos.chunk()));
    }

    #[test]
    fn edited_chunks_keep_their_block_entities() {
        let mut store = ChunkStore::flat();
        store.set_block(BlockPos::new(19, -60, -11), blocks::CHEST);

        let chunk = store.edited(ChunkPos::new(1, -1)).expect("这个区块改过了");
        let entity = chunk.block_entities().next().expect("箱子有方块实体");
        assert_eq!((entity.x, entity.y, entity.z), (3, -60, 5));
    }

    #[test]
    fn signs_start_blank_and_keep_what_is_written() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -60, 3);

        assert_eq!(store.sign(pos), None);

        store.set_block(pos, blocks::OAK_SIGN);
        assert_eq!(store.sign(pos), Some(SignContent::default()));

        let mut content = SignContent::default();
        content.front.messages[0] = "hello".to_owned();
        assert!(store.set_sign(pos, content.clone()));
        assert_eq!(store.sign(pos), Some(content));
    }

    #[test]
    fn sign_content_does_not_survive_a_different_block() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -60, 3);
        store.set_block(pos, blocks::OAK_SIGN);
        let mut content = SignContent::default();
        content.front.messages[0] = "hello".to_owned();
        store.set_sign(pos, content);

        // 含水或者转向都还是同一个方块，内容要留着。
        store.set_block(pos, blocks::OAK_SIGN + 1);
        assert!(store.sign(pos).is_some_and(|sign| sign.front.has_message()));

        store.set_block(pos, blocks::STONE);
        assert_eq!(store.sign(pos), None);
    }

    #[test]
    fn sign_content_cannot_be_written_to_another_block() {
        let mut store = ChunkStore::flat();
        assert!(!store.set_sign(BlockPos::new(0, -61, 0), SignContent::default()));
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn revision_changes_with_every_write_to_the_chunk() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -60, 3);
        let untouched = store.revision(pos.chunk());
        assert_eq!(store.revision(ChunkPos::new(-40, 70)), untouched);

        assert!(!store.set_block(BlockPos::new(3, -61, 3), blocks::GRASS_BLOCK));
        assert_eq!(store.revision(pos.chunk()), untouched);

        store.set_block(pos, blocks::OAK_SIGN);
        let placed = store.revision(pos.chunk());
        assert_ne!(placed, untouched);

        store.set_sign(pos, SignContent::default());
        let signed = store.revision(pos.chunk());
        assert_ne!(signed, placed);

        // 读不改版本号，别的区块也不受影响。
        assert_eq!(store.sign(pos), Some(SignContent::default()));
        assert_eq!(store.block(pos), Some(blocks::OAK_SIGN));
        assert_eq!(store.revision(pos.chunk()), signed);
        assert_eq!(store.revision(ChunkPos::new(-40, 70)), untouched);
    }

    #[test]
    fn light_written_next_door_changes_the_neighbor_revision() {
        let mut store = ChunkStore::flat();
        let neighbor = ChunkPos::new(1, 0);
        let far = ChunkPos::new(5, 5);
        // 光从 (0, 0) 的东边照进 (1, 0)。
        store.set_block(BlockPos::new(15, -60, 3), blocks::GLOWSTONE);
        let before = (store.revision(neighbor), store.revision(far));

        store.finish_light_updates();

        assert_ne!(store.revision(neighbor), before.0);
        assert_eq!(store.revision(far), before.1);
    }

    #[test]
    fn moving_a_column_changes_all_three_revision_components() {
        let mut store = ChunkStore::flat();
        store.set_block(BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
        store.finish_light_updates();
        let pos = ChunkPos::new(0, 0);
        let before = store.revision(pos);

        let column = store.take_column(pos).expect("光照算完了");
        let taken = store.revision(pos);
        assert_ne!(taken.chunk, before.chunk);
        assert_ne!(taken.light[0], before.light[0]);
        assert_ne!(taken.light[1], before.light[1]);

        store.insert_column(pos, column).expect("光照算完了");
        let inserted = store.revision(pos);
        assert_ne!(inserted.chunk, taken.chunk);
        assert_ne!(inserted.light[0], taken.light[0]);
        assert_ne!(inserted.light[1], taken.light[1]);
    }

    #[test]
    fn editors_are_reported_with_their_world_position() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(19, -60, -11);
        store.set_block(pos, blocks::OAK_WALL_SIGN);
        let content = SignContent {
            editor: Some([7; 16]),
            ..SignContent::default()
        };
        store.set_sign(pos, content);

        assert_eq!(store.sign_editors().collect::<Vec<_>>(), [(pos, [7; 16])]);
    }
}
