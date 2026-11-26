//! 信标方块实体，对应官方 `BeaconBlockEntity`、`BeaconBeamOwner` 和 `BeaconBeamBlock`。
//!
//! 每刻向上最多扫 10 格推进光柱段（`checkingBeamSections`）；`gameTime % 80 == 0` 且
//! 上一轮光柱非空时更新底座层数（1–4 层 `#beacon_base_blocks`），有层数时给范围内玩家
//! 施加主/副效果并播环境音；扫完到 `WORLD_SURFACE` 时切换 `beamSections`，并按活跃状态
//! 变化播激活/失效音效、触发 `construct_beacon` 进度。
//!
//! Beacon block entity, matching vanilla `BeaconBlockEntity`, `BeaconBeamOwner`, and
//! `BeaconBeamBlock`. Scans up to 10 blocks per tick to build `checkingBeamSections`; every
//! 80 ticks updates pyramid levels, applies effects, and plays the ambient sound when active;
//! swaps `beamSections` upon reaching `WORLD_SURFACE` and emits activation/deactivation sounds
//! and the `construct_beacon` trigger.

use std::any::Any;

use mc_bamboo_core::{Aabb, BlockPos, sign::DyeColor};
use mc_bamboo_data::{
    BlockClass, blocks, registries::mob_effect, sounds::sound_event, tags::block_tags,
};
use mc_bamboo_nbt::Tag;

use crate::{
    archive::tag, block_entity::BlockEntity, entity::ctx::PlayerView, level::Effect,
    state::BlockState, world::World,
};

/// 塔层数上限，官方 `BeaconBlockEntity.MAX_LEVELS`。
pub const MAX_LEVELS: i32 = 4;
/// 解锁副效果所需的最小层数，官方 `LEVELS_NEEDED_FOR_SECONDARY`。
pub const LEVELS_NEEDED_FOR_SECONDARY: i32 = 4;
/// 数据槽下标：塔层数，官方 `DATA_LEVELS`。
pub const DATA_LEVELS: usize = 0;
/// 数据槽下标：主效果，官方 `DATA_PRIMARY`。
pub const DATA_PRIMARY: usize = 1;
/// 数据槽下标：副效果，官方 `DATA_SECONDARY`。
pub const DATA_SECONDARY: usize = 2;
/// 数据槽个数，官方 `NUM_DATA_VALUES`。
pub const NUM_DATA_VALUES: usize = 3;
/// 每刻最多向上检查的格数，官方 `BLOCKS_CHECK_PER_TICK`。
const BLOCKS_CHECK_PER_TICK: usize = 10;

/// 官方 `BeaconBlockEntity.BEACON_EFFECTS`：按层数分档的合法效果。
///
/// Official `BeaconBlockEntity.BEACON_EFFECTS`: valid effects per pyramid tier.
pub const BEACON_EFFECTS: [&[i32]; 4] = [
    &[mob_effect::SPEED, mob_effect::HASTE],
    &[mob_effect::RESISTANCE, mob_effect::JUMP_BOOST],
    &[mob_effect::STRENGTH],
    &[mob_effect::REGENERATION],
];

/// 一段同色光柱，对应官方 `BeaconBeamOwner.Section`。
///
/// One monochrome segment of a beacon beam (`BeaconBeamOwner.Section`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeaconSection {
    /// 不透明 ARGB 颜色（`0xFF_RRGGBB`）。
    pub color: i32,
    /// 段高（格数，构造时为 1）。
    pub height: i32,
}

impl BeaconSection {
    /// 构造一段高度为 1 的光柱段。
    pub const fn new(color: i32) -> Self {
        Self { color, height: 1 }
    }
}

/// 官方 `BeaconBlockEntity`。
///
/// `CustomName` 和 `lock` 由游戏层的 `block_items` 组件表管理，不放在这里。
///
/// Official `BeaconBlockEntity`. `CustomName` and `lock` live in the game layer's
/// `block_items` component table rather than on this struct.
#[derive(Debug, Clone)]
pub struct BeaconBlockEntity {
    pos: BlockPos,
    beam_sections: Vec<BeaconSection>,
    checking_beam_sections: Vec<BeaconSection>,
    levels: i32,
    last_check_y: i32,
    primary_power: Option<i32>,
    secondary_power: Option<i32>,
}

impl BeaconBlockEntity {
    /// 新建一个初始态信标方块实体。
    ///
    /// 官方在 `setLevel` 里把 `lastCheckY` 设为 `level.getMinY() - 1`。这里用 `i32::MIN`
    /// 占位，首次 `tick` 时同样满足 `last_check_y < pos.y`，从信标所在格开始扫。
    ///
    /// Create a fresh beacon block entity. Vanilla sets `lastCheckY = level.getMinY() - 1`
    /// in `setLevel`; `i32::MIN` satisfies `last_check_y < pos.y` on the first tick.
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            beam_sections: Vec::new(),
            checking_beam_sections: Vec::new(),
            levels: 0,
            last_check_y: i32::MIN,
            primary_power: None,
            secondary_power: None,
        }
    }

    /// 官方 `BeaconBlockEntity.loadAdditional`：读 `primary_effect` 与 `secondary_effect`
    /// 并按 `VALID_EFFECTS` 过滤。`Levels` 不读盘，保持构造器的 0。
    ///
    /// Official `BeaconBlockEntity.loadAdditional`: reads and filters `primary_effect` and
    /// `secondary_effect`. `Levels` is not loaded and remains 0.
    pub fn read(pos: BlockPos, raw: &Tag) -> Self {
        let mut entity = Self::new(pos);
        entity.primary_power = load_effect(raw, "primary_effect");
        entity.secondary_power = load_effect(raw, "secondary_effect");
        entity
    }

    /// 官方 `BeaconBlockEntity.saveAdditional` 中由方块实体自己持有的三个键：
    /// `primary_effect`、`secondary_effect`、`Levels`。
    ///
    /// The three `saveAdditional` keys owned by the block entity itself:
    /// `primary_effect`, `secondary_effect`, and `Levels`.
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        if let Some(name) = self.primary_power.and_then(effect_name) {
            out.push(("primary_effect".to_owned(), Tag::String(name.to_owned())));
        }
        if let Some(name) = self.secondary_power.and_then(effect_name) {
            out.push(("secondary_effect".to_owned(), Tag::String(name.to_owned())));
        }
        out.push(("Levels".to_owned(), Tag::Int(self.levels)));
        out
    }

    /// 塔层数（0–4）。
    pub const fn levels(&self) -> i32 {
        self.levels
    }

    /// 主效果的 `mob_effect` 注册表 ID。
    pub const fn primary_power(&self) -> Option<i32> {
        self.primary_power
    }

    /// 副效果的 `mob_effect` 注册表 ID。
    pub const fn secondary_power(&self) -> Option<i32> {
        self.secondary_power
    }

    /// 官方 `BeaconBlockEntity.getBeamSections()`：层数为 0 时返回空切片。
    ///
    /// Official `getBeamSections()`: empty when `levels == 0`.
    pub fn beam_sections(&self) -> &[BeaconSection] {
        if self.levels == 0 {
            &[]
        } else {
            &self.beam_sections
        }
    }

    /// 内部已提交的光柱段列表（不含 `levels == 0` 过滤），供对拍和测试检查。
    ///
    /// Committed `beamSections` regardless of `levels`, for tests and oracle checks.
    pub fn raw_beam_sections(&self) -> &[BeaconSection] {
        &self.beam_sections
    }

    /// 当前正在扫描中的光柱段列表。
    pub fn checking_beam_sections(&self) -> &[BeaconSection] {
        &self.checking_beam_sections
    }

    /// 上一次扫描到达的 y 坐标。
    pub const fn last_check_y(&self) -> i32 {
        self.last_check_y
    }

    /// 官方 `ContainerData.get(id)`。
    pub fn data(&self, id: usize) -> i32 {
        match id {
            DATA_LEVELS => self.levels,
            DATA_PRIMARY => encode_effect(self.primary_power),
            DATA_SECONDARY => encode_effect(self.secondary_power),
            _ => 0,
        }
    }

    /// 菜单读到的全部数据槽 `[levels, encoded_primary, encoded_secondary]`。
    pub fn data_values(&self) -> [i32; NUM_DATA_VALUES] {
        [
            self.levels,
            encode_effect(self.primary_power),
            encode_effect(self.secondary_power),
        ]
    }

    /// 官方 `ContainerData.set(id, value)`。
    ///
    /// 服务端写 `DATA_PRIMARY` 且 `beam_sections` 非空时播放 `block.beacon.power_select`。
    /// 传入 `world = None` 时只写字段、不播音效（如存档/夹具初始化）。
    ///
    /// Official `ContainerData.set(id, value)`. Writing `DATA_PRIMARY` with a non-empty
    /// `beam_sections` plays `block.beacon.power_select` when `world` is provided.
    pub fn set_data(&mut self, id: usize, value: i32, world: Option<&mut World>) {
        match id {
            DATA_LEVELS => self.levels = value,
            DATA_PRIMARY => {
                if let Some(world) = world
                    && !self.beam_sections.is_empty()
                {
                    play_sound(world, self.pos, "minecraft:block.beacon.power_select");
                }
                self.primary_power = filter_effect(decode_effect(value));
            }
            DATA_SECONDARY => {
                self.secondary_power = filter_effect(decode_effect(value));
            }
            _ => {}
        }
    }

    /// 官方 `BeaconBlockEntity.tick(Level, BlockPos, BlockState, BeaconBlockEntity)`。
    pub fn server_tick(&mut self, world: &mut World) {
        let x = self.pos.x;
        let y = self.pos.y;
        let z = self.pos.z;

        let mut check_pos = if self.last_check_y < y {
            self.checking_beam_sections.clear();
            self.last_check_y = y - 1;
            self.pos
        } else {
            BlockPos::new(x, self.last_check_y + 1, z)
        };

        let world_surface = world.world_surface_y(BlockPos::new(x, 0, z));

        for _ in 0..BLOCKS_CHECK_PER_TICK {
            if check_pos.y > world_surface {
                break;
            }
            let state = world.block_state(check_pos);
            if let Some(dye) = beam_color(state) {
                let color = dye.texture_diffuse_color();
                if self.checking_beam_sections.len() <= 1 {
                    self.checking_beam_sections.push(BeaconSection::new(color));
                } else if let Some(last) = self.checking_beam_sections.last_mut() {
                    if color == last.color {
                        last.height += 1;
                    } else {
                        let mixed = average_argb(last.color, color);
                        self.checking_beam_sections.push(BeaconSection::new(mixed));
                    }
                }
            } else if let Some(last) = self.checking_beam_sections.last_mut()
                && (state.light_dampening() < 15 || state.is(blocks::BEDROCK))
            {
                last.height += 1;
            } else {
                self.checking_beam_sections.clear();
                self.last_check_y = world_surface;
                break;
            }
            check_pos = check_pos.above();
            self.last_check_y += 1;
        }

        let prev_levels = self.levels;
        if world.game_time() % 80 == 0 {
            if !self.beam_sections.is_empty() {
                self.levels = update_base(world, x, y, z);
            }
            if self.levels > 0 && !self.beam_sections.is_empty() {
                apply_effects(
                    world,
                    self.pos,
                    self.levels,
                    self.primary_power,
                    self.secondary_power,
                );
                play_sound(world, self.pos, "minecraft:block.beacon.ambient");
            }
        }

        if self.last_check_y >= world_surface {
            self.last_check_y = world.min_y() - 1;
            let was_active = prev_levels > 0;
            self.beam_sections = std::mem::take(&mut self.checking_beam_sections);
            let is_active = self.levels > 0;
            if !was_active && is_active {
                play_sound(world, self.pos, "minecraft:block.beacon.activate");
                // 官方 `new AABB(x, y, z, x, y - 4, z).inflate(10.0, 5.0, 10.0)`：
                // 构造器把 y 和 y - 4 排序为 [y - 4, y]，膨胀后是 [x-10, y-9, z-10]..[x+10, y+5, z+10]。
                let area = Aabb::new(
                    f64::from(x) - 10.0,
                    f64::from(y - 4) - 5.0,
                    f64::from(z) - 10.0,
                    f64::from(x) + 10.0,
                    f64::from(y) + 5.0,
                    f64::from(z) + 10.0,
                );
                let players = world.player_views();
                for player in players {
                    if !player.spectator && player_bounds(&player).intersects(area) {
                        world.push_effect(Effect::ConstructBeacon {
                            target: player.uid,
                            levels: self.levels,
                        });
                    }
                }
            } else if was_active && !is_active {
                play_sound(world, self.pos, "minecraft:block.beacon.deactivate");
            }
        }
    }
}

impl BlockEntity for BeaconBlockEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        Ok(self.additional())
    }
}

/// 官方 `BeaconBlockEntity.updateBase`：检查信标下方 1–4 层是否全部为 `#beacon_base_blocks`。
///
/// Official `BeaconBlockEntity.updateBase`: checks layers 1..=4 below the beacon for
/// `#minecraft:beacon_base_blocks`.
pub fn update_base(world: &World, x: i32, y: i32, z: i32) -> i32 {
    let mut levels = 0;
    let min_y = world.min_y();
    for layer in 1..=MAX_LEVELS {
        let cy = y - layer;
        if cy < min_y {
            break;
        }
        let mut complete = true;
        'rows: for cx in (x - layer)..=(x + layer) {
            for cz in (z - layer)..=(z + layer) {
                if !world
                    .block_state(BlockPos::new(cx, cy, cz))
                    .is_tagged(block_tags::BEACON_BASE_BLOCKS)
                {
                    complete = false;
                    break 'rows;
                }
            }
        }
        if !complete {
            break;
        }
        levels = layer;
    }
    levels
}

/// 官方 `BeaconBlockEntity.applyEffects`。
fn apply_effects(
    world: &mut World,
    pos: BlockPos,
    levels: i32,
    primary: Option<i32>,
    secondary: Option<i32>,
) {
    let Some(primary) = primary else {
        return;
    };
    let radius = f64::from(levels * 10 + 10);
    let primary_amplifier = if levels >= LEVELS_NEEDED_FOR_SECONDARY && secondary == Some(primary) {
        1
    } else {
        0
    };
    let duration = (9 + levels * 2) * 20;
    let world_height = f64::from(world.max_y() - world.min_y() + 1);
    let area = Aabb::new(
        f64::from(pos.x) - radius,
        f64::from(pos.y) - radius,
        f64::from(pos.z) - radius,
        f64::from(pos.x + 1) + radius,
        f64::from(pos.y + 1) + radius + world_height,
        f64::from(pos.z + 1) + radius,
    );
    let targets: Vec<i32> = world
        .player_views()
        .into_iter()
        .filter(|player| !player.spectator && player_bounds(player).intersects(area))
        .map(|player| player.uid)
        .collect();
    for &target in &targets {
        world.push_effect(Effect::PlayerEffect {
            target,
            effect: primary,
            duration,
            amplifier: primary_amplifier,
            ambient: true,
            visible: true,
            show_icon: true,
        });
    }
    if levels >= LEVELS_NEEDED_FOR_SECONDARY
        && let Some(secondary) = secondary
        && secondary != primary
    {
        for &target in &targets {
            world.push_effect(Effect::PlayerEffect {
                target,
                effect: secondary,
                duration,
                amplifier: 0,
                ambient: true,
                visible: true,
                show_icon: true,
            });
        }
    }
}

/// 官方 `BeaconBlockEntity.playSound`：方块中心、BLOCKS 声道、音量 1、音高 1。
fn play_sound(world: &mut World, pos: BlockPos, name: &str) {
    world.play_block_sound(pos, sound_event(name), 1.0, 1.0);
}

/// 玩家在 `EntityDimensions.makeBoundingBox(position)` 下的碰撞盒。
fn player_bounds(player: &PlayerView) -> Aabb {
    let half = f64::from(player.width) / 2.0;
    Aabb::new(
        player.position.x - half,
        player.position.y,
        player.position.z - half,
        player.position.x + half,
        player.position.y + f64::from(player.height),
        player.position.z + half,
    )
}

/// 这个效果是不是信标允许的六种效果之一，官方 `VALID_EFFECTS.contains`。
pub fn is_valid_effect(effect: i32) -> bool {
    BEACON_EFFECTS.iter().any(|tier| tier.contains(&effect))
}

/// 官方 `BeaconBlockEntity.filterEffect`。
pub fn filter_effect(effect: Option<i32>) -> Option<i32> {
    effect.filter(|&id| is_valid_effect(id))
}

/// 官方 `BeaconBlockEntity.getRequiredLevelsFor`。
pub fn required_levels(effect: Option<i32>) -> i32 {
    let Some(effect) = effect else {
        return 0;
    };
    for (index, tier) in BEACON_EFFECTS.iter().enumerate() {
        if tier.contains(&effect) {
            return index as i32 + 1;
        }
    }
    i32::MAX
}

/// 官方 `BeaconBlockEntity.validateEffects(primary, secondary, levels)`。
///
/// 校验客户端发来的主/副效果组合对当前塔层数是否合法。当 `primary` 为 `None`
/// 且 `secondary` 为 1–3 层效果时返回 `false`（官方 Java 会在 `primary.equals(secondary)`
/// 处抛 NPE，这里按非法拒绝而不 panic）。
///
/// Official `BeaconBlockEntity.validateEffects(primary, secondary, levels)`. Returns `false`
/// instead of panicking when `primary` is `None` and `secondary` is a tier 1–3 effect.
pub fn validate_effects(primary: Option<i32>, secondary: Option<i32>, levels: i32) -> bool {
    if secondary.is_some() && levels < LEVELS_NEEDED_FOR_SECONDARY {
        return false;
    }
    let req_primary = required_levels(primary);
    let req_secondary = required_levels(secondary);
    if req_primary > levels || req_secondary > levels {
        return false;
    }
    if req_primary >= LEVELS_NEEDED_FOR_SECONDARY {
        return false;
    }
    if req_secondary != 0 && req_secondary < LEVELS_NEEDED_FOR_SECONDARY && primary != secondary {
        return false;
    }
    true
}

/// 官方 `BeaconMenu.encodeEffect`：`None => 0`，有效果时 `mob_effect_id + 1`。
pub const fn encode_effect(effect: Option<i32>) -> i32 {
    match effect {
        None => 0,
        Some(id) => id + 1,
    }
}

/// 官方 `BeaconMenu.decodeEffect`：`0 => None`，其余查 `mob_effect` 注册表。
pub fn decode_effect(value: i32) -> Option<i32> {
    if value <= 0 {
        return None;
    }
    let id = value - 1;
    (u32::try_from(id)
        .ok()
        .is_some_and(|n| n < mob_effect::TOTAL))
    .then_some(id)
}

/// 官方 `ARGB.average(a, b)`：四个 8 位通道分别取整数均值（向零截断）。
pub const fn average_argb(a: i32, b: i32) -> i32 {
    let a = a as u32;
    let b = b as u32;
    let alpha = ((a >> 24) + (b >> 24)) / 2;
    let red = (((a >> 16) & 0xFF) + ((b >> 16) & 0xFF)) / 2;
    let green = (((a >> 8) & 0xFF) + ((b >> 8) & 0xFF)) / 2;
    let blue = ((a & 0xFF) + (b & 0xFF)) / 2;
    ((alpha << 24) | (red << 16) | (green << 8) | blue) as i32
}

/// 官方 `BeaconBeamBlock.getColor()`：只有 `BeaconBlock`、`StainedGlassBlock` 和
/// `StainedGlassPaneBlock` 三个类实现它。
///
/// Official `BeaconBeamBlock.getColor()`, implemented by `BeaconBlock`, `StainedGlassBlock`,
/// and `StainedGlassPaneBlock`.
pub fn beam_color(state: BlockState) -> Option<DyeColor> {
    match state.class() {
        BlockClass::BeaconBlock => Some(DyeColor::White),
        BlockClass::StainedGlassBlock => stained_glass_color(state.block_id()),
        BlockClass::StainedGlassPaneBlock => stained_glass_pane_color(state.block_id()),
        _ => None,
    }
}

fn stained_glass_color(block_id: usize) -> Option<DyeColor> {
    let id = |s: u32| BlockState(s).block_id();
    Some(match block_id {
        x if x == id(blocks::WHITE_STAINED_GLASS) => DyeColor::White,
        x if x == id(blocks::ORANGE_STAINED_GLASS) => DyeColor::Orange,
        x if x == id(blocks::MAGENTA_STAINED_GLASS) => DyeColor::Magenta,
        x if x == id(blocks::LIGHT_BLUE_STAINED_GLASS) => DyeColor::LightBlue,
        x if x == id(blocks::YELLOW_STAINED_GLASS) => DyeColor::Yellow,
        x if x == id(blocks::LIME_STAINED_GLASS) => DyeColor::Lime,
        x if x == id(blocks::PINK_STAINED_GLASS) => DyeColor::Pink,
        x if x == id(blocks::GRAY_STAINED_GLASS) => DyeColor::Gray,
        x if x == id(blocks::LIGHT_GRAY_STAINED_GLASS) => DyeColor::LightGray,
        x if x == id(blocks::CYAN_STAINED_GLASS) => DyeColor::Cyan,
        x if x == id(blocks::PURPLE_STAINED_GLASS) => DyeColor::Purple,
        x if x == id(blocks::BLUE_STAINED_GLASS) => DyeColor::Blue,
        x if x == id(blocks::BROWN_STAINED_GLASS) => DyeColor::Brown,
        x if x == id(blocks::GREEN_STAINED_GLASS) => DyeColor::Green,
        x if x == id(blocks::RED_STAINED_GLASS) => DyeColor::Red,
        x if x == id(blocks::BLACK_STAINED_GLASS) => DyeColor::Black,
        _ => return None,
    })
}

fn stained_glass_pane_color(block_id: usize) -> Option<DyeColor> {
    let id = |s: u32| BlockState(s).block_id();
    Some(match block_id {
        x if x == id(blocks::WHITE_STAINED_GLASS_PANE) => DyeColor::White,
        x if x == id(blocks::ORANGE_STAINED_GLASS_PANE) => DyeColor::Orange,
        x if x == id(blocks::MAGENTA_STAINED_GLASS_PANE) => DyeColor::Magenta,
        x if x == id(blocks::LIGHT_BLUE_STAINED_GLASS_PANE) => DyeColor::LightBlue,
        x if x == id(blocks::YELLOW_STAINED_GLASS_PANE) => DyeColor::Yellow,
        x if x == id(blocks::LIME_STAINED_GLASS_PANE) => DyeColor::Lime,
        x if x == id(blocks::PINK_STAINED_GLASS_PANE) => DyeColor::Pink,
        x if x == id(blocks::GRAY_STAINED_GLASS_PANE) => DyeColor::Gray,
        x if x == id(blocks::LIGHT_GRAY_STAINED_GLASS_PANE) => DyeColor::LightGray,
        x if x == id(blocks::CYAN_STAINED_GLASS_PANE) => DyeColor::Cyan,
        x if x == id(blocks::PURPLE_STAINED_GLASS_PANE) => DyeColor::Purple,
        x if x == id(blocks::BLUE_STAINED_GLASS_PANE) => DyeColor::Blue,
        x if x == id(blocks::BROWN_STAINED_GLASS_PANE) => DyeColor::Brown,
        x if x == id(blocks::GREEN_STAINED_GLASS_PANE) => DyeColor::Green,
        x if x == id(blocks::RED_STAINED_GLASS_PANE) => DyeColor::Red,
        x if x == id(blocks::BLACK_STAINED_GLASS_PANE) => DyeColor::Black,
        _ => return None,
    })
}

fn effect_name(effect: i32) -> Option<&'static str> {
    mob_effect::NAMES
        .get(usize::try_from(effect).ok()?)
        .copied()
}

/// 官方 `BeaconBlockEntity.loadEffect`：按注册表名解析并用 `VALID_EFFECTS` 过滤。
fn load_effect(raw: &Tag, key: &str) -> Option<i32> {
    let Some(Tag::String(name)) = tag::field(raw, key) else {
        return None;
    };
    let qualified = if name.contains(':') {
        name.as_str()
    } else {
        // 官方 `Identifier` 默认命名空间 `minecraft`。
        return mob_effect::by_name(&format!("minecraft:{name}")).filter(|&id| is_valid_effect(id));
    };
    mob_effect::by_name(qualified).filter(|&id| is_valid_effect(id))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::{testing::TestLevel, ticks::BlockTicks};

    const ORIGIN: BlockPos = BlockPos::new(0, 64, 0);

    fn build_pyramid(level: &mut TestLevel, pos: BlockPos, layers: i32, block: u32) {
        for layer in 1..=layers {
            let cy = pos.y - layer;
            for cx in (pos.x - layer)..=(pos.x + layer) {
                for cz in (pos.z - layer)..=(pos.z + layer) {
                    level.set(BlockPos::new(cx, cy, cz), BlockState(block));
                }
            }
        }
    }

    fn test_player(uid: i32, pos: Vec3, spectator: bool) -> PlayerView {
        PlayerView {
            uid,
            uuid: None,
            fall_flying: false,
            main_arm_right: true,
            position: pos,
            delta: Vec3::new(0.0, 0.0, 0.0),
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::new(0.0, 0.0, 1.0),
            gaze_disguise: false,
            alive: true,
            spectator,
            creative: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            game_mode: if spectator { 3 } else { 0 },
            mining_fatigue: None,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            yaw: 0.0,
            pitch: 0.0,
            combat: Default::default(),
            effects: Default::default(),
            shoulder_left: None,
            shoulder_right: None,
        }
    }

    fn advance_ticks(_level: &mut TestLevel, ticks: &mut BlockTicks, count: usize) {
        let next = ticks.now() + count as i64;
        ticks.set_time(next);
    }

    #[test]
    fn beam_sections_tint_and_block_like_vanilla() {
        let mut level = TestLevel::new();
        level.set(ORIGIN, BlockState(blocks::BEACON));
        // 第 1 块染色玻璃：直接设为红色，不和白色平均。
        level.set(ORIGIN.above(), BlockState(blocks::RED_STAINED_GLASS));
        // 紧接着的同色玻璃板：延长当前段。
        level.set(
            BlockPos::new(0, 66, 0),
            BlockState(blocks::RED_STAINED_GLASS_PANE),
        );
        // 第 2 块不同色玻璃：与红做 ARGB 均值。
        level.set(
            BlockPos::new(0, 67, 0),
            BlockState(blocks::BLUE_STAINED_GLASS),
        );
        // 基岩不挡光，延长蓝色混合段。
        level.set(BlockPos::new(0, 68, 0), BlockState(blocks::BEDROCK));

        let mut ticks = BlockTicks::new();
        let mut beacon = BeaconBlockEntity::new(ORIGIN);
        let mut world = World::new(&mut level, &mut ticks);
        beacon.server_tick(&mut world);

        let white = DyeColor::White.texture_diffuse_color();
        let red = DyeColor::Red.texture_diffuse_color();
        let blue = DyeColor::Blue.texture_diffuse_color();
        let mixed = average_argb(red, blue);
        // world_surface = 69（基岩 68 上方一格），扫描 64..=69 共 6 格：
        // 64: beacon -> [White(1)]
        // 65: red glass (len <= 1) -> [White(1), Red(1)]
        // 66: red pane (same color) -> [White(1), Red(2)]
        // 67: blue glass -> [White(1), Red(2), mixed(1)]
        // 68: bedrock -> mixed.height = 2
        // 69: air (world_surface) -> mixed.height = 3
        assert_eq!(
            beacon.raw_beam_sections(),
            &[
                BeaconSection {
                    color: white,
                    height: 1,
                },
                BeaconSection {
                    color: red,
                    height: 2,
                },
                BeaconSection {
                    color: mixed,
                    height: 3,
                },
            ]
        );
        // levels 为 0 时 getBeamSections() 返回空。
        assert!(beacon.beam_sections().is_empty());

        // 头顶放一块不透明石头：下一轮扫描清空光柱。
        world.set_block(BlockPos::new(0, 70, 0), BlockState(blocks::STONE), 0);
        beacon.server_tick(&mut world);
        assert!(beacon.raw_beam_sections().is_empty());
    }

    #[test]
    fn pyramid_levels_and_effects_apply_every_80_ticks() {
        let mut level = TestLevel::new();
        level.set(ORIGIN, BlockState(blocks::BEACON));
        build_pyramid(&mut level, ORIGIN, 4, blocks::IRON_BLOCK);
        // 范围内玩家、旁观者、范围外玩家。
        level
            .players
            .push(test_player(1, Vec3::new(50.0, 64.0, 0.0), false));
        level
            .players
            .push(test_player(2, Vec3::new(2.0, 64.0, 0.0), true));
        level
            .players
            .push(test_player(3, Vec3::new(60.0, 64.0, 0.0), false));

        let mut ticks = BlockTicks::new();
        let mut beacon = BeaconBlockEntity::new(ORIGIN);
        // 第 1 刻（gameTime = 0）：此时 beam_sections 还是空的，不会算底座；扫完后提交 beam_sections。
        {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.server_tick(&mut world);
            assert_eq!(beacon.levels(), 0);
            assert!(!beacon.raw_beam_sections().is_empty());
        }

        // 设置主效果 SPEED + 副效果 SPEED（4 层应给 SPEED II），再推进到 gameTime = 80。
        {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.set_data(
                DATA_PRIMARY,
                encode_effect(Some(mob_effect::SPEED)),
                Some(&mut world),
            );
            beacon.set_data(
                DATA_SECONDARY,
                encode_effect(Some(mob_effect::SPEED)),
                Some(&mut world),
            );
            let effects = world.drain_effects();
            assert!(effects.iter().any(|e| matches!(
                e,
                Effect::Sound { sound, .. } if *sound == i32::from(sound_event("minecraft:block.beacon.power_select"))
            )));
        }

        advance_ticks(&mut level, &mut ticks, 80);
        let effects = {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.server_tick(&mut world);
            world.drain_effects()
        };

        assert_eq!(beacon.levels(), 4);
        assert_eq!(beacon.beam_sections().len(), 1);
        // 玩家 1 在 50 格内（levels*10+10 = 50，AABB maxX = 51，玩家 minX = 49.7）拿到 SPEED II 340 刻。
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::PlayerEffect {
                target: 1,
                effect,
                duration: 340,
                amplifier: 1,
                ambient: true,
                visible: true,
                show_icon: true,
            } if *effect == mob_effect::SPEED
        )));
        // 旁观者（2）和超距玩家（3）都没有效果。
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::PlayerEffect { target: 2 | 3, .. }))
        );
        // 播了环境音和激活音。
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event("minecraft:block.beacon.ambient"))
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event("minecraft:block.beacon.activate"))
        )));
        // 玩家 1 不在构造进度的 10 格范围内，没有收到 ConstructBeacon。
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::ConstructBeacon { .. }))
        );

        // 拆掉第一层一块铁块，再推进到 gameTime = 160 -> 降为 0 层并播放失效音效。
        level.set(BlockPos::new(0, 63, 0), BlockState(blocks::AIR));
        advance_ticks(&mut level, &mut ticks, 80);
        let effects = {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.server_tick(&mut world);
            world.drain_effects()
        };
        assert_eq!(beacon.levels(), 0);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event("minecraft:block.beacon.deactivate"))
        )));
    }

    #[test]
    fn secondary_regeneration_grants_both_effects_and_construct_trigger() {
        let mut level = TestLevel::new();
        level.set(ORIGIN, BlockState(blocks::BEACON));
        build_pyramid(&mut level, ORIGIN, 4, blocks::DIAMOND_BLOCK);
        level
            .players
            .push(test_player(7, Vec3::new(3.0, 64.0, 0.0), false));

        let mut ticks = BlockTicks::new();
        let mut beacon = BeaconBlockEntity::new(ORIGIN);
        {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.server_tick(&mut world);
            beacon.set_data(DATA_PRIMARY, encode_effect(Some(mob_effect::HASTE)), None);
            beacon.set_data(
                DATA_SECONDARY,
                encode_effect(Some(mob_effect::REGENERATION)),
                None,
            );
        }

        advance_ticks(&mut level, &mut ticks, 80);
        let effects = {
            let mut world = World::new(&mut level, &mut ticks);
            beacon.server_tick(&mut world);
            world.drain_effects()
        };

        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::PlayerEffect { target: 7, effect, amplifier: 0, duration: 340, .. }
                if *effect == mob_effect::HASTE
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::PlayerEffect { target: 7, effect, amplifier: 0, duration: 340, .. }
                if *effect == mob_effect::REGENERATION
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::ConstructBeacon {
                target: 7,
                levels: 4
            }
        )));
    }

    #[test]
    fn validate_effects_matches_official_tier_rules() {
        // 层数不够拒绝。
        assert!(!validate_effects(Some(mob_effect::SPEED), None, 0));
        assert!(validate_effects(Some(mob_effect::SPEED), None, 1));
        assert!(!validate_effects(Some(mob_effect::RESISTANCE), None, 1));
        assert!(validate_effects(Some(mob_effect::RESISTANCE), None, 2));
        assert!(!validate_effects(Some(mob_effect::STRENGTH), None, 2));
        assert!(validate_effects(Some(mob_effect::STRENGTH), None, 3));
        // Regeneration 不能当主效果，即使 4 层也不行。
        assert!(!validate_effects(Some(mob_effect::REGENERATION), None, 4));
        // 副效果需要 4 层。
        assert!(!validate_effects(
            Some(mob_effect::SPEED),
            Some(mob_effect::SPEED),
            3
        ));
        assert!(validate_effects(
            Some(mob_effect::SPEED),
            Some(mob_effect::SPEED),
            4
        ));
        assert!(validate_effects(
            Some(mob_effect::SPEED),
            Some(mob_effect::REGENERATION),
            4
        ));
        // 副效果不能是与主效果不同的 1–3 层效果。
        assert!(!validate_effects(
            Some(mob_effect::SPEED),
            Some(mob_effect::HASTE),
            4
        ));
        // 主效果为空时：副效果为空或 Regeneration 允许，其余 1–3 层拒绝（不 panic）。
        assert!(validate_effects(None, None, 0));
        assert!(validate_effects(None, Some(mob_effect::REGENERATION), 4));
        assert!(!validate_effects(None, Some(mob_effect::SPEED), 4));
        // 非信标效果一律拒绝。
        assert!(!validate_effects(Some(mob_effect::POISON), None, 4));
        assert!(!validate_effects(
            Some(mob_effect::SPEED),
            Some(mob_effect::POISON),
            4
        ));
    }

    #[test]
    fn archive_round_trip_filters_invalid_effects_and_ignores_saved_levels() {
        let raw = Tag::Compound(vec![
            (
                "primary_effect".to_owned(),
                Tag::String("minecraft:speed".to_owned()),
            ),
            (
                "secondary_effect".to_owned(),
                Tag::String("minecraft:poison".to_owned()),
            ),
            ("Levels".to_owned(), Tag::Int(4)),
        ]);
        let entity = BeaconBlockEntity::read(ORIGIN, &raw);
        assert_eq!(entity.primary_power(), Some(mob_effect::SPEED));
        assert_eq!(entity.secondary_power(), None);
        assert_eq!(entity.levels(), 0);
        assert_eq!(
            entity.additional(),
            vec![
                (
                    "primary_effect".to_owned(),
                    Tag::String("minecraft:speed".to_owned())
                ),
                ("Levels".to_owned(), Tag::Int(0)),
            ]
        );
    }
}
