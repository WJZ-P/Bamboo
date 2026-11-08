//! 战利品表本体，对应官方的 `LootTable`。
//!
//! 表的求值就是按顺序跑每个池；最后按官方 `createStackSplitter` 把超过最大
//! 堆叠数的栈拆开。箱子填充还要再按 `fill` 的规则洗牌和拆分，随机数的取数
//! 次数和官方一致。
//!
//! 数据包里的表以 JSON 原样放在 `mc-bamboo-data`，第一次用到时解析并缓存。
//! 缓存是进程级的，表只依赖静态数据，和世界种子无关。

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use mc_bamboo_blocks::random::{shuffle, LegacyRandom, RandomSource, XoroshiroRandom};

use crate::context::{LootContext, LootParams};
use crate::function::{parse_modifier, LootFunction};
use crate::json::{opt_array, opt_string, object};
use crate::pool::LootPool;
use crate::random_sequence::RandomSequences;
use crate::stack::LootStack;
use crate::{LootError, Result};

/// 一张战利品表。
#[derive(Debug)]
pub struct LootTable {
    /// 参数集名字，比如 `minecraft:block`。求值本身不检查它，留作诊断。
    param_set: Option<String>,
    /// 随机序列名。有这个字段时，没有显式种子就用按名字缓存的随机流。
    random_sequence: Option<String>,
    pools: Vec<LootPool>,
    modifier: Option<LootFunction>,
}

/// 能装战利品的容器，对应官方的 `Container`。
///
/// `fill` 只碰空槽位，槽位顺序和官方一样由容器提供。
pub trait LootContainer {
    fn slot_count(&self) -> usize;
    fn is_slot_empty(&self, slot: usize) -> bool;
    fn set_slot(&mut self, slot: usize, stack: LootStack);
}

impl LootTable {
    /// 解析一张表。`pools` 缺省时是空表。
    pub fn parse(value: &Value) -> Result<Self> {
        let map = object(value, "loot table")?;
        let param_set = opt_string(map, "type", "loot table type")?.map(str::to_owned);
        let random_sequence = opt_string(map, "random_sequence", "random_sequence")?.map(str::to_owned);
        let pools = match opt_array(map, "pools", "pools")? {
            Some(pools) => pools.iter().map(LootPool::parse).collect::<Result<Vec<_>>>()?,
            None => Vec::new(),
        };
        let modifier = parse_modifier(crate::json::field(map, "modifier"))?;
        Ok(Self {
            param_set,
            random_sequence,
            pools,
            modifier,
        })
    }

    /// 按注册表名字取表，解析结果按名字缓存。
    pub fn from_name(name: &str) -> Result<Arc<Self>> {
        if let Some(table) = table_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
        {
            return Ok(Arc::clone(table));
        }
        let raw = mc_bamboo_data::loot::loot_table(name)
            .ok_or_else(|| LootError::UnknownTable(name.to_owned()))?;
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| LootError::Invalid(format!("{name}: {error}")))?;
        let table = Arc::new(Self::parse(&value).map_err(|error| match error {
            LootError::Invalid(message) => LootError::Invalid(format!("{name}: {message}")),
            error => error,
        })?);
        table_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), Arc::clone(&table));
        Ok(table)
    }

    pub fn param_set(&self) -> Option<&str> {
        self.param_set.as_deref()
    }

    pub fn random_sequence(&self) -> Option<&str> {
        self.random_sequence.as_deref()
    }

    /// `LootTable.getRandomItems(params, randomSource)`：调用方给随机源。
    pub fn get_random_items(
        &self,
        params: &LootParams<'_>,
        random: &dyn RandomSource,
    ) -> Result<Vec<LootStack>> {
        let context = LootContext::new(params, random);
        let mut stacks = Vec::new();
        self.get_random_items_raw(&context, 0, &mut stacks)?;
        Ok(create_stack_splitter(stacks))
    }

    /// `LootTable.getRandomItems(params, seed)`：非零种子用旧版随机源，
    /// 否则按 `random_sequence` 取缓存序列，再不行用调用方给的默认源。
    pub fn get_random_items_with_seed(
        &self,
        params: &LootParams<'_>,
        seed: i64,
        sequences: Option<&RandomSequences>,
        fallback: &dyn RandomSource,
    ) -> Result<Vec<LootStack>> {
        let mut legacy = None;
        let mut cached = None;
        let source = self.resolve_source(seed, sequences, fallback, &mut legacy, &mut cached)?;
        self.get_random_items(params, source)
    }

    /// `LootTable.getRandomItemsRaw`：跑所有池，嵌套表也会走到这里。
    ///
    /// 官方用访问过的表集合防无限循环；这里没有可比较的表身份，改用深度上限。
    pub(crate) fn get_random_items_raw(
        &self,
        context: &LootContext<'_, '_, '_>,
        depth: u32,
        output: &mut Vec<LootStack>,
    ) -> Result<()> {
        if depth > crate::entry::MAX_DEPTH {
            return Err(LootError::Invalid(
                "loot table nests deeper than the depth limit".to_owned(),
            ));
        }
        for pool in &self.pools {
            pool.add_random_items(context, self.modifier.as_ref(), output)?;
        }
        Ok(())
    }

    /// `LootTable.fill`：把战利品装进容器。
    pub fn fill(
        &self,
        container: &mut dyn LootContainer,
        params: &LootParams<'_>,
        seed: i64,
        sequences: Option<&RandomSequences>,
        fallback: &dyn RandomSource,
    ) -> Result<()> {
        let mut legacy = None;
        let mut cached = None;
        let source = self.resolve_source(seed, sequences, fallback, &mut legacy, &mut cached)?;
        let context = LootContext::new(params, source);
        let mut stacks = Vec::new();
        self.get_random_items_raw(&context, 0, &mut stacks)?;
        let mut stacks = create_stack_splitter(stacks);

        let random = context.random();
        let mut available_slots = get_available_slots(container, random);
        shuffle_and_split_items(&mut stacks, available_slots.len(), random);
        for stack in stacks {
            let Some(slot) = available_slots.pop() else {
                // 官方这里打一条警告就把剩下的物品丢掉。
                return Ok(());
            };
            let stack = if stack.is_empty() {
                LootStack::empty()
            } else {
                stack
            };
            container.set_slot(slot, stack);
        }
        Ok(())
    }

    /// 按官方 `new LootContext.Builder(params).withOptionalRandomSeed(seed)`
    /// 的优先级选随机源：非零种子最优先，其次是 `random_sequence`，最后是默认源。
    fn resolve_source<'a>(
        &self,
        seed: i64,
        sequences: Option<&RandomSequences>,
        fallback: &'a dyn RandomSource,
        legacy: &'a mut Option<LegacyRandom>,
        cached: &'a mut Option<Rc<XoroshiroRandom>>,
    ) -> Result<&'a dyn RandomSource> {
        if seed != 0 {
            *legacy = Some(LegacyRandom::new(seed));
            return Ok(legacy.as_ref().unwrap());
        }
        if let Some(key) = &self.random_sequence {
            let sequences = sequences.ok_or_else(|| {
                LootError::Unsupported(format!(
                    "loot table wants random sequence `{key}` but no sequence cache was given"
                ))
            })?;
            *cached = Some(sequences.get(key));
            return Ok(cached.as_ref().unwrap().as_ref());
        }
        Ok(fallback)
    }
}

fn table_cache() -> &'static Mutex<HashMap<String, Arc<LootTable>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<LootTable>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `LootTable.createStackSplitter`：数量小于最大堆叠数就原样输出，
/// 否则按最大堆叠数切开，直到数量为 0。
///
/// 注意空栈（数量 0）也小于上限，所以会原样留在结果里，官方一样。
pub fn create_stack_splitter(stacks: Vec<LootStack>) -> Vec<LootStack> {
    let mut output = Vec::new();
    for stack in stacks {
        let max = stack.max_stack_size();
        if stack.count < max || max <= 0 {
            output.push(stack);
            continue;
        }
        let mut left = stack.count;
        while left > 0 {
            let part = stack.copy_with_count(max.min(left));
            left -= part.count;
            output.push(part);
        }
    }
    output
}

/// `LootTable.getAvailableSlots`：空槽位列表洗一遍。
fn get_available_slots(
    container: &dyn LootContainer,
    random: &dyn RandomSource,
) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..container.slot_count())
        .filter(|slot| container.is_slot_empty(*slot))
        .collect();
    shuffle(random, &mut slots);
    slots
}

/// `LootTable.shuffleAndSplitItems`：把数量大于 1 的栈拆开，直到每个物品
/// 都占一个槽位，最后整体洗牌。
pub fn shuffle_and_split_items(
    stacks: &mut Vec<LootStack>,
    available_slots: usize,
    random: &dyn RandomSource,
) {
    let mut splittable = Vec::new();
    let mut result = Vec::new();
    for stack in stacks.drain(..) {
        if stack.is_empty() {
            continue;
        }
        if stack.count > 1 {
            splittable.push(stack);
        } else {
            result.push(stack);
        }
    }
    while available_slots as i64 - result.len() as i64 - splittable.len() as i64 > 0
        && !splittable.is_empty()
    {
        let index = random.mth_next_int(0, splittable.len() as i32 - 1) as usize;
        let mut stack = splittable.remove(index);
        let remove = random.mth_next_int(1, stack.count / 2);
        let copy = stack.split(remove);
        if stack.count > 1 && random.next_boolean() {
            splittable.push(stack);
        } else {
            result.push(stack);
        }
        if copy.count > 1 && random.next_boolean() {
            splittable.push(copy);
        } else {
            result.push(copy);
        }
    }
    result.append(&mut splittable);
    shuffle(random, &mut result);
    *stacks = result;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use mc_bamboo_data::{items, loot};

    #[test]
    fn all_packaged_tables_parse() {
        for &(name, _) in loot::LOOT_TABLES {
            LootTable::from_name(name)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn unknown_tables_are_reported() {
        assert!(matches!(
            LootTable::from_name("minecraft:blocks/not_a_table"),
            Err(LootError::UnknownTable(_))
        ));
    }

    #[test]
    fn a_missing_pool_list_is_an_empty_table() {
        let table = LootTable::parse(&json!({})).unwrap();
        let params = LootParams::default();
        let random = LegacyRandom::new(0);
        assert!(table.get_random_items(&params, &random).unwrap().is_empty());
    }

    #[test]
    fn stacks_are_split_at_the_max_stack_size() {
        let stacks = vec![
            LootStack::new(items::STONE, 130),
            LootStack::new(items::DIAMOND, 1),
        ];
        let split = create_stack_splitter(stacks);
        assert_eq!(split.len(), 4);
        assert_eq!(split[0].count, 64);
        assert_eq!(split[1].count, 64);
        assert_eq!(split[2].count, 2);
        assert_eq!(split[3].item, items::DIAMOND);
    }

    #[test]
    fn empty_stacks_survive_the_splitter() {
        let stacks = vec![LootStack::empty()];
        let split = create_stack_splitter(stacks);
        assert_eq!(split.len(), 1);
        assert!(split[0].is_empty());
    }

    #[test]
    fn shuffling_preserves_the_total_count() {
        let mut stacks = vec![LootStack::new(items::STONE, 5)];
        let random = LegacyRandom::new(3);
        shuffle_and_split_items(&mut stacks, 5, &random);
        // 官方在「结果栈数 + 可拆栈数」够填满槽位时就停，不保证一格一个。
        assert!(stacks.len() >= 2 && stacks.len() <= 5);
        assert_eq!(stacks.iter().map(|stack| stack.count).sum::<i32>(), 5);
        assert!(stacks.iter().all(|stack| stack.item == items::STONE));
    }

    #[test]
    fn shuffling_stops_when_slots_run_out() {
        let mut stacks = vec![LootStack::new(items::STONE, 10)];
        let random = LegacyRandom::new(3);
        shuffle_and_split_items(&mut stacks, 2, &random);
        assert_eq!(stacks.len(), 2);
        assert_eq!(stacks.iter().map(|stack| stack.count).sum::<i32>(), 10);
    }

    struct FixedContainer {
        slots: Vec<Option<LootStack>>,
        taken: Vec<bool>,
    }

    impl FixedContainer {
        fn new(size: usize) -> Self {
            Self {
                slots: vec![None; size],
                taken: vec![false; size],
            }
        }
    }

    impl LootContainer for FixedContainer {
        fn slot_count(&self) -> usize {
            self.slots.len()
        }

        fn is_slot_empty(&self, slot: usize) -> bool {
            self.slots[slot].is_none()
        }

        fn set_slot(&mut self, slot: usize, stack: LootStack) {
            assert!(!self.taken[slot], "槽位只该被填一次");
            self.taken[slot] = true;
            self.slots[slot] = Some(stack);
        }
    }

    #[test]
    fn filling_uses_every_empty_slot_once() {
        let table = LootTable::from_name("minecraft:chests/simple_dungeon").unwrap();
        let params = LootParams::default();
        let mut container = FixedContainer::new(27);
        let fallback = LegacyRandom::new(9);
        table
            .fill(
                &mut container,
                &params,
                424_242,
                None,
                &fallback,
            )
            .unwrap();
        let filled = container.slots.iter().filter(|slot| slot.is_some()).count();
        assert!(filled > 0);
        assert!(filled <= 27);
    }
}
