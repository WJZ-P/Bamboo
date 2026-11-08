//! 组件里嵌套的物品堆，官方的 `ItemStackTemplate`。
//!
//! 容器、收纳袋、弩的弹药、使用后剩余的物品、装饰罐的四面都用它，它自己又带一份组件补丁，
//! 所以可以无限嵌套。官方靠数据包大小兜着，这里按 [`crate::components::cursor::MAX_STACK_DEPTH`]
//! 限制层数。

use mc_bamboo_data::items::AIR;

use crate::{
    codec::{DecodeError, write_varint},
    components::{
        ComponentPatch,
        cursor::{Cursor, Registry},
    },
};

/// 官方 `ItemStackTemplate`：物品、数量和一份组件补丁，不允许是空的。
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStackTemplate {
    /// `minecraft:item` 的注册表 ID。
    pub item: i32,
    pub count: i32,
    pub components: ComponentPatch,
}

impl ItemStackTemplate {
    /// 官方 `ItemStackTemplate.STREAM_CODEC`。
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        cursor.nested(|cursor| {
            let item = cursor.registry_id(Registry::ITEM)?;
            let count = cursor.varint()?;
            let components = ComponentPatch::read(cursor)?;
            // 官方的构造方法要求 `count != 0 && item != AIR`，不满足时抛异常。
            if count == 0 || item == AIR {
                return Err(DecodeError::InvalidComponentValue(
                    "nested item stack must be non-empty",
                ));
            }
            Ok(Self {
                item,
                count,
                components,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.item);
        write_varint(out, value.count);
        value.components.write(out);
    }

    /// 官方 `ItemStack.validateContainedItemSizes` 看的那个上限。
    pub fn max_stack_size(&self) -> i32 {
        let item = usize::try_from(self.item)
            .ok()
            .and_then(|id| mc_bamboo_data::items::ITEMS.get(id));
        // 组件补丁可以改写 `max_stack_size`，先看补丁再看物品自己的默认值。
        self.components
            .max_stack_size()
            .or_else(|| item.map(|item| item.max_stack_size))
            .unwrap_or(1)
    }
}
