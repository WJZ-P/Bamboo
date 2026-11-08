//! 把实体介绍给客户端，以及移动之外的实体状态。

use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::packets::play::clientbound;
use uuid::Uuid;

use super::{HumanoidArm, write_lp_vec3};
use crate::{
    codec::{write_f32, write_f64, write_string, write_uuid, write_varint, write_vec3},
    item::ItemStack,
    packet::ClientboundPacket,
};

/// 实体元数据序列化器的 ID。
const SERIALIZER_BYTE: i32 = 0;
const SERIALIZER_FLOAT: i32 = 3;
const SERIALIZER_POSE: i32 = 20;
const SERIALIZER_HUMANOID_ARM: i32 = 42;

/// `set_entity_data` 的结束标记。
const ENTITY_DATA_END: u8 = 0xff;

/// `set_equipment` 里表示后面还有一项的位。
const EQUIPMENT_CONTINUE: u8 = 0x80;

/// 让客户端创建一个实体。
///
/// 玩家实体必须排在这名玩家的 `player_info_update`（带 `ADD_PLAYER`）之后，
/// 也不能发给玩家自己，否则客户端会把本地玩家删掉。
///
/// 位置和角度取上次发出去的状态，这样下一个位移才接得上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddEntity {
    pub entity_id: i32,
    pub uuid: Uuid,
    /// `minecraft:entity_type` 注册表 ID。
    pub entity_type: i32,
    pub position: Vec3,
    pub movement: Vec3,
    /// 这三个角度是 [`pack_degrees`](super::pack_degrees) 的结果。
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    pub data: i32,
}

impl ClientboundPacket for AddEntity {
    const ID: i32 = clientbound::ADD_ENTITY;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_uuid(out, self.uuid);
        write_varint(out, self.entity_type);
        write_vec3(out, self.position);
        write_lp_vec3(out, self.movement);
        out.extend_from_slice(&[self.pitch, self.yaw, self.head_yaw]);
        write_varint(out, self.data);
    }
}

/// 让客户端移除实体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveEntities<'a> {
    pub entity_ids: &'a [i32],
}

impl ClientboundPacket for RemoveEntities<'_> {
    const ID: i32 = clientbound::REMOVE_ENTITIES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_ids.len() as i32);
        for &id in self.entity_ids {
            write_varint(out, id);
        }
    }
}

/// 实体的姿势。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Standing = 0,
    FallFlying = 1,
    Sleeping = 2,
    Swimming = 3,
    SpinAttack = 4,
    Crouching = 5,
    Dying = 7,
}

/// 玩家实体的一项元数据。
///
/// 序列化器和 index 对不上时客户端会断线，所以每个 index 一个变体，不接受任意组合。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityData {
    /// index 0，BYTE。`ENTITY_FLAG_*` 的组合。
    SharedFlags(u8),
    /// index 6，POSE。
    Pose(Pose),
    /// index 9，FLOAT。
    Health(f32),
    /// index 15，HUMANOID_ARM。
    MainHand(HumanoidArm),
    /// index 16，BYTE。皮肤外层的掩码。
    ModelCustomisation(u8),
}

/// [`EntityData::SharedFlags`] 里表示按住潜行键的位。
pub const ENTITY_FLAG_SHIFT_KEY_DOWN: u8 = 0x02;
/// [`EntityData::SharedFlags`] 里表示正在冲刺的位。
pub const ENTITY_FLAG_SPRINTING: u8 = 0x08;
/// [`EntityData::SharedFlags`] 里表示正在滑翔的位。
pub const ENTITY_FLAG_FALL_FLYING: u8 = 0x80;

impl EntityData {
    fn write(self, out: &mut Vec<u8>) {
        match self {
            Self::SharedFlags(flags) => {
                write_data_header(out, 0, SERIALIZER_BYTE);
                out.push(flags);
            }
            Self::Pose(pose) => {
                write_data_header(out, 6, SERIALIZER_POSE);
                write_varint(out, pose as i32);
            }
            Self::Health(health) => {
                write_data_header(out, 9, SERIALIZER_FLOAT);
                write_f32(out, health);
            }
            Self::MainHand(arm) => {
                write_data_header(out, 15, SERIALIZER_HUMANOID_ARM);
                write_varint(out, arm as i32);
            }
            Self::ModelCustomisation(parts) => {
                write_data_header(out, 16, SERIALIZER_BYTE);
                out.push(parts);
            }
        }
    }
}

fn write_data_header(out: &mut Vec<u8>, index: u8, serializer: i32) {
    out.push(index);
    write_varint(out, serializer);
}

/// 更新实体元数据，只写变化的项。配对时 index 要升序，和官方一致。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetEntityData<'a> {
    pub entity_id: i32,
    pub items: &'a [EntityData],
}

impl ClientboundPacket for SetEntityData<'_> {
    const ID: i32 = clientbound::SET_ENTITY_DATA;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        for item in self.items {
            item.write(out);
        }
        out.push(ENTITY_DATA_END);
    }
}

/// 属性修饰符的运算方式。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeOperation {
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

/// 一个属性修饰符。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeModifier<'a> {
    pub id: &'a str,
    pub amount: f64,
    pub operation: AttributeOperation,
}

/// 一个属性的基础值和修饰符。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeSnapshot<'a> {
    /// `minecraft:attribute` 注册表 ID。
    pub attribute: i32,
    pub base: f64,
    pub modifiers: &'a [AttributeModifier<'a>],
}

/// 同步实体的属性。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpdateAttributes<'a> {
    pub entity_id: i32,
    pub attributes: &'a [AttributeSnapshot<'a>],
}

impl ClientboundPacket for UpdateAttributes<'_> {
    const ID: i32 = clientbound::UPDATE_ATTRIBUTES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.attributes.len() as i32);
        for attribute in self.attributes {
            write_varint(out, attribute.attribute);
            write_f64(out, attribute.base);
            write_varint(out, attribute.modifiers.len() as i32);
            for modifier in attribute.modifiers {
                write_string(out, modifier.id);
                write_f64(out, modifier.amount);
                write_varint(out, modifier.operation as i32);
            }
        }
    }
}

/// 实体的装备槽。枚举值是官方的 ordinal，不是 `EquipmentSlot.id`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquipmentSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

/// 更新实体的装备，发给看得到这个实体的其他玩家。
///
/// 物品带着组件：客户端要靠它们画附魔光效和改过的名字。发不出去的那几种组件由
/// [`ItemStack::write`] 滤掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEquipment<'a> {
    entity_id: i32,
    slots: &'a [(EquipmentSlot, &'a ItemStack)],
}

impl<'a> SetEquipment<'a> {
    /// `slots` 为空时返回 `None`：线上没有个数，客户端至少读一项，空包会让它断线。
    pub fn new(entity_id: i32, slots: &'a [(EquipmentSlot, &'a ItemStack)]) -> Option<Self> {
        (!slots.is_empty()).then_some(Self { entity_id, slots })
    }
}

impl ClientboundPacket for SetEquipment<'_> {
    const ID: i32 = clientbound::SET_EQUIPMENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        for (index, (slot, stack)) in self.slots.iter().enumerate() {
            let is_last = index + 1 == self.slots.len();
            out.push(*slot as u8 | if is_last { 0 } else { EQUIPMENT_CONTINUE });
            stack.write(out);
        }
    }
}

/// 挥手动画的类型。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingAnimationType {
    None = 0,
    Whack = 1,
    Stab = 2,
}

/// 让其他玩家看到实体挥手。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    pub entity_id: i32,
    pub hand: Hand,
    pub animation: SwingAnimationType,
    /// 动画的 tick 数。
    pub duration: i32,
}

impl SwingAnimation {
    /// 物品没有自定义动画时的挥手。
    pub const fn default_swing(entity_id: i32, hand: Hand) -> Self {
        Self {
            entity_id,
            hand,
            animation: SwingAnimationType::Whack,
            duration: 6,
        }
    }
}

impl ClientboundPacket for SwingAnimation {
    const ID: i32 = clientbound::SWING_ANIMATION;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.entity_id);
        write_varint(out, self.hand as i32);
        write_varint(out, self.animation as i32);
        write_varint(out, self.duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, frame, hex};

    const ENTITY: i32 = 7;

    fn set_entity_data(entity_id: i32, items: &[EntityData]) -> Vec<u8> {
        body(&SetEntityData { entity_id, items })
    }

    #[test]
    fn add_entity_matches_official_bytes() {
        let packet = AddEntity {
            entity_id: ENTITY,
            uuid: Uuid::from_u128(0x069a79f4_44e9_4726_a5be_fca90e38aaf5),
            entity_type: 159,
            position: Vec3::new(8.5, 65.0, -3.25),
            movement: Vec3::default(),
            pitch: 0x00,
            yaw: 0x40,
            head_yaw: 0x40,
            data: 0,
        };
        assert_eq!(
            body(&packet),
            hex("07
                 06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5
                 9f 01
                 40 21 00 00 00 00 00 00
                 40 50 40 00 00 00 00 00
                 c0 0a 00 00 00 00 00 00
                 00
                 00 40 40
                 00")
        );
    }

    #[test]
    fn remove_entities_lists_var_ints() {
        let packet = RemoveEntities {
            entity_ids: &[7, 300],
        };
        assert_eq!(body(&packet), hex("02 07 ac 02"));
    }

    #[test]
    fn set_entity_data_matches_official_bytes() {
        let items = [
            EntityData::SharedFlags(ENTITY_FLAG_SHIFT_KEY_DOWN | ENTITY_FLAG_SPRINTING),
            EntityData::Pose(Pose::Crouching),
            EntityData::Health(20.0),
            EntityData::MainHand(HumanoidArm::Left),
            EntityData::ModelCustomisation(0x7f),
        ];
        assert_eq!(
            set_entity_data(ENTITY, &items),
            hex("07
                 00 00 0a
                 06 14 05
                 09 03 41 a0 00 00
                 0f 2a 00
                 10 00 7f
                 ff")
        );
    }

    #[test]
    fn main_hand_and_model_parts_match_the_join_example() {
        let items = [
            EntityData::MainHand(HumanoidArm::Left),
            EntityData::ModelCustomisation(0x7f),
        ];
        assert_eq!(set_entity_data(1, &items), hex("01 0f 2a 00 10 00 7f ff"));
    }

    #[test]
    fn fall_flying_refusal_matches_official_bytes() {
        let packet = SetEntityData {
            entity_id: ENTITY,
            items: &[EntityData::SharedFlags(0)],
        };
        assert_eq!(body(&packet), hex("07 00 00 00 ff"));
        assert_eq!(frame(&packet), hex("06 65 07 00 00 00 ff"));

        let sprinting = [EntityData::SharedFlags(ENTITY_FLAG_SPRINTING)];
        assert_eq!(set_entity_data(ENTITY, &sprinting), hex("07 00 00 08 ff"));
    }

    #[test]
    fn update_attributes_matches_official_bytes() {
        let block_range = [AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }];
        let entity_range = [AttributeModifier {
            id: "minecraft:creative_mode_entity_range",
            amount: 2.0,
            operation: AttributeOperation::AddValue,
        }];
        let packet = UpdateAttributes {
            entity_id: 1,
            attributes: &[
                AttributeSnapshot {
                    attribute: 8,
                    base: 4.5,
                    modifiers: &block_range,
                },
                AttributeSnapshot {
                    attribute: 13,
                    base: 3.0,
                    modifiers: &entity_range,
                },
            ],
        };

        let mut expected = hex("01 02 08 40 12 00 00 00 00 00 00 01 23");
        expected.extend_from_slice(b"minecraft:creative_mode_block_range");
        expected.extend(hex("3f e0 00 00 00 00 00 00 00"));
        expected.extend(hex("0d 40 08 00 00 00 00 00 00 01 24"));
        expected.extend_from_slice(b"minecraft:creative_mode_entity_range");
        expected.extend(hex("40 00 00 00 00 00 00 00 00"));

        assert_eq!(expected.len(), 113);
        assert_eq!(body(&packet), expected);
        // ID 大于 127 时占两个字节。
        assert_eq!(frame(&packet)[1..3], [0x86, 0x01]);
    }

    #[test]
    fn update_attributes_matches_the_single_attribute_examples() {
        let modifiers = [AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }];
        let attributes = [AttributeSnapshot {
            attribute: 8,
            base: 4.5,
            modifiers: &modifiers,
        }];

        for (entity_id, entity_hex) in [(ENTITY, "07"), (5, "05")] {
            let mut expected = hex(entity_hex);
            expected.extend(hex("01 08 40 12 00 00 00 00 00 00 01 23"));
            expected.extend_from_slice(b"minecraft:creative_mode_block_range");
            expected.extend(hex("3f e0 00 00 00 00 00 00 00"));

            let packet = UpdateAttributes {
                entity_id,
                attributes: &attributes,
            };
            assert_eq!(body(&packet), expected);
        }
    }

    #[test]
    fn set_equipment_needs_at_least_one_slot() {
        assert_eq!(SetEquipment::new(ENTITY, &[]), None);
    }

    #[test]
    fn set_equipment_marks_every_slot_but_the_last() {
        let stone = ItemStack::new(1, 1);
        let empty = ItemStack::empty();
        let slots = [
            (EquipmentSlot::MainHand, &stone),
            (EquipmentSlot::OffHand, &empty),
        ];
        let packet = SetEquipment::new(5, &slots).unwrap();
        assert_eq!(body(&packet), hex("05 80 01 01 00 00 01 00"));
    }

    #[test]
    fn set_equipment_matches_the_three_slot_example() {
        let stone = ItemStack::new(1, 64);
        let empty = ItemStack::empty();
        let helmet = ItemStack::new(1084, 1);
        let slots = [
            (EquipmentSlot::MainHand, &stone),
            (EquipmentSlot::OffHand, &empty),
            (EquipmentSlot::Head, &helmet),
        ];
        let packet = SetEquipment::new(ENTITY, &slots).unwrap();
        assert_eq!(
            body(&packet),
            hex("07
                 80 40 01 00 00
                 81 00
                 05 01 bc 08 00 00")
        );
    }

    #[test]
    fn set_equipment_forwards_the_components() {
        let stack = ItemStack {
            item: 1,
            count: 2,
            added: vec![(1, vec![0x10])],
            removed: vec![12],
        };
        let slots = [(EquipmentSlot::MainHand, &stack)];
        let packet = SetEquipment::new(ENTITY, &slots).unwrap();
        assert_eq!(body(&packet), hex("07 00 02 01 01 01 01 10 0c"));
    }

    #[test]
    fn swing_animation_matches_official_bytes() {
        let main_hand = SwingAnimation::default_swing(ENTITY, Hand::Main);
        assert_eq!(body(&main_hand), hex("07 00 01 06"));
        assert_eq!(frame(&main_hand), hex("05 7b 07 00 01 06"));

        let stab = SwingAnimation {
            entity_id: ENTITY,
            hand: Hand::Off,
            animation: SwingAnimationType::Stab,
            duration: 10,
        };
        assert_eq!(body(&stab), hex("07 01 02 0a"));
        assert_eq!(
            body(&SwingAnimation::default_swing(5, Hand::Main)),
            hex("05 00 01 06")
        );
    }
}
