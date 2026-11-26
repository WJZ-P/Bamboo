//! 盔甲架：官方 `ArmorStand`，`LivingEntity` 一支、没有 AI。
//!
//! 状态收在 [`ArmorStand`] 里，别的生物不读。尺寸有普通 / 小型 / 标记三种，
//! 同步数据是 `DATA_CLIENT_FLAGS` 一个字节加 6 个姿势旋转。存档键见
//! [`write_nbt`] / [`read_nbt`]，受伤的两段式摇晃和破坏见
//! [`super::living::hurt_server`] 的盔甲架分支。
//!
//! The armour stand, vanilla `ArmorStand`: a `LivingEntity` with no AI. Its
//! state lives in [`ArmorStand`]. There are normal, small and marker sizes;
//! the synced data is one `DATA_CLIENT_FLAGS` byte plus six pose rotations.
//! The two-hit wobble and breaking logic lives in the armour-stand branch of
//! [`super::living::hurt_server`].

use mc_bamboo_nbt::Tag;

use crate::archive::tag::{self, put};

use super::Mob;

/// 一个姿势绕三根轴的旋转（度），官方 `net.minecraft.core.Rotations`。
///
/// A pose's rotation around the three axes in degrees, vanilla `Rotations`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotations {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Rotations {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// `Rotations.CODEC`：`[x, y, z]` 三个浮点的列表。
    ///
    /// `Rotations.CODEC`: a list of three floats `[x, y, z]`.
    pub fn to_tag(self) -> Tag {
        Tag::List(vec![
            Tag::Float(self.x),
            Tag::Float(self.y),
            Tag::Float(self.z),
        ])
    }

    /// 读 `Rotations.CODEC`；元素不是浮点或数量不对时按缺省值处理。
    ///
    /// Reads `Rotations.CODEC`; a malformed list falls back to the default.
    pub fn from_tag(tag: &Tag, default: Self) -> Self {
        let Tag::List(values) = tag else {
            return default;
        };
        let mut floats = [default.x, default.y, default.z];
        for (slot, value) in values.iter().take(3).enumerate() {
            if let Tag::Float(value) = value {
                floats[slot] = *value;
            }
        }
        Self::new(floats[0], floats[1], floats[2])
    }
}

/// `DATA_CLIENT_FLAGS` 的位。
pub const CLIENT_FLAG_SMALL: u8 = 1;
pub const CLIENT_FLAG_SHOW_ARMS: u8 = 4;
pub const CLIENT_FLAG_NO_BASEPLATE: u8 = 8;
pub const CLIENT_FLAG_MARKER: u8 = 16;

/// `DisabledSlots` 里「取出」和「放入」两段的位偏移。
pub const DISABLE_TAKING_OFFSET: i32 = 8;
pub const DISABLE_PUTTING_OFFSET: i32 = 16;

pub const DEFAULT_HEAD_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
pub const DEFAULT_BODY_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
pub const DEFAULT_LEFT_ARM_POSE: Rotations = Rotations::new(-10.0, 0.0, -10.0);
pub const DEFAULT_RIGHT_ARM_POSE: Rotations = Rotations::new(-15.0, 0.0, 10.0);
pub const DEFAULT_LEFT_LEG_POSE: Rotations = Rotations::new(-1.0, 0.0, -1.0);
pub const DEFAULT_RIGHT_LEG_POSE: Rotations = Rotations::new(1.0, 0.0, 1.0);

/// 普通 / 小型 / 标记三种尺寸和眼高。
pub const NORMAL_DIMENSIONS: (f32, f32, f32) = (0.5, 1.975, 1.7775);
/// `BABY_DIMENSIONS`：`dims.scale(0.5).withEyeHeight(0.9875)`。
pub const BABY_DIMENSIONS: (f32, f32, f32) = (0.25, 0.9875, 0.9875);
/// `MARKER_DIMENSIONS`：`EntityDimensions.fixed(0, 0)`。
pub const MARKER_DIMENSIONS: (f32, f32, f32) = (0.0, 0.0, 0.0);

/// 盔甲架的专属状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmorStand {
    /// `DATA_CLIENT_FLAGS`：小型、手臂、无底座、标记。
    pub client_flags: u8,
    pub head_pose: Rotations,
    pub body_pose: Rotations,
    pub left_arm_pose: Rotations,
    pub right_arm_pose: Rotations,
    pub left_leg_pose: Rotations,
    pub right_leg_pose: Rotations,
    /// `invisible` 字段，和共享标志里的隐身位是两回事。
    pub invisible: bool,
    /// `disabledSlots`。
    pub disabled_slots: i32,
    /// `lastHit`：上次被敲的岛时钟绝对时刻。
    pub last_hit: i64,
}

impl Default for ArmorStand {
    fn default() -> Self {
        Self {
            client_flags: 0,
            head_pose: DEFAULT_HEAD_POSE,
            body_pose: DEFAULT_BODY_POSE,
            left_arm_pose: DEFAULT_LEFT_ARM_POSE,
            right_arm_pose: DEFAULT_RIGHT_ARM_POSE,
            left_leg_pose: DEFAULT_LEFT_LEG_POSE,
            right_leg_pose: DEFAULT_RIGHT_LEG_POSE,
            invisible: false,
            disabled_slots: 0,
            last_hit: 0,
        }
    }
}

impl ArmorStand {
    pub const fn is_small(&self) -> bool {
        self.client_flags & CLIENT_FLAG_SMALL != 0
    }

    pub const fn show_arms(&self) -> bool {
        self.client_flags & CLIENT_FLAG_SHOW_ARMS != 0
    }

    pub const fn show_base_plate(&self) -> bool {
        self.client_flags & CLIENT_FLAG_NO_BASEPLATE == 0
    }

    pub const fn is_marker(&self) -> bool {
        self.client_flags & CLIENT_FLAG_MARKER != 0
    }

    /// `hasPhysics()`：不是标记、没有 `NoGravity`。
    pub const fn has_physics(&self, no_gravity: bool) -> bool {
        !self.is_marker() && !no_gravity
    }

    /// `getDimensionsMarker(isMarker)`：标记、小型、普通的三种尺寸。
    pub const fn dimensions(&self) -> (f32, f32, f32) {
        if self.is_marker() {
            MARKER_DIMENSIONS
        } else if self.is_small() {
            BABY_DIMENSIONS
        } else {
            NORMAL_DIMENSIONS
        }
    }

    /// `isDisabled(slot)`：`disabledSlots & (1 << slot.getFilterBit(0))`，手槽还要看手臂。
    pub fn is_disabled(&self, slot_id: i32, slot_is_hand: bool) -> bool {
        let bit = slot_id;
        self.disabled_slots & (1 << bit) != 0 || (slot_is_hand && !self.show_arms())
    }

    /// `setBit`。
    fn set_flag(&mut self, mask: u8, enabled: bool) {
        if enabled {
            self.client_flags |= mask;
        } else {
            self.client_flags &= !mask;
        }
    }

    pub fn set_small(&mut self, small: bool) {
        self.set_flag(CLIENT_FLAG_SMALL, small);
    }

    pub fn set_show_arms(&mut self, arms: bool) {
        self.set_flag(CLIENT_FLAG_SHOW_ARMS, arms);
    }

    pub fn set_no_base_plate(&mut self, no_base_plate: bool) {
        self.set_flag(CLIENT_FLAG_NO_BASEPLATE, no_base_plate);
    }

    pub fn set_marker(&mut self, marker: bool) {
        self.set_flag(CLIENT_FLAG_MARKER, marker);
    }

    /// `addAdditionalSaveData`：六个布尔、`DisabledSlots` 和 `Pose`。
    pub fn write_nbt(&self, mob: &Mob, out: &mut Vec<(String, Tag)>) {
        put(out, "Invisible", Tag::Byte(i8::from(self.invisible)));
        put(out, "Small", Tag::Byte(i8::from(self.is_small())));
        put(out, "ShowArms", Tag::Byte(i8::from(self.show_arms())));
        put(out, "DisabledSlots", Tag::Int(self.disabled_slots));
        put(
            out,
            "NoBasePlate",
            Tag::Byte(i8::from(!self.show_base_plate())),
        );
        if self.is_marker() {
            put(out, "Marker", Tag::Byte(1));
        }
        let pose = Tag::Compound(vec![
            ("Head".to_owned(), self.head_pose.to_tag()),
            ("Body".to_owned(), self.body_pose.to_tag()),
            ("LeftArm".to_owned(), self.left_arm_pose.to_tag()),
            ("RightArm".to_owned(), self.right_arm_pose.to_tag()),
            ("LeftLeg".to_owned(), self.left_leg_pose.to_tag()),
            ("RightLeg".to_owned(), self.right_leg_pose.to_tag()),
        ]);
        put(out, "Pose", pose);
        let _ = mob;
    }

    /// `readAdditionalSaveData`：六个布尔、`DisabledSlots` 和 `Pose`。
    pub fn read_nbt(&mut self, tag: &Tag) {
        self.invisible = tag::bool_or(tag, "Invisible", false);
        self.set_small(tag::bool_or(tag, "Small", false));
        self.set_show_arms(tag::bool_or(tag, "ShowArms", false));
        self.disabled_slots = tag::int_or(tag, "DisabledSlots", 0);
        self.set_no_base_plate(tag::bool_or(tag, "NoBasePlate", false));
        self.set_marker(tag::bool_or(tag, "Marker", false));
        if let Some(pose) = tag::field(tag, "Pose") {
            let entries = tag::entries(pose).unwrap_or_default();
            let read = |key: &str, default: Rotations| {
                entries
                    .iter()
                    .find(|(name, _)| name == key)
                    .map_or(default, |(_, value)| Rotations::from_tag(value, default))
            };
            self.head_pose = read("Head", DEFAULT_HEAD_POSE);
            self.body_pose = read("Body", DEFAULT_BODY_POSE);
            self.left_arm_pose = read("LeftArm", DEFAULT_LEFT_ARM_POSE);
            self.right_arm_pose = read("RightArm", DEFAULT_RIGHT_ARM_POSE);
            self.left_leg_pose = read("LeftLeg", DEFAULT_LEFT_LEG_POSE);
            self.right_leg_pose = read("RightLeg", DEFAULT_RIGHT_LEG_POSE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        archive::tag,
        entity::{
            Entity,
            mob::{MobType, nbt},
        },
    };
    use mc_bamboo_core::Vec3;

    #[test]
    fn dimensions_follow_the_client_flags() {
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::default());
        assert_eq!(entity.kind.dimensions(), (0.5, 1.975));
        entity.with_mob(|_, mob| mob.armor_stand.set_small(true));
        assert_eq!(entity.kind.dimensions(), (0.25, 0.9875));
        entity.with_mob(|_, mob| mob.armor_stand.set_marker(true));
        assert_eq!(entity.kind.dimensions(), (0.0, 0.0));
    }

    #[test]
    fn armour_stand_state_survives_a_save_and_load() {
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| {
            let stand = &mut mob.armor_stand;
            stand.set_small(true);
            stand.set_show_arms(true);
            stand.set_no_base_plate(true);
            stand.set_marker(true);
            stand.invisible = true;
            stand.disabled_slots = 0x00ff_ffff;
            stand.head_pose = Rotations::new(1.0, 2.0, 3.0);
            stand.body_pose = Rotations::new(-4.0, -5.0, -6.0);
            stand.left_arm_pose = Rotations::new(7.0, 8.0, 9.0);
            stand.right_arm_pose = Rotations::new(10.0, 11.0, 12.0);
            stand.left_leg_pose = Rotations::new(13.0, 14.0, 15.0);
            stand.right_leg_pose = Rotations::new(16.0, 17.0, 18.0);
        });
        let saved = nbt::write_mob(&entity.split()).expect("an armour stand saves");
        assert_eq!(tag::byte_or(&saved, "Invisible", 0), 1);
        assert_eq!(tag::byte_or(&saved, "Small", 0), 1);
        assert_eq!(tag::byte_or(&saved, "ShowArms", 0), 1);
        assert_eq!(tag::byte_or(&saved, "NoBasePlate", 0), 1);
        assert_eq!(tag::byte_or(&saved, "Marker", 0), 1);
        assert_eq!(tag::int_or(&saved, "DisabledSlots", 0), 0x00ff_ffff);
        let pose = tag::field(&saved, "Pose").expect("Pose is written");
        let entries = tag::entries(pose).expect("Pose is a compound");
        let head = entries
            .iter()
            .find(|(key, _)| key == "Head")
            .map(|(_, value)| value)
            .expect("Head pose is written");
        assert_eq!(
            Rotations::from_tag(head, DEFAULT_HEAD_POSE),
            Rotations::new(1.0, 2.0, 3.0)
        );

        let row = nbt::read_mob(&saved, "minecraft:armor_stand", 7)
            .expect("the tag loads")
            .expect("armour stands are modelled");
        let entity = row.into_entity();
        let mob = entity.kind.mob().expect("a mob");
        assert!(mob.no_ai);
        assert_eq!(
            mob.armor_stand.client_flags,
            CLIENT_FLAG_SMALL
                | CLIENT_FLAG_SHOW_ARMS
                | CLIENT_FLAG_NO_BASEPLATE
                | CLIENT_FLAG_MARKER
        );
        assert!(mob.armor_stand.invisible);
        assert_eq!(mob.armor_stand.disabled_slots, 0x00ff_ffff);
        assert_eq!(mob.armor_stand.head_pose, Rotations::new(1.0, 2.0, 3.0));
        assert_eq!(mob.armor_stand.body_pose, Rotations::new(-4.0, -5.0, -6.0));
        assert_eq!(mob.armor_stand.left_arm_pose, Rotations::new(7.0, 8.0, 9.0));
        assert_eq!(
            mob.armor_stand.right_arm_pose,
            Rotations::new(10.0, 11.0, 12.0)
        );
        assert_eq!(
            mob.armor_stand.left_leg_pose,
            Rotations::new(13.0, 14.0, 15.0)
        );
        assert_eq!(
            mob.armor_stand.right_leg_pose,
            Rotations::new(16.0, 17.0, 18.0)
        );
        // 标记盔甲架的碰撞盒是 (0, 0)。
        assert_eq!(entity.dimensions, (0.0, 0.0));
    }
}
