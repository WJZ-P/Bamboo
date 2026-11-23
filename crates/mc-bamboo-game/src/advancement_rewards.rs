//! 进度完成时的奖励，官方 `AdvancementRewards.grant`。
//!
//! 经验、战利品表、配方解锁、函数。函数系统 Bamboo 还没有，见文档已知差异。

use mc_bamboo_advancements::AdvancementTree;

use crate::{inbound::PlayerId, region::Region};

/// 发一个进度的全部奖励。只在进度从「未完成」变「完成」的那一刻调用。
pub(crate) fn grant(region: &mut Region, player: PlayerId, index: usize) {
    let tree = AdvancementTree::global();
    let rewards = tree.nodes[index].advancement.rewards.clone();
    // 官方 `AdvancementRewards.grant` 的顺序：经验 → 战利品表 → 配方 → 函数。
    if rewards.experience > 0
        && let Some(target) = region.players.get_mut(&player)
    {
        // 官方 `AdvancementRewards.grant`：直接 `giveExperiencePoints`，不发包广播反馈。
        target.give_experience_points(rewards.experience);
    }
    // 战利品表奖励：`rewards.loot` 是 HolderSet，Bamboo 的战利品表求值入口在
    // `mc-bamboo-loot`；目前内置进度没有 loot 奖励（导出里全是 recipes / experience），
    // 这里保留接口，见文档已知差异。
    if !rewards.recipes.is_empty()
        && let Some(player) = region.players.get_mut(&player)
    {
        for recipe in &rewards.recipes {
            player.advancements.known_recipes.insert(recipe.clone());
        }
    }
    // Bamboo 没有函数系统：官方的 `function` 奖励在这里跳过，见文档。
    crate::advancements::announce(region, player, index);
}
