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

use super::{element::PoolElement, rotation::shuffled_copy};

#[derive(Debug, Clone)]
pub struct TemplatePool {
    /// `rawTemplates`：展开前的 element 序列。
    elements: Vec<PoolElement>,
    /// `templates`：按 weight 重复的 `elements` 下标。
    expanded: Vec<usize>,
    fallback: String,
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

    /// `StructureTemplatePool.getShuffledTemplates(random)`：返回展开后的下标。
    pub fn shuffled_templates(&self, random: &WorldgenRandom) -> Vec<usize> {
        shuffled_copy(&self.expanded, random)
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
