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
    /// `maxSize`：官方缓存在字段里（`Integer.MIN_VALUE` 表示未算），首次调用才算。
    ///
    /// `maxSize`: vanilla caches it in a field (`Integer.MIN_VALUE` means unset) and
    /// computes it on first use.
    max_size: std::cell::Cell<i32>,
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
        Ok(Self {
            elements,
            expanded,
            fallback,
            max_size: std::cell::Cell::new(i32::MIN),
        })
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
    ///
    /// `getMaxSize(manager)`: the max `getYSpan()` of every (non-empty) expanded
    /// element's bounding box at `Rotation.NONE` / `BlockPos.ZERO`.
    pub fn max_size(&self) -> Result<i32, String> {
        if self.max_size.get() != i32::MIN {
            return Ok(self.max_size.get());
        }
        let mut max = 0;
        for &index in &self.expanded {
            let element = &self.elements[index];
            if matches!(element, PoolElement::Empty) {
                continue;
            }
            let box_ = element.bounding_box((0, 0, 0), Rotation::None)?;
            max = max.max(box_.max_y - box_.min_y + 1);
        }
        self.max_size.set(max);
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

    pub fn len(&self) -> usize {
        self.pools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }
}
