//! 游戏事件：`GameEvent` 注册表、通知半径和振动频率表（`VibrationSystem`）。

pub mod dispatch;
pub mod registry;
pub mod vibration;

/// `GameEvent` 注册表里的条目，顺序就是注册顺序，判别值即网络 / 注册表 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GameEvent {
    BlockActivate,
    BlockAttach,
    BlockChange,
    BlockClose,
    BlockDeactivate,
    BlockDestroy,
    BlockDetach,
    BlockOpen,
    BlockPlace,
    Bounce,
    ContainerClose,
    ContainerOpen,
    Drink,
    Eat,
    ElytraGlide,
    EntityDamage,
    EntityDie,
    EntityDismount,
    EntityInteract,
    EntityMount,
    EntityPlace,
    EntityAction,
    Equip,
    Explode,
    Flap,
    FluidPickup,
    FluidPlace,
    HitGround,
    InstrumentPlay,
    ItemInteractFinish,
    ItemInteractStart,
    JukeboxPlay,
    JukeboxStopPlay,
    LightningStrike,
    NoteBlockPlay,
    PrimeFuse,
    ProjectileLand,
    ProjectileShoot,
    SculkSensorTendrilsClicking,
    Shear,
    Shriek,
    Splash,
    Step,
    Swim,
    Teleport,
    Unequip,
    Resonate1,
    Resonate2,
    Resonate3,
    Resonate4,
    Resonate5,
    Resonate6,
    Resonate7,
    Resonate8,
    Resonate9,
    Resonate10,
    Resonate11,
    Resonate12,
    Resonate13,
    Resonate14,
    Resonate15,
}

use GameEvent as E;

/// 全部条目，按注册顺序。
pub const ALL: [GameEvent; 61] = [
    E::BlockActivate,
    E::BlockAttach,
    E::BlockChange,
    E::BlockClose,
    E::BlockDeactivate,
    E::BlockDestroy,
    E::BlockDetach,
    E::BlockOpen,
    E::BlockPlace,
    E::Bounce,
    E::ContainerClose,
    E::ContainerOpen,
    E::Drink,
    E::Eat,
    E::ElytraGlide,
    E::EntityDamage,
    E::EntityDie,
    E::EntityDismount,
    E::EntityInteract,
    E::EntityMount,
    E::EntityPlace,
    E::EntityAction,
    E::Equip,
    E::Explode,
    E::Flap,
    E::FluidPickup,
    E::FluidPlace,
    E::HitGround,
    E::InstrumentPlay,
    E::ItemInteractFinish,
    E::ItemInteractStart,
    E::JukeboxPlay,
    E::JukeboxStopPlay,
    E::LightningStrike,
    E::NoteBlockPlay,
    E::PrimeFuse,
    E::ProjectileLand,
    E::ProjectileShoot,
    E::SculkSensorTendrilsClicking,
    E::Shear,
    E::Shriek,
    E::Splash,
    E::Step,
    E::Swim,
    E::Teleport,
    E::Unequip,
    E::Resonate1,
    E::Resonate2,
    E::Resonate3,
    E::Resonate4,
    E::Resonate5,
    E::Resonate6,
    E::Resonate7,
    E::Resonate8,
    E::Resonate9,
    E::Resonate10,
    E::Resonate11,
    E::Resonate12,
    E::Resonate13,
    E::Resonate14,
    E::Resonate15,
];

const NAMES: [&str; 61] = [
    "block_activate",
    "block_attach",
    "block_change",
    "block_close",
    "block_deactivate",
    "block_destroy",
    "block_detach",
    "block_open",
    "block_place",
    "bounce",
    "container_close",
    "container_open",
    "drink",
    "eat",
    "elytra_glide",
    "entity_damage",
    "entity_die",
    "entity_dismount",
    "entity_interact",
    "entity_mount",
    "entity_place",
    "entity_action",
    "equip",
    "explode",
    "flap",
    "fluid_pickup",
    "fluid_place",
    "hit_ground",
    "instrument_play",
    "item_interact_finish",
    "item_interact_start",
    "jukebox_play",
    "jukebox_stop_play",
    "lightning_strike",
    "note_block_play",
    "prime_fuse",
    "projectile_land",
    "projectile_shoot",
    "sculk_sensor_tendrils_clicking",
    "shear",
    "shriek",
    "splash",
    "step",
    "swim",
    "teleport",
    "unequip",
    "resonate_1",
    "resonate_2",
    "resonate_3",
    "resonate_4",
    "resonate_5",
    "resonate_6",
    "resonate_7",
    "resonate_8",
    "resonate_9",
    "resonate_10",
    "resonate_11",
    "resonate_12",
    "resonate_13",
    "resonate_14",
    "resonate_15",
];

/// `GameEvent.DEFAULT_NOTIFICATION_RADIUS`。
pub const DEFAULT_NOTIFICATION_RADIUS: i32 = 16;

impl GameEvent {
    /// 注册表 id。
    pub fn id(self) -> u8 {
        self as u8
    }

    /// 按 id 取条目，越界返回 `None`。
    pub fn from_id(id: u8) -> Option<Self> {
        ALL.get(usize::from(id)).copied()
    }

    /// 不带命名空间的名字。
    pub fn name(self) -> &'static str {
        NAMES[usize::from(self.id())]
    }

    /// 按名字取条目，可带 `minecraft:` 前缀。
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        NAMES.iter().position(|n| *n == name).map(|i| ALL[i])
    }

    /// `GameEvent.notificationRadius`。
    pub fn notification_radius(self) -> i32 {
        match self {
            E::JukeboxPlay | E::JukeboxStopPlay => 10,
            E::Shriek => 32,
            _ => DEFAULT_NOTIFICATION_RADIUS,
        }
    }

    /// `VibrationSystem.getGameEventFrequency`，表里没有的是 0。
    pub fn vibration_frequency(self) -> i32 {
        match self {
            E::Step | E::Swim | E::Flap => 1,
            E::ProjectileLand | E::HitGround | E::Splash | E::Bounce => 2,
            E::ItemInteractFinish | E::ProjectileShoot | E::InstrumentPlay => 3,
            E::EntityAction | E::ElytraGlide | E::Unequip => 4,
            E::EntityDismount | E::Equip => 5,
            E::EntityInteract | E::Shear | E::EntityMount => 6,
            E::EntityDamage => 7,
            E::Drink | E::Eat => 8,
            E::ContainerClose | E::BlockClose | E::BlockDeactivate | E::BlockDetach => 9,
            E::ContainerOpen
            | E::BlockOpen
            | E::BlockActivate
            | E::BlockAttach
            | E::PrimeFuse
            | E::NoteBlockPlay => 10,
            E::BlockChange => 11,
            E::BlockDestroy | E::FluidPickup => 12,
            E::BlockPlace | E::FluidPlace => 13,
            E::EntityPlace | E::LightningStrike | E::Teleport => 14,
            E::EntityDie | E::Explode => 15,
            _ => self.resonance_frequency().unwrap_or(0),
        }
    }

    /// `RESONATE_n` 返回 n。
    fn resonance_frequency(self) -> Option<i32> {
        let id = i32::from(self.id());
        let first = i32::from(E::Resonate1.id());
        (id >= first).then_some(id - first + 1)
    }

    /// `#minecraft:vibrations`：`VibrationUser.getListenableEvents` 的缺省标签。
    pub fn in_vibrations_tag(self) -> bool {
        game_event_tag_contains("minecraft:vibrations", self.id())
    }

    /// `#minecraft:ignore_vibrations_sneaking`。
    pub fn in_ignore_vibrations_sneaking_tag(self) -> bool {
        game_event_tag_contains("minecraft:ignore_vibrations_sneaking", self.id())
    }

    /// `#minecraft:shrieker_can_listen`：幽匿尖啸体只听的标签。
    pub fn in_shrieker_can_listen_tag(self) -> bool {
        game_event_tag_contains("minecraft:shrieker_can_listen", self.id())
    }

    /// `VibrationSystem.RESONANCE_EVENTS[frequency - 1]`。
    pub fn resonate(frequency: i32) -> Option<Self> {
        if !(1..=15).contains(&frequency) {
            return None;
        }
        // 范围已检查，转换不会失败。
        let offset = u8::try_from(frequency - 1).ok()?;
        Self::from_id(E::Resonate1.id() + offset)
    }
}

/// 查 `minecraft:game_event` 注册表组里的一个标签。
fn game_event_tag_contains(name: &str, id: u8) -> bool {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:game_event")
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .is_some_and(|tag| tag.ids.contains(&i32::from(id)))
}

/// `VibrationSystem.getRedstoneStrengthForDistance`。
pub fn redstone_strength_for_distance(distance: f32, radius: i32) -> i32 {
    let per_block = 15.0 / f64::from(radius);
    let drop = (per_block * f64::from(distance)).floor();
    // floor 后的 double 转 int 照 Java 的饱和语义。
    let drop = drop as i32;
    1.max(15i32.saturating_sub(drop))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_round_trip() {
        for (i, event) in ALL.iter().enumerate() {
            assert_eq!(usize::from(event.id()), i);
            assert_eq!(GameEvent::from_name(event.name()), Some(*event));
        }
        assert_eq!(GameEvent::from_id(61), None);
        assert_eq!(GameEvent::from_name("minecraft:step"), Some(E::Step));
        assert_eq!(GameEvent::from_name("nope"), None);
    }

    #[test]
    fn radii() {
        assert_eq!(E::Shriek.notification_radius(), 32);
        assert_eq!(E::JukeboxPlay.notification_radius(), 10);
        assert_eq!(E::Step.notification_radius(), 16);
    }

    #[test]
    fn frequencies() {
        assert_eq!(E::Step.vibration_frequency(), 1);
        assert_eq!(E::BlockPlace.vibration_frequency(), 13);
        assert_eq!(E::EntityDie.vibration_frequency(), 15);
        assert_eq!(E::Shriek.vibration_frequency(), 0);
        assert_eq!(E::ItemInteractStart.vibration_frequency(), 0);
        for f in 1..=15 {
            let e = GameEvent::resonate(f).unwrap();
            assert_eq!(e.vibration_frequency(), f);
        }
        assert_eq!(GameEvent::resonate(0), None);
        assert_eq!(GameEvent::resonate(16), None);
    }

    #[test]
    fn redstone_strength() {
        assert_eq!(redstone_strength_for_distance(0.0, 8), 15);
        assert_eq!(redstone_strength_for_distance(1.0, 8), 14);
        assert_eq!(redstone_strength_for_distance(8.0, 8), 1);
        assert_eq!(redstone_strength_for_distance(16.0, 16), 1);
        assert_eq!(redstone_strength_for_distance(5.5, 16), 10);
    }
}
