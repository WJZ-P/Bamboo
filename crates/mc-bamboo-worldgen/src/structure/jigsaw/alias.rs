//! `PoolAliasLookup`：jigsaw 结构把拼图方块里的 pool 名映射成实际 pool。
//!
//! 官方 `PoolAliasLookup.create(bindings, pos, seed)` 先用
//! `RandomSource.create(seed).forkPositional().at(pos)` 造一个位置随机源，再按
//! binding 顺序逐个解析（`direct` 不取数，`random` 取一次 `nextInt(totalWeight)`，
//! `random_group` 先选组再把组里的 binding 依次解析）。所有 binding 共用这一个随机源，
//! 所以取数顺序就是配置里的顺序。没有 binding 时是恒等映射（`PoolAliasLookup.EMPTY`）。
//!
//! `PoolAliasLookup`: the jigsaw structure's map from a jigsaw block's pool name to the
//! actual pool. Vanilla `PoolAliasLookup.create(bindings, pos, seed)` builds
//! `RandomSource.create(seed).forkPositional().at(pos)`, then resolves each binding in
//! order (`direct` draws nothing, `random` draws one `nextInt(totalWeight)`,
//! `random_group` picks a group and then resolves that group's bindings). All bindings
//! share one random source, so the draw order is the config order. With no bindings it
//! is the identity map (`PoolAliasLookup.EMPTY`).

use std::collections::BTreeMap;

use mc_bamboo_blocks::LegacyRandom;
use serde_json::Value;

use crate::random::Random;

/// `PoolAliasLookup`：别名 → 目标 pool 的映射。
///
/// `PoolAliasLookup`: alias → target pool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolAliasLookup {
    mappings: BTreeMap<String, String>,
}

impl PoolAliasLookup {
    /// `PoolAliasLookup.EMPTY`。
    ///
    /// `PoolAliasLookup.EMPTY`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// `PoolAliasLookup.create(bindings, pos, seed)`。
    ///
    /// `PoolAliasLookup.create(bindings, pos, seed)`.
    pub fn create(bindings: &[Value], pos: (i32, i32, i32), seed: i64) -> Result<Self, String> {
        let mut mappings = BTreeMap::new();
        if bindings.is_empty() {
            return Ok(Self { mappings });
        }
        let random = Random::Legacy(LegacyRandom::new(seed));
        let positional = random.fork_positional();
        let positioned = positional.at(pos.0, pos.1, pos.2);
        resolve_bindings(bindings, &positioned, &mut mappings)?;
        Ok(Self { mappings })
    }

    /// `lookup(alias)`：没有映射时原样返回。
    ///
    /// `lookup(alias)`: identity when no binding names it.
    pub fn lookup<'a>(&'a self, name: &'a str) -> &'a str {
        self.mappings.get(name).map_or(name, String::as_str)
    }

    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

fn resolve_bindings(
    bindings: &[Value],
    random: &Random,
    mappings: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for binding in bindings {
        let kind = binding
            .get("type")
            .and_then(Value::as_str)
            .ok_or("pool alias binding has no type")?;
        match kind {
            "minecraft:direct" => {
                mappings.insert(
                    string_field(binding, "alias")?,
                    string_field(binding, "target")?,
                );
            }
            "minecraft:random" => {
                let alias = string_field(binding, "alias")?;
                let targets = weighted_entries(
                    binding
                        .get("targets")
                        .ok_or("random alias has no targets")?,
                )?;
                let target = weighted_pick(&targets, random)?
                    .as_str()
                    .ok_or("random alias target is not a name")?;
                mappings.insert(alias, target.to_owned());
            }
            "minecraft:random_group" => {
                let groups =
                    weighted_entries(binding.get("groups").ok_or("random group has no groups")?)?;
                let inner = weighted_pick(&groups, random)?
                    .as_array()
                    .ok_or("random group entry is not a list")?;
                resolve_bindings(inner, random, mappings)?;
            }
            other => return Err(format!("unsupported pool alias binding {other}")),
        }
    }
    Ok(())
}

/// `WeightedList.unwrap()` 的展开：每项 `(data, weight)`，按配置顺序。
///
/// The `WeightedList.unwrap()` view: `(data, weight)` in config order.
fn weighted_entries(raw: &Value) -> Result<Vec<(&Value, i32)>, String> {
    let entries = raw.as_array().ok_or("weighted list is not a list")?;
    entries
        .iter()
        .map(|entry| {
            let data = entry.get("data").ok_or("weighted entry has no data")?;
            let weight = entry
                .get("weight")
                .and_then(Value::as_i64)
                .ok_or("weighted entry has no weight")? as i32;
            Ok((data, weight))
        })
        .collect()
}

/// `WeightedList.getRandomOrThrow(random)`：`nextInt(totalWeight)` 后按权重累减。
/// `Flat` 与 `Compact` 两种选择器在取值上等价，这里只实现累减这一种。
///
/// `WeightedList.getRandomOrThrow(random)`: one `nextInt(totalWeight)` then cumulative
/// subtraction. `Flat` and `Compact` selectors agree on the value, so only one is needed.
fn weighted_pick<'a>(entries: &[(&'a Value, i32)], random: &Random) -> Result<&'a Value, String> {
    let total: i32 = entries.iter().map(|(_, weight)| weight).sum();
    if total <= 0 {
        return Err("weighted list has no elements".to_owned());
    }
    let mut selection = random.next_int(total);
    for (data, weight) in entries {
        selection -= weight;
        if selection < 0 {
            return Ok(data);
        }
    }
    Err(format!("{selection} exceeded total weight"))
}

fn string_field(raw: &Value, key: &str) -> Result<String, String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("pool alias has no {key}"))
}
