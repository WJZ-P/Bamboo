//! 船第二期的 Java 对拍取样：菜单标题、生物乘客落位和运输船整棵 NBT 往返。
//!
//! 只在 `oracle-samples` 下编译。`EntityOracle.java` 在官方假世界里取同一组事实，
//! 逐键比较，见 `mc-bamboo-vanilla entity-oracle`。
//!
//! Boat oracle samples: menu title, mob passenger landing and full chest-boat NBT
//! round trip. Compiled only under `oracle-samples`.

use std::sync::{Arc, Mutex};

use mc_bamboo_blocks::{
    WorldBorder,
    archive::{read_entity, write_entity},
    entity::{Entity, EntityKind},
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, items, registries::entity_type as ids};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    codec::{Reader, decode_varint},
    frame::{MAX_FRAME_LEN, find_frame},
    item::ItemStack,
    packet::play::{ClientInformation, GameMode},
};
use mc_bamboo_world::flat;
use uuid::Uuid;

use crate::{
    Outgoing, PlayerSink, inbound::PlayerId, player::Player, region::Region,
    riding::dismount::boat_dismount_location_for_mob,
};

/// `replay_boat_facts` 的返回值：标题翻译键、生物落位、官方整棵 NBT 往返后的
/// Bamboo 存档标签、被抽中的运输船槽 0 物品、随机源抽数前后的内部种子。
///
/// Results for the Java comparison: translation key, mob landing, the Bamboo
/// archive tag after a full official-NBT round trip, the randomly selected
/// container's slot-0 item and the raw world-random seeds around the draw.
#[derive(Debug, Clone, PartialEq)]
pub struct BoatFactsSample {
    pub title: String,
    pub land_x: i64,
    pub land_y: i64,
    pub land_z: i64,
    pub reloaded: String,
    pub selected_item: i32,
    pub seed_before: i64,
    pub seed_after: i64,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl PlayerSink for Capture {
    fn send(&mut self, frames: Outgoing) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {}
}

/// 官方 `Entity.save` 的 NBT → Bamboo `read_entity` → `write_entity` 再写出。
fn archive_round_trip(nbt: &str) -> Result<String, String> {
    let official = mc_bamboo_nbt::parse_snbt(nbt).map_err(|error| format!("{error:?}"))?;
    let row = read_entity(&official, 1 << 24)
        .map_err(|error| format!("{error}"))?
        .ok_or("read_entity returned none")?;
    let entity = row.into_entity();
    let again = write_entity(&entity.split()).map_err(|error| format!("{error}"))?;
    Ok(again.to_snbt())
}

/// 取样用的船、乘客和漏斗状态都走生产代码：
/// `menu::open_boat`、`riding::dismount::boat_dismount_location_for_mob`、
/// `hopper::suck_from_entity_container`（由 `World::tick_block_entities` 驱动）、
/// `archive::write_entity` / `read_entity`。
///
/// Every sampled fact goes through production code: `menu::open_boat`, the mob
/// boat dismount helper, hopper container selection and the entity archive
/// codecs.
pub fn replay_boat_facts(nbt: &str, seed: i64) -> Result<BoatFactsSample, String> {
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(1),
        "Kitten".to_owned(),
        GameMode::Creative,
        ClientInformation::default(),
        Vec3::new(0.5, f64::from(flat::surface_y()), 0.5),
    ));
    // 官方假世界的脚下是 y=63 的石头；落位逻辑只看这一格和四周。
    for x in -2..=2 {
        for z in -2..=2 {
            region
                .chunks
                .set_block(BlockPos::new(x, 63, z), blocks::STONE);
        }
    }
    let at = Vec3::new(0.5, 64.0, 0.5);
    let first = region.entities.spawn(Entity::boat(ids::OAK_CHEST_BOAT, at));
    let second = region
        .entities
        .spawn(Entity::boat(ids::SPRUCE_CHEST_BOAT, at));
    for (uid, item) in [(first, items::DIAMOND), (second, items::BREAD)] {
        region.entities.with_mut(uid, |entity| {
            if let EntityKind::Boat(data) = &mut entity.kind {
                data.chest.as_mut().expect("chest boat").items[0] = ItemStack::new(item, 3);
            }
        });
    }

    // 1. 打开运输船菜单，从真实的 OpenScreen 包里读出标题翻译键。
    if !crate::menu::open_boat(&mut region, id, first) {
        return Err("open_boat failed".to_owned());
    }
    region.links.flush();
    let bytes = std::mem::take(&mut *capture.0.lock().unwrap());
    let mut rest = bytes.as_slice();
    let mut title = None;
    while let Ok(Some(bounds)) = find_frame(rest, MAX_FRAME_LEN) {
        let end = bounds.header_len + bounds.payload_len;
        let payload = &rest[bounds.header_len..end];
        if let Ok((packet_id, consumed)) = decode_varint(payload)
            && packet_id == mc_bamboo_data::packets::play::clientbound::OPEN_SCREEN
        {
            let mut body = Reader::new(&payload[consumed..]);
            let _ = body.varint();
            let _ = body.varint();
            if let Some(tag) = mc_bamboo_nbt::Reader::new(
                body.remaining(),
                mc_bamboo_nbt::Accounter::new(mc_bamboo_nbt::DEFAULT_QUOTA),
            )
            .any_tag()
            .ok()
            .flatten()
                && let Tag::Compound(entries) = tag
            {
                title = entries.into_iter().find_map(|(key, value)| {
                    (key == "translate" && matches!(&value, Tag::String(_))).then(|| match value {
                        Tag::String(text) => text,
                        _ => String::new(),
                    })
                });
            }
        }
        rest = &rest[end..];
    }
    let title = title.ok_or("open screen title not found")?;

    // 3. 生物乘客的落位：猪宽 0.9、高 0.9、朝向 45°，船位置 (0.5, 64, 0.5)。
    let vehicle = region
        .entities
        .public_at(first)
        .ok_or("boat public snapshot missing")?
        .clone();
    let landing = boat_dismount_location_for_mob(
        &region.chunks,
        &WorldBorder::default(),
        0.9,
        0.9,
        45.0,
        0.0,
        64.0,
        vehicle.position,
        1.375,
        vehicle.bounding_box().max_y,
    )
    .ok_or("boat dismount returned none")?;

    // 2. 漏斗上方两艘重叠的运输船：官方 `getEntityContainer` 抽一次 nextInt(2)。
    let hopper = BlockPos::new(0, 63, 0);
    region.chunks.set_block(hopper, blocks::HOPPER);
    {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.set_block_entity(Box::new(mc_bamboo_blocks::HopperEntity::new(hopper)));
        world.random().set_seed(seed);
        let seed_before = world.random().seed();
        world.tick_block_entities();
        let seed_after = world.random().seed();
        // 漏斗槽 0 里进来的物品就是被抽中的那只船槽 0 的物品。
        // 漏斗的一格容器由方块实体自己的槽位表提供。
        let contents = world.container_contents(hopper);
        let selected_item = contents
            .first()
            .map(|&(item, _)| item)
            .ok_or("hopper container missing")?;
        let reloaded = archive_round_trip(nbt)?;
        return Ok(BoatFactsSample {
            title,
            land_x: landing.x.to_bits() as i64,
            land_y: landing.y.to_bits() as i64,
            land_z: landing.z.to_bits() as i64,
            reloaded,
            selected_item,
            seed_before,
            seed_after,
        });
    }
}
