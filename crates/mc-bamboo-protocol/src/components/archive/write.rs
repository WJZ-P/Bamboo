//! 把值写成存档编解码器的中间值 `Dyn`，语义照官方的 `DynamicOps` 和 `Codec`。
//!
//! 官方有三种「可选字段」写法，编码行为各不相同，这里各给一个方法：
//!
//! - `optionalFieldOf(name)`：没有值时不写；
//! - `optionalFieldOf(name, default)`：值等于默认值时不写；
//! - `optionalAlwaysPresentFieldOf`：永远写。

use super::{
    dynamic::Dyn,
    read::{Error, Result},
};
use crate::components::cursor::{Holder, HolderSet, Registry};

/// 一个复合标签的构造器。字段按调用顺序写出，和官方 `RecordCodecBuilder` 的顺序一致。
pub struct Fields(Vec<(String, Dyn)>);

impl Fields {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// 官方 `fieldOf`。
    pub fn put(mut self, name: &str, value: Dyn) -> Self {
        self.0.push((name.to_owned(), value));
        self
    }

    /// 官方 `optionalFieldOf(name)`：没有值时不写这个键。
    pub fn optional(self, name: &str, value: Option<Dyn>) -> Self {
        match value {
            Some(value) => self.put(name, value),
            None => self,
        }
    }

    /// 官方 `optionalFieldOf(name, default)`：等于默认值时不写这个键。
    pub fn default(self, name: &str, value: Dyn, default: &Dyn) -> Self {
        if value.same(default) {
            self
        } else {
            self.put(name, value)
        }
    }

    /// 官方 `optionalAlwaysPresentFieldOf`：永远写这个键，哪怕等于默认值。
    pub fn always(self, name: &str, value: Dyn) -> Self {
        self.put(name, value)
    }

    pub fn build(self) -> Dyn {
        Dyn::Map(self.0)
    }
}

/// 官方 `MapCodec.unitCodec`。
pub fn unit() -> Dyn {
    Dyn::empty_map()
}

/// 官方 `Identifier.CODEC` 编码出来的标签。
pub fn identifier(name: &str) -> Dyn {
    Dyn::String(name.to_owned())
}

/// 一个 `Dyn` 列表。
pub fn list(values: impl IntoIterator<Item = Dyn>) -> Dyn {
    Dyn::List(values.into_iter().collect())
}

/// 官方 `ExtraCodecs.compactListCodec`：只有一个元素时写裸元素，其余写列表。
pub fn compact(values: Vec<Dyn>) -> Dyn {
    if values.len() == 1 {
        values
            .into_iter()
            .next()
            .unwrap_or_else(|| Dyn::List(Vec::new()))
    } else {
        Dyn::List(values)
    }
}

/// 官方 `StringRepresentable.fromEnum` 的编码：写枚举常量名。
pub fn enum_name(names: &[&str], id: i32) -> Result<Dyn> {
    usize::try_from(id)
        .ok()
        .and_then(|index| names.get(index))
        .map(|name| Dyn::String((*name).to_owned()))
        .ok_or_else(|| Error::new(format!("{id} is not a known enum id")))
}

/// 官方 `StringRepresentable.fromEnum` 的解码表，按 ID 取名字。
pub fn enum_id(names: &[&str], tag: &mc_bamboo_nbt::Tag) -> Result<i32> {
    super::read::enum_id(names, tag)
}

/// 官方 `RegistryFixedCodec.encode`：只接受注册表引用。
pub fn holder_fixed<T>(registry: Registry, holder: &Holder<T>) -> Result<Dyn> {
    match holder {
        Holder::Id(id) => holder_name(registry, *id),
        Holder::Inline(_) => Err(Error::new(format!(
            "a direct value cannot be serialized in {}",
            registry.name
        ))),
    }
}

/// 官方 `RegistryFileCodec.encode`：引用写名字，内联写元素本身。
pub fn holder<T>(
    registry: Registry,
    holder: &Holder<T>,
    element: impl FnOnce(&T) -> Result<Dyn>,
) -> Result<Dyn> {
    match holder {
        Holder::Id(id) => holder_name(registry, *id),
        Holder::Inline(value) => element(value),
    }
}

/// 一个注册表 ID 对应的完整名字。
pub fn holder_name(registry: Registry, id: i32) -> Result<Dyn> {
    Ok(Dyn::String(super::read::id_name(registry, id)?.to_owned()))
}

/// 官方 `HolderSetCodec.encode`：标签写成 `#名字`，直接列出的条目写成紧凑列表。
pub fn holder_set(registry: Registry, set: &HolderSet) -> Result<Dyn> {
    match set {
        HolderSet::Tag(name) => Ok(Dyn::String(format!("#{name}"))),
        HolderSet::Ids(ids) => {
            let values = ids
                .iter()
                .map(|&id| holder_name(registry, id))
                .collect::<Result<Vec<_>>>()?;
            Ok(compact(values))
        }
    }
}

/// 官方 `ExtraCodecs.intRange` 的编码校验。
pub fn int_range(value: i32, min: i32, max: i32) -> Result<Dyn> {
    if (min..=max).contains(&value) {
        Ok(Dyn::Int(value))
    } else {
        Err(Error::new(format!("{value} is not in [{min};{max}]")))
    }
}

/// 官方 `ExtraCodecs.intRange` 的解码校验。
pub fn range_i32(value: i32, min: i32, max: i32) -> Result<i32> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(Error::new(format!("{value} is not in [{min};{max}]")))
    }
}

/// 官方 `ExtraCodecs.floatRange` 的编码校验。
pub fn float_range(value: f32, min: f32, max: f32) -> Result<Dyn> {
    if (min..=max).contains(&value) {
        Ok(Dyn::Float(value))
    } else {
        Err(Error::new(format!("{value} is not in [{min};{max}]")))
    }
}

/// 官方 `ExtraCodecs.floatRange` 的解码校验。
pub fn range_f32(value: f32, min: f32, max: f32) -> Result<f32> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(Error::new(format!("{value} is not in [{min};{max}]")))
    }
}
