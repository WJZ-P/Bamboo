use crate::{inbound, outgoing, player, player_consume, region};

/// S5 收纳袋长按逐件倒出对拍场景。
///
/// S5 fixture for pouring items out of a held bundle.
#[derive(Debug, Clone)]
pub struct ReplayBundleUse {
    pub ticks: usize,
    /// [物品 ID, 数量, ...]，和官方 `new BundleContents(list)` 的顺序一致。
    ///
    /// [item id, count, ...], in the same order as vanilla `new BundleContents(list)`.
    pub contents: Vec<i32>,
    pub selected: i32,
}

/// S5 逐刻状态：使用倒计时、收纳袋内容、累计掉出的物品实体（原始位）。
///
/// S5 per-tick state: use countdown, bundle contents, cumulative dropped item
/// entities (raw bits).
#[derive(Debug, Clone, PartialEq)]
pub struct BundleUseStep {
    pub using: i32,
    pub selected: i32,
    pub contents: Vec<[i64; 2]>,
    pub drops: Vec<[i64; 10]>,
}

/// 每刻的包，顺序为使用者、旁观者。
///
/// Packets per tick, in user then observer order.
pub type BundleUsePackets = [Vec<Vec<u8>>; 2];

/// S5 收纳袋长按场景的正式重放。
///
/// Replay bundle use through the production item tick.
pub fn replay_bundle_use(scene: &ReplayBundleUse) -> (Vec<BundleUseStep>, Vec<BundleUsePackets>) {
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry, stack::ItemStackTemplate},
        item::ItemStack,
        packet::play::{ClientInformation, GameMode},
    };
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl inbound::PlayerSink for Capture {
        fn send(&mut self, frames: outgoing::Outgoing) -> bool {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(frames.to_raw_bytes());
            true
        }
        fn close(&mut self) {}
    }
    let take = |capture: &Capture| -> Vec<Vec<u8>> {
        let bytes = std::mem::take(&mut *capture.0.lock().unwrap());
        let mut rest = bytes.as_slice();
        let mut out = Vec::new();
        while !rest.is_empty() {
            let Some(bounds) = mc_bamboo_protocol::frame::find_frame(
                rest,
                mc_bamboo_protocol::frame::MAX_FRAME_LEN,
            )
            .ok()
            .flatten() else {
                break;
            };
            let end = bounds.header_len + bounds.payload_len;
            out.push(rest[bounds.header_len..end].to_vec());
            rest = &rest[end..];
        }
        out
    };
    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(1 << 24);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Oracle0".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    player.random =
        mc_bamboo_blocks::entity::JavaRandom::new(mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED);
    // 官方假世界的 `Level.random` 是 `RandomSource.create(1)`，
    // `soundSeedGenerator` 是 `RandomSource.create(12345)`；两边对齐种子。
    //
    // The vanilla fake world seeds `Level.random` with `RandomSource.create(1)`
    // and `soundSeedGenerator` with `RandomSource.create(12345)`; match both.
    region.random = mc_bamboo_blocks::LevelRandom::new(1);
    region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);
    // 造内容物：和官方 `new BundleContents(list)` 一样按给定顺序写入组件。
    // Build the contents in the given order, like vanilla `new BundleContents(list)`.
    let mut templates = Vec::with_capacity(scene.contents.len() / 2);
    let mut index = 0;
    while index + 1 < scene.contents.len() {
        templates.push(ItemStackTemplate {
            item: scene.contents[index],
            count: scene.contents[index + 1],
            components: ComponentPatch::new(),
        });
        index += 2;
    }
    let mut patch = ComponentPatch::new();
    patch.put(
        mc_bamboo_data::registries::data_component_type::BUNDLE_CONTENTS,
        PatchEntry::Set(ComponentValue::BundleContents(templates)),
    );
    let (added, removed) = patch.to_opaque();
    let mut bundle = ItemStack {
        item: mc_bamboo_data::items::BUNDLE,
        count: 1,
        bundle_selected: None,
        added,
        removed,
    };
    if scene.selected >= 0 {
        mc_bamboo_protocol::components::bundle::toggle_selected(&mut bundle, scene.selected);
    }
    let slot = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[slot] = bundle;
    // 初次容器同步占一个 state_id。
    // Initial container synchronization consumes one state ID.
    player.inventory.set_state_id(1);
    let capture = Capture::default();
    let observer_id = inbound::PlayerId(id.0 + 1);
    let observer = Capture::default();
    region.links.add(observer_id, Box::new(observer.clone()));
    region.insert_player(player::Player::new(
        observer_id,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(1.5, 64.0, 0.5),
    ));
    region.links.add(id, Box::new(capture.clone()));
    region.insert_player(player);
    region.links.flush();
    let _ = take(&capture);
    let _ = take(&observer);

    let step_of = |region: &region::Region| -> BundleUseStep {
        let p = &region.players[&id];
        let held = p.inventory.held(Hand::Main);
        let contents = mc_bamboo_protocol::components::bundle::contents(held)
            .unwrap_or_default()
            .iter()
            .map(|t| [i64::from(t.item), i64::from(t.count)])
            .collect();
        let mut drops = Vec::new();
        for entity in region.entities.iter() {
            if let Some(ground) = entity.ground() {
                drops.push([
                    i64::from(ground.stack.item),
                    i64::from(ground.stack.count),
                    entity.position.x.to_bits() as i64,
                    entity.position.y.to_bits() as i64,
                    entity.position.z.to_bits() as i64,
                    entity.delta.x.to_bits() as i64,
                    entity.delta.y.to_bits() as i64,
                    entity.delta.z.to_bits() as i64,
                    i64::from(ground.pickup_delay),
                    i64::from(ground.age),
                ]);
            }
        }
        BundleUseStep {
            using: p.use_item.as_ref().map_or(0, |u| u.remaining),
            selected: held.bundle_selected.map_or(-1, |n| n as i32),
            contents,
            drops,
        }
    };
    let mut steps = vec![step_of(&region)];
    let mut packets = vec![[Vec::new(), Vec::new()]];
    crate::interaction::on_use_item(&mut region, id, Hand::Main, 1, (0.0, 0.0));
    for _ in 1..=scene.ticks {
        player_consume::tick(&mut region);
        steps.push(step_of(&region));
        region.links.flush();
        // 逐连接比较音效和手中收纳袋的容器槽位更新。
        // Compare sounds and the held bundle's container slot update per connection.
        packets.push([&capture, &observer].map(|sink| {
            use mc_bamboo_data::packets::play::clientbound::{CONTAINER_SET_SLOT, SOUND};
            take(sink)
                .into_iter()
                .filter(|body| {
                    mc_bamboo_protocol::codec::decode_varint(body)
                        .is_ok_and(|(packet, _)| matches!(packet, SOUND | CONTAINER_SET_SLOT))
                })
                .collect()
        }));
    }
    (steps, packets)
}
