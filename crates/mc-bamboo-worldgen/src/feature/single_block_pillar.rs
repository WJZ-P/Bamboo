//! `SingleBlockPillarFeature`：沿竖直方向连续放同一处方块。
//!
//! **已知限制**：`cap_feature`（顶盖）还没接——它要递归走 `PlacedFeature.place`，
//! 需要 `PlacementEnv`，而当前叶子接口不传。带 `cap_feature` 的配置解析时返回
//! `None`（标 unsupported）。26.3 里只有 `minecraft:basalt_pillar` 带它，
//! 那条在下界群系，不在本轮主世界对拍范围。
//!
//! **Known limit**: `cap_feature` is not wired yet; it needs a recursive
//! `PlacedFeature.place` with a `PlacementEnv` that the leaf interface does not carry.
//! Configs with `cap_feature` parse to `None` (unsupported). Only
//! `minecraft:basalt_pillar` uses it in 26.3, and that lives in nether biomes.
//!
//! 官方 `SingleBlockPillarFeature.place`：从 `origin` 起朝 `direction`（`up` 或 `down`）
//! 逐格放，条件是 `canReplace.test(level, pos) && random.nextFloat() < chanceToContinue`
//! 且没超出建造高度。循环结束后回退一格，若配置了 `cap_feature` 就在那里放一次。
//! `place` 恒返回 true。
//!
//! Official `SingleBlockPillarFeature.place`: from `origin`, step along `direction`
//! (`up` or `down`) placing blocks while `canReplace.test(level, pos)`,
//! `random.nextFloat() < chanceToContinue` and the position stays inside the build
//! height. Afterwards it steps back one block and places `cap_feature` there if present.
//! `place` always returns true.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{block_predicate::BlockPredicate, state_provider::StateProvider, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// 已解析的 `SingleBlockPillarFeature` 配置。
pub struct SingleBlockPillarFeature {
    block: StateProvider,
    can_replace: BlockPredicate,
    /// `true` 表示 `Direction.UP`，`false` 表示 `Direction.DOWN`（`VERTICAL_CODEC` 只允许这两个）。
    up: bool,
    chance_to_continue: f32,
}

impl SingleBlockPillarFeature {
    /// 解析配置；`direction` 必须是 `up` 或 `down`。
    ///
    /// Parse the config; `direction` must be `up` or `down`.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let up = match object.get("direction")?.as_str()? {
            "up" => true,
            "down" => false,
            _ => return None,
        };
        let block = StateProvider::parse(object.get("block")?)?;
        if !block.is_supported() {
            return None;
        }
        let can_replace = match object.get("can_replace") {
            Some(value) => BlockPredicate::parse(value),
            None => BlockPredicate::True,
        };
        if !can_replace.is_supported() {
            return None;
        }
        // 带 `cap_feature` 的配置还没移植（见模块文档）。
        // Configs with a `cap_feature` stay unported (see the module docs).
        if object.contains_key("cap_feature") {
            return None;
        }
        Some(Self {
            block,
            can_replace,
            up,
            chance_to_continue: object
                .get("chance_to_continue")
                .and_then(Value::as_f64)
                .unwrap_or(1.0) as f32,
        })
    }

    /// 官方 `SingleBlockPillarFeature.place`：一个候选位置。
    ///
    /// Official `SingleBlockPillarFeature.place` at one candidate.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        let mut pos = origin;
        while self.can_replace.test(world, pos)
            && random.next_float() < self.chance_to_continue
            && !is_outside_build_height(world, pos)
        {
            let Some(state) = self.block.get_state(world, random, pos) else {
                break;
            };
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
            pos = step(pos, self.up);
        }
        // 官方这里会回退一格并放 `cap_feature`；`cap_feature` 未移植，
        // 所以带它的配置根本不会走到这里（`parse` 返回 None）。
        // The official code steps back one block and places `cap_feature`; that is
        // unported, so such configs never reach here (`parse` returns None).
        let _ = step(pos, !self.up);
        true
    }
}

/// 朝 `up` / `down` 走一格。
///
/// Step one block up or down.
fn step(pos: Pos, up: bool) -> Pos {
    (pos.0, pos.1 + if up { 1 } else { -1 }, pos.2)
}

/// `LevelHeightAccessor.isOutsideBuildHeight(pos)`。
fn is_outside_build_height(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    pos.1 < world.min_y || pos.1 >= world.min_y + world.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_exported_basalt_pillar_config() {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let config = &root["feature"]["minecraft:basalt_pillar"];
        // `basalt_pillar` 顶层是 overlay，子项才是 single_block_pillar。
        let inner = &config["features"][0]["feature"];
        assert_eq!(inner["type"], "minecraft:single_block_pillar");
        // `basalt_pillar` 带 `cap_feature`，本轮标 unsupported。
        // `basalt_pillar` carries a `cap_feature`, so it stays unsupported for now.
        assert!(SingleBlockPillarFeature::parse(inner).is_none());
        let plain: Value = json!({
            "block": {"id": "minecraft:basalt"},
            "can_replace": {"tag": "minecraft:air", "type": "minecraft:matching_block_tag"},
            "direction": "up",
            "type": "minecraft:single_block_pillar",
        });
        let pillar = SingleBlockPillarFeature::parse(&plain).unwrap();
        assert!(pillar.up);
    }

    #[test]
    fn rejects_non_vertical_directions() {
        let sideways: Value = json!({
            "block": {"id": "minecraft:basalt"},
            "direction": "north",
            "type": "minecraft:single_block_pillar",
        });
        assert!(SingleBlockPillarFeature::parse(&sideways).is_none());
    }
}
