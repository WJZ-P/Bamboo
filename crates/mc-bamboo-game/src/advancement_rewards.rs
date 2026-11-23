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
    // 官方 `AdvancementRewards.grant` 的战利品表奖励：用现有的
    // `LootTable::from_name` + `get_random_items_with_seed` 求值，产物走
    // `inventory::give_or_drop`。内置 26.3 定义里没有 loot 奖励，这条路径
    // 由命令 / 自定义数据包触发。
    for name in &rewards.loot {
        let Ok(table) = mc_bamboo_loot::LootTable::from_name(name) else {
            continue;
        };
        let params = mc_bamboo_loot::LootParams::new(0.0);
        let stacks = table.get_random_items_with_seed(
            &params,
            0,
            Some(&region.loot_sequences),
            region.random.source(),
        );
        if let Ok(stacks) = stacks {
            for stack in &stacks {
                crate::inventory::give_or_drop(
                    region,
                    player,
                    crate::loot_stacks::loot_stack(stack),
                );
            }
        }
    }
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
