//! 并行算阶段里的骑乘树：一棵树的根和它的乘客排成同一份活，交给同一个线程。
//!
//! `ServerLevel.tickNonPassenger` 先 tick 根实体，再按乘客表递归 `rideTick`。这里把
//! 同一棵树的实体在 uid 升序名单里的下标排到同一个组里，组内按「根 → 乘客（表顺序）
//! → 乘客的乘客」重排；落地仍按 uid 升序发布，所以两种驱动对同一场景逐位一致。

use std::collections::{BTreeMap, BTreeSet};

use crate::entity::{EntityPublic, EntityUid, FIRST_ENTITY_ID};

/// 把 uid 升序的名单切成若干棵树，返回每棵树的下标序列（树之间按根的下标升序）。
/// 组内的第一项一定是根。没有骑乘关系的实体各自成一棵单节点树。
///
/// Splits an uid-ascending list into riding trees, returning each tree's row
/// indices (trees ordered by the root's index). The first item of each group is
/// the root. Entities without a riding relation form single-node trees.
pub fn riding_groups(uids: &[EntityUid], public: &[EntityPublic]) -> Vec<Vec<usize>> {
    let by_uid: BTreeMap<EntityUid, usize> = uids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, uid)| (uid, index))
        .collect();
    // 从每个载具到它的实体乘客：名单里玩家 ID 不在实体表里，跳过。
    let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut has_parent = BTreeSet::new();
    for (index, entry) in public.iter().enumerate() {
        let Some(list) = passengers_of(entry) else {
            continue;
        };
        for passenger in list {
            if passenger < FIRST_ENTITY_ID {
                continue;
            }
            let Some(&child) = by_uid.get(&passenger) else {
                continue;
            };
            // 一个乘客最多挂在一辆载具上；坏数据里出现第二个载具就忽略后者。
            if child == index || has_parent.contains(&child) {
                continue;
            }
            children.entry(index).or_default().push(child);
            has_parent.insert(child);
        }
    }
    let mut groups = Vec::new();
    let mut visited = BTreeSet::new();
    for root in 0..uids.len() {
        if has_parent.contains(&root) || visited.contains(&root) {
            continue;
        }
        let mut group = Vec::new();
        collect_tree(root, &children, &mut visited, &mut group, 0);
        groups.push(group);
    }
    // 成环的链条没有根：剩下的节点各自成组，坏数据不 panic。
    for index in 0..uids.len() {
        if !visited.insert(index) {
            continue;
        }
        groups.push(vec![index]);
    }
    groups.sort_by_key(|group| group[0]);
    groups
}

fn passengers_of(entry: &EntityPublic) -> Option<Vec<EntityUid>> {
    match &entry.kind {
        crate::entity::KindPublic::Mob(mob) => Some(mob.passengers.clone()),
        _ => None,
    }
}

/// 递归深度上限：存盘里的 `Passengers` 坏数据可以造出很深的链，超过就剩下的
/// 节点各自留在本刻开头的快照里，不跑 tick。
const MAX_DEPTH: usize = 64;

fn collect_tree(
    index: usize,
    children: &BTreeMap<usize, Vec<usize>>,
    visited: &mut BTreeSet<usize>,
    out: &mut Vec<usize>,
    depth: usize,
) {
    if !visited.insert(index) || depth > MAX_DEPTH {
        return;
    }
    out.push(index);
    if let Some(list) = children.get(&index) {
        for &child in list {
            collect_tree(child, children, visited, out, depth + 1);
        }
    }
}
