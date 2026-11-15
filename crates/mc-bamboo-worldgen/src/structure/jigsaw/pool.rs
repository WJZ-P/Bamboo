//! `StructureTemplatePool` 的解析与随机选取。
//!
//! 官方构造时把每个 element 按 weight 重复进 `templates` 列表，`getRandomTemplate`
//! 用 `nextInt(size)`，`getShuffledTemplates` 用 `Util.shuffledCopy(templates, random)`。
//!
//! Parse `StructureTemplatePool` and its random selection. Vanilla expands each
//! element into `templates` by weight; `getRandomTemplate` uses `nextInt(size)`,
//! `getShuffledTemplates` uses `Util.shuffledCopy(templates, random)`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    element::PoolElement,
    rotation::{Rotation, shuffled_copy},
};

#[derive(Debug, Clone)]
pub struct TemplatePool {
    /// `rawTemplates`：展开前的 element 序列。
    elements: Vec<PoolElement>,
    /// `templates`：按 weight 重复的 `elements` 下标。
    expanded: Vec<usize>,
    fallback: String,
    /// `maxSize`：官方是懒算的字段（`Integer.MIN_VALUE` 表示未算），这里在解析时就一次
    /// 算好。理由：官方懒算只是为了省一次遍历，输入全是只读数据，提前算结果相同；而用
    /// `Cell` 会让本类型（以及 `TemplatePools`）变成 `!Sync`，没法放进 `Arc` 供多线程
    /// 区块生成共享。提前算换来 `Sync` 是值得的。
    ///
    /// `maxSize`: vanilla computes it lazily (`Integer.MIN_VALUE` means unset). This port
    /// computes it once during parsing: the lazy form only saves one pass over read-only
    /// data, so the value is identical, but a `Cell` would make this type (and
    /// `TemplatePools`) `!Sync` and thus unusable from an `Arc` shared across generation
    /// threads.
    max_size: i32,
}

impl TemplatePool {
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let entries = raw
            .get("elements")
            .and_then(Value::as_array)
            .ok_or("template_pool has no elements")?;
        let mut elements = Vec::with_capacity(entries.len());
        let mut expanded = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let element = PoolElement::parse(
                entry
                    .get("element")
                    .ok_or("template_pool entry has no element")?,
            )?;
            let weight = entry
                .get("weight")
                .and_then(Value::as_u64)
                .ok_or("template_pool entry has no weight")?;
            for _ in 0..weight {
                expanded.push(index);
            }
            elements.push(element);
        }
        let fallback = raw
            .get("fallback")
            .and_then(Value::as_str)
            .ok_or("template_pool has no fallback")?
            .to_owned();
        let mut pool = Self {
            elements,
            expanded,
            fallback,
            max_size: 0,
        };
        // 解析时就一次性算好（官方懒算，结果相同，见字段注释）。
        // Computed once during parsing (vanilla is lazy but the value is identical; see the
        // field comment).
        pool.max_size = pool.compute_max_size()?;
        Ok(pool)
    }

    /// `StructureTemplatePool.getRandomTemplate(random)`。
    pub fn get_random_template(&self, random: &WorldgenRandom) -> Result<PoolElement, String> {
        if self.expanded.is_empty() {
            return Ok(PoolElement::Empty);
        }
        let slot = random.next_int(self.expanded.len() as i32) as usize;
        let index = self.expanded[slot];
        Ok(self.elements[index].clone())
    }

    /// `StructureTemplatePool.getShuffledTemplates(random)`：展开列表洗牌后的 element。
    pub fn shuffled_elements(&self, random: &WorldgenRandom) -> Vec<PoolElement> {
        shuffled_copy(&self.expanded, random)
            .into_iter()
            .map(|index| self.elements[index].clone())
            .collect()
    }

    pub fn element(&self, index: usize) -> Option<&PoolElement> {
        self.elements.get(index)
    }

    pub const fn size(&self) -> usize {
        self.expanded.len()
    }

    pub fn fallback(&self) -> &str {
        &self.fallback
    }

    /// `StructureTemplatePool.getMaxSize(manager)`：展开列表里（除 empty）各 element
    /// 在 `Rotation.NONE` / `BlockPos.ZERO` 下包围盒的最大 `getYSpan()`。
    /// 解析时已算好，这里只返回。
    ///
    /// `getMaxSize(manager)`: the max `getYSpan()` of every (non-empty) expanded
    /// element's bounding box at `Rotation.NONE` / `BlockPos.ZERO`. Precomputed during
    /// parsing.
    pub const fn max_size(&self) -> i32 {
        self.max_size
    }

    /// `getMaxSize` 的实际计算，只在 `parse` 里调一次。
    ///
    /// The actual `getMaxSize` computation, called once from `parse`.
    fn compute_max_size(&self) -> Result<i32, String> {
        let mut max = 0;
        for &index in &self.expanded {
            let element = &self.elements[index];
            if matches!(element, PoolElement::Empty) {
                continue;
            }
            let box_ = element.bounding_box((0, 0, 0), Rotation::None)?;
            max = max.max(box_.max_y - box_.min_y + 1);
        }
        Ok(max)
    }
}

/// `worldgen.json` 的 `template_pool` 对象。
#[derive(Debug, Clone)]
pub struct TemplatePools {
    pools: BTreeMap<String, TemplatePool>,
}

impl TemplatePools {
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("template_pool is not an object")?;
        let mut pools = BTreeMap::new();
        for (name, value) in object {
            pools.insert(name.clone(), TemplatePool::parse(value)?);
        }
        Ok(Self { pools })
    }

    pub fn get(&self, name: &str) -> Option<&TemplatePool> {
        self.pools.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.pools.keys()
    }

    pub fn len(&self) -> usize {
        self.pools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TemplatePools` 必须能跨线程共享：流水线可能把它放进 `Arc` 分给多个生成线程。
    /// 这条断言会把「有人又给 `TemplatePool` 加回 `Cell`」变成编译错误。
    ///
    /// `TemplatePools` must be shareable across threads (the pipeline may put it in an
    /// `Arc`); this assertion turns a reintroduced `Cell` into a compile error.
    #[test]
    fn template_pools_is_sync_and_send() {
        fn assert_sync_send<T: Sync + Send>() {}
        assert_sync_send::<TemplatePools>();
        assert_sync_send::<TemplatePool>();
    }

    /// `max_size` 提前算的结果要和「按定义现算」一致，覆盖全部 245 个 pool。
    ///
    /// The precomputed `max_size` must equal a fresh computation for all 245 pools.
    #[test]
    fn precomputed_max_size_matches_a_fresh_computation() {
        let raw: serde_json::Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let pools = TemplatePools::parse(raw.get("template_pool").unwrap()).unwrap();
        assert_eq!(pools.len(), 245);
        for name in pools.names().cloned().collect::<Vec<_>>() {
            let pool = pools.get(&name).unwrap();
            assert_eq!(
                pool.max_size(),
                pool.compute_max_size().unwrap(),
                "{name}: precomputed max_size differs"
            );
        }
    }
}
