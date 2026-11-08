//! 三个入口，对应官方游戏里触发战利品表的地方。
//!
//! - [`block_drops`]：`Block.getDrops(state, level, pos, blockEntity, entity, tool)`。
//! - [`entity_death_drops`]：`LivingEntity.dropFromLootTable`。
//! - [`chest_loot`]：`RandomizableContainer.unpackLootTable` / `LootTable.fill`。
//!
//! 方块破坏、实体死亡、开箱子各自往 `LootParams` 里放的参数不一样，这里按官方
//! 的顺序逐个放好，随机源的选择也交给 [`LootTable`]。

use mc_bamboo_blocks::BlockState;

use crate::Result;
use crate::context::{
    BlockEntityView, DamageSourceView, EntityView, LevelView, LootParams, Position,
};
use crate::random_sequence::RandomSequences;
use crate::stack::LootStack;
use crate::table::{LootContainer, LootTable};

/// `Block.getDrops` 的参数。
pub struct BlockDropParams<'a> {
    pub state: BlockState,
    pub pos: (i32, i32, i32),
    pub level: &'a dyn LevelView,
    /// `BlockEntity`，没有就是 `None`。
    pub block_entity: Option<BlockEntityView>,
    /// 破坏方块的实体，对应 `THIS_ENTITY`。
    pub breaker: Option<EntityView>,
    /// 手里拿的工具，官方缺省是 `ItemStack.EMPTY`。
    pub tool: Option<LootStack>,
    /// 爆炸掉落时才有，对应 `EXPLOSION_RADIUS`。
    pub explosion_radius: Option<f32>,
    pub luck: f32,
}

impl BlockDropParams<'_> {
    /// 组装官方的 `LootParams`：`ORIGIN` 是方块中心，
    /// `TOOL` 是工具或空栈，其余按有没有提供来放。
    fn loot_params(&self) -> LootParams<'_> {
        LootParams {
            level: Some(self.level),
            origin: Some(Position::new(
                f64::from(self.pos.0) + 0.5,
                f64::from(self.pos.1) + 0.5,
                f64::from(self.pos.2) + 0.5,
            )),
            block_state: Some(self.state),
            block_entity: self.block_entity.clone(),
            tool: Some(self.tool.clone().unwrap_or_else(LootStack::empty)),
            this_entity: self.breaker.clone(),
            explosion_radius: self.explosion_radius,
            luck: self.luck,
            ..LootParams::default()
        }
    }
}

/// `Block.getDrops`：返回方块掉落物，已经按最大堆叠数拆好。
///
/// 没有掉落表的方块（比如空气）得到空列表。
pub fn block_drops(
    params: &BlockDropParams<'_>,
    sequences: Option<&RandomSequences>,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
) -> Result<Vec<LootStack>> {
    let Some(table_name) = mc_bamboo_data::loot::block_loot_table(params.state.block().name) else {
        return Ok(Vec::new());
    };
    let table = LootTable::from_name(table_name)?;
    let loot_params = params.loot_params();
    table.get_random_items_with_seed(&loot_params, 0, sequences, fallback)
}

/// `LivingEntity.dropFromLootTable` 的参数。
pub struct EntityDeathParams<'a> {
    /// 死掉的实体，对应 `THIS_ENTITY`。
    pub entity: EntityView,
    /// 死亡位置，官方用的是 `this.position()`。
    pub origin: Position,
    pub damage_source: DamageSourceView,
    /// `damageSource.getEntity()`。
    pub attacking_entity: Option<EntityView>,
    /// `damageSource.getDirectEntity()`。
    pub direct_attacking_entity: Option<EntityView>,
    /// 只有玩家击杀时官方才放，对应 `LAST_DAMAGE_PLAYER`。
    pub last_damage_player: Option<EntityView>,
    /// 玩家击杀时是玩家的幸运值，否则 0。
    pub luck: f32,
    pub level: Option<&'a dyn LevelView>,
}

impl EntityDeathParams<'_> {
    /// 组装 `LootContextParamSets.ENTITY` 的参数，顺序和
    /// `LivingEntity.dropFromLootTable` 放进 `LootParams.Builder` 的一样。
    ///
    /// Builds the `LootContextParamSets.ENTITY` parameters in the same order
    /// `LivingEntity.dropFromLootTable` fills `LootParams.Builder`.
    pub fn loot_params(&self) -> LootParams<'_> {
        LootParams {
            level: self.level,
            origin: Some(self.origin),
            this_entity: Some(self.entity.clone()),
            damage_source: Some(self.damage_source.clone()),
            attacking_entity: self.attacking_entity.clone(),
            direct_attacking_entity: self.direct_attacking_entity.clone(),
            last_damage_player: self.last_damage_player.clone(),
            luck: self.luck,
            ..LootParams::default()
        }
    }
}

/// 实体死亡掉落。`seed` 是实体的 `LootTableSeed`，非零时会用旧版随机源。
pub fn entity_death_drops(
    table_name: &str,
    params: &EntityDeathParams<'_>,
    seed: i64,
    sequences: Option<&RandomSequences>,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
) -> Result<Vec<LootStack>> {
    let table = LootTable::from_name(table_name)?;
    let loot_params = params.loot_params();
    table.get_random_items_with_seed(&loot_params, seed, sequences, fallback)
}

/// `RandomizableContainer.unpackLootTable` 的参数。
pub struct ChestParams<'a> {
    /// 方块实体的位置，`ORIGIN` 取它的中心。
    pub pos: (i32, i32, i32),
    /// 打开容器的玩家，官方只有这时才放 `THIS_ENTITY` 和幸运值。
    pub player: Option<EntityView>,
    pub luck: f32,
    pub level: &'a dyn LevelView,
}

impl ChestParams<'_> {
    fn loot_params(&self) -> LootParams<'_> {
        LootParams {
            level: Some(self.level),
            origin: Some(Position::new(
                f64::from(self.pos.0) + 0.5,
                f64::from(self.pos.1) + 0.5,
                f64::from(self.pos.2) + 0.5,
            )),
            this_entity: self.player.clone(),
            luck: self.luck,
            ..LootParams::default()
        }
    }
}

/// 箱子填充。`seed` 是方块实体里的 `LootTableSeed`，0 表示按表的随机序列走。
pub fn chest_loot(
    table_name: &str,
    container: &mut dyn LootContainer,
    params: &ChestParams<'_>,
    seed: i64,
    sequences: Option<&RandomSequences>,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
) -> Result<()> {
    let table = LootTable::from_name(table_name)?;
    let loot_params = params.loot_params();
    table.fill(container, &loot_params, seed, sequences, fallback)
}
