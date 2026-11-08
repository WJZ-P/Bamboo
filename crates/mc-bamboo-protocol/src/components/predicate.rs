//! `can_break` 和 `can_place_on` 里的方块谓词，官方 `advancements.predicates.BlockPredicate`。

use mc_bamboo_nbt::Tag;

use crate::{
    codec::{DecodeError, write_bool, write_string, write_varint},
    components::{
        ComponentValue,
        cursor::{
            Cursor, HolderSet, MAX_STRING, Registry, write_holder_set, write_list, write_optional,
        },
    },
};

/// 官方 `DataComponentPredicate.STREAM_CODEC` 的列表上限。
const MAX_PARTIAL_PREDICATES: usize = 64;

/// 官方 `BlockPredicate`。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockPredicate {
    /// 这一格必须是这些方块之一。
    pub blocks: Option<HolderSet>,
    /// 方块状态的属性要求。
    pub properties: Option<StateProperties>,
    /// 方块实体的 NBT 要求，官方 `NbtPredicate`。
    pub nbt: Option<Vec<(String, Tag)>>,
    /// 方块实体的组件要求。
    pub components: ComponentMatchers,
}

impl BlockPredicate {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            blocks: cursor.optional(|cursor| cursor.holder_set(Registry::BLOCK))?,
            properties: cursor.optional(StateProperties::read)?,
            nbt: cursor.optional(Cursor::nbt_compound)?,
            components: ComponentMatchers::read(cursor)?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_optional(out, value.blocks.as_ref(), write_holder_set);
        write_optional(out, value.properties.as_ref(), |out, properties| {
            properties.write(out);
        });
        write_optional(out, value.nbt.as_ref(), |out, entries| {
            Tag::Compound(entries.clone()).write_network(out);
        });
        value.components.write(out);
    }
}

/// 官方 `StatePropertiesPredicate`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateProperties(pub Vec<PropertyMatcher>);

impl StateProperties {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self(cursor.list(usize::MAX, PropertyMatcher::read)?))
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_list(out, &self.0, PropertyMatcher::write);
    }
}

/// 官方 `StatePropertiesPredicate.PropertyMatcher`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyMatcher {
    pub name: String,
    pub matcher: ValueMatcher,
}

impl PropertyMatcher {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            name: cursor.string(MAX_STRING)?,
            matcher: ValueMatcher::read(cursor)?,
        })
    }

    fn write(out: &mut Vec<u8>, value: &Self) {
        write_string(out, &value.name);
        value.matcher.write(out);
    }
}

/// 官方 `StatePropertiesPredicate.ValueMatcher`：一个确切的值，或者一个区间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueMatcher {
    Exact(String),
    Ranged {
        min: Option<String>,
        max: Option<String>,
    },
}

impl ValueMatcher {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let matcher = cursor.either(
            |cursor| cursor.string(MAX_STRING),
            |cursor| {
                Ok((
                    cursor.optional(|cursor| cursor.string(MAX_STRING))?,
                    cursor.optional(|cursor| cursor.string(MAX_STRING))?,
                ))
            },
        )?;
        Ok(match matcher {
            Ok(value) => Self::Exact(value),
            Err((min, max)) => Self::Ranged { min, max },
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Exact(value) => {
                write_bool(out, true);
                write_string(out, value);
            }
            Self::Ranged { min, max } => {
                write_bool(out, false);
                for bound in [min, max] {
                    write_optional(out, bound.as_ref(), |out, value| write_string(out, value));
                }
            }
        }
    }
}

/// 官方 `DataComponentMatchers`：一组要完全相等的组件，加上一组部分匹配的谓词。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentMatchers {
    /// 官方 `DataComponentExactPredicate`：这些组件的值必须一模一样。
    pub exact: Vec<TypedComponent>,
    /// 官方 `DataComponentPredicate`：按谓词类型分派，值是 NBT。
    pub partial: Vec<PartialPredicate>,
}

impl ComponentMatchers {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            exact: cursor.list(usize::MAX, TypedComponent::read)?,
            partial: cursor.list(MAX_PARTIAL_PREDICATES, PartialPredicate::read)?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_list(out, &self.exact, TypedComponent::write);
        write_list(out, &self.partial, PartialPredicate::write);
    }
}

/// 官方 `TypedDataComponent`：组件类型加上它的值。
#[derive(Debug, Clone, PartialEq)]
pub struct TypedComponent {
    /// `minecraft:data_component_type` 的注册表 ID。
    pub kind: i32,
    pub value: ComponentValue,
}

impl TypedComponent {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let kind = cursor.registry_id(Registry::DATA_COMPONENT_TYPE)?;
        Ok(Self {
            kind,
            value: ComponentValue::read(cursor, kind)?,
        })
    }

    fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.kind);
        value.value.write(out);
    }
}

/// 官方 `DataComponentPredicate.Single`。值走的是 NBT 编解码，内容不往下看。
#[derive(Debug, Clone, PartialEq)]
pub struct PartialPredicate {
    pub kind: PredicateType,
    pub value: Tag,
}

impl PartialPredicate {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: PredicateType::read(cursor)?,
            value: cursor.nbt_tag()?,
        })
    }

    fn write(out: &mut Vec<u8>, value: &Self) {
        value.kind.write(out);
        value.value.write_network(out);
    }
}

/// 官方 `DataComponentPredicate.Type`：一个具体的谓词类型，或者「这个组件有值就行」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateType {
    /// `minecraft:data_component_predicate_type` 的注册表 ID。
    Concrete(i32),
    /// `minecraft:data_component_type` 的注册表 ID，官方的 `AnyValueType`。
    AnyValue(i32),
}

impl PredicateType {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let kind = cursor.either(
            |cursor| cursor.registry_id(Registry::DATA_COMPONENT_PREDICATE_TYPE),
            |cursor| cursor.registry_id(Registry::DATA_COMPONENT_TYPE),
        )?;
        Ok(match kind {
            Ok(id) => Self::Concrete(id),
            Err(id) => Self::AnyValue(id),
        })
    }

    fn write(self, out: &mut Vec<u8>) {
        match self {
            Self::Concrete(id) => {
                write_bool(out, true);
                write_varint(out, id);
            }
            Self::AnyValue(id) => {
                write_bool(out, false);
                write_varint(out, id);
            }
        }
    }
}
