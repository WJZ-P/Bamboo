//! 制图台的格子限制、结果预览与取出后处理，对应 `CartographyTableMenu` 与
//! `MapItem.onCraftedPostProcess`。
//!
//! 真实世界直接查 `Region.maps`（找不到地图数据时按官方直接 return，不碰结果格）；
//! 菜单 oracle 的假世界继续用固定的比例和锁定状态逐槽对拍。
//!
//! Cartography table slot rules, result preview, and crafting post-processing
//! matching `CartographyTableMenu` and `MapItem.onCraftedPostProcess`.

use mc_bamboo_data::{item_tag, items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};

use super::item_patch::{assign, resolved};

/// `MapPostProcessing` 枚举：LOCK=0、SCALE=1。
pub(super) const LOCK: i32 = 0;
pub(super) const SCALE: i32 = 1;
/// `MapItemSavedData.MAX_SCALE`。
pub(super) const MAX_SCALE: i32 = 4;

/// `CartographyTableMenu$3.mayPlace`：地图槽只收带 `map_id` 的物品。
pub(super) fn is_map(stack: &ItemStack) -> bool {
    resolved(stack, kinds::MAP_ID).is_some()
}

/// `CartographyTableMenu$4.mayPlace`。
pub(super) fn is_additional(stack: &ItemStack) -> bool {
    matches!(stack.item, items::PAPER | items::MAP | items::GLASS_PANE)
}

/// `quickMoveStack` 里判断背包物品能不能进地图槽。
pub(super) fn has_map_id(stack: &ItemStack) -> bool {
    resolved(stack, kinds::MAP_ID).is_some()
}

/// `ItemTags.EXTENDABLE_MAPS`。
pub(super) fn extendable(stack: &ItemStack) -> bool {
    item_tag("minecraft:extendable_maps").is_some_and(|ids| ids.contains(&stack.item))
}

/// `CartographyTableMenu.lambda$setupResultSlot$0` 里能靠物品组件处理的三支。
///
/// 真实世界没有地图存档，按官方「找不到地图数据」的分支处理：结果格保持原样，
/// 重放里由假世界提供 `scale`/`locked`，能按官方算出复制、放大和锁定。
pub(super) fn result(
    map: &ItemStack,
    additional: &ItemStack,
    old: &ItemStack,
    scale: i32,
    locked: bool,
) -> ItemStack {
    if additional.item == items::PAPER && extendable(map) && !locked && scale < MAX_SCALE {
        let mut result = copy_with_count(map, 1);
        set_post_processing(&mut result, SCALE);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    } else if additional.item == items::GLASS_PANE && !locked {
        let mut result = copy_with_count(map, 1);
        set_post_processing(&mut result, LOCK);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    } else if additional.item == items::MAP {
        let result = copy_with_count(map, 2);
        return if result.matches(old) {
            old.clone()
        } else {
            result
        };
    }
    ItemStack::empty()
}

/// 复制一份数量为 `count` 的地图，保留组件补丁。
pub(super) fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

/// 官方 `setupResultSlot` 里给结果写 `MAP_POST_PROCESSING` 的那一步。
pub(super) fn set_post_processing(stack: &mut ItemStack, kind: i32) {
    assign(
        stack,
        kinds::MAP_POST_PROCESSING,
        Some(ComponentValue::MapPostProcessing(kind)),
    );
}

/// 物品是否带有 `MAP_POST_PROCESSING` 组件。
///
/// Whether the stack carries a `MAP_POST_PROCESSING` component.
pub(super) fn has_post_processing(stack: &ItemStack) -> bool {
    !stack.is_empty() && resolved(stack, kinds::MAP_POST_PROCESSING).is_some()
}

/// 官方 `MapItem.onCraftedPostProcess`：移除 `MAP_POST_PROCESSING` 组件，
/// 并在服务端地图表里按 `LOCK` / `SCALE` 分配新 `map_id` 与对应的锁定 / 放大数据。
///
/// Official `MapItem.onCraftedPostProcess`: strip `MAP_POST_PROCESSING` and,
/// using the server `MapStore`, allocate a new `map_id` with `locked()` or
/// `scaled()` data.
pub(super) fn post_process_map(stack: &mut ItemStack, maps: &mut crate::map::MapStore) {
    if stack.is_empty() || stack.item != items::FILLED_MAP {
        return;
    }
    let Some(ComponentValue::MapPostProcessing(kind)) = resolved(stack, kinds::MAP_POST_PROCESSING)
    else {
        return;
    };
    assign(stack, kinds::MAP_POST_PROCESSING, None);
    let Some(old_id) = crate::map::stack_map_id(stack) else {
        return;
    };
    let next_data = match kind {
        LOCK => maps.get(old_id).map(crate::map::MapItemSavedData::locked),
        SCALE => maps.get(old_id).map(crate::map::MapItemSavedData::scaled),
        _ => None,
    };
    if let Some(data) = next_data {
        let new_id = maps.next_map_id();
        maps.insert(new_id, data);
        assign(stack, kinds::MAP_ID, Some(ComponentValue::MapId(new_id)));
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::components::ComponentValue;

    use super::*;

    fn stack(item: i32, count: i32) -> ItemStack {
        ItemStack::new(item, count)
    }

    #[test]
    fn only_the_three_additional_items_are_accepted() {
        assert!(is_additional(&stack(items::PAPER, 1)));
        assert!(is_additional(&stack(items::MAP, 1)));
        assert!(is_additional(&stack(items::GLASS_PANE, 1)));
        assert!(!is_additional(&stack(items::STICK, 1)));
        assert!(!is_map(&stack(items::FILLED_MAP, 1)));
    }

    #[test]
    fn a_map_id_component_makes_the_map_slot_accept_it() {
        let mut map = stack(items::FILLED_MAP, 1);
        assign(&mut map, kinds::MAP_ID, Some(ComponentValue::MapId(7)));
        assert!(is_map(&map));
        assert!(has_map_id(&map));
        assert!(extendable(&map));
    }

    #[test]
    fn post_processing_values_match_the_official_enum() {
        assert_eq!(LOCK, 0);
        assert_eq!(SCALE, 1);
        let mut map = stack(items::FILLED_MAP, 1);
        set_post_processing(&mut map, SCALE);
        assert_eq!(
            resolved(&map, kinds::MAP_POST_PROCESSING),
            Some(ComponentValue::MapPostProcessing(SCALE))
        );
    }

    #[test]
    fn real_cartography_table_scales_locks_copies_and_handles_missing_maps() {
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex};

        use mc_bamboo_core::{BlockPos, Vec3};
        use mc_bamboo_data::{blocks, packets::play::clientbound};
        use mc_bamboo_protocol::{
            codec::Reader,
            frame::{MAX_FRAME_LEN, find_frame},
            packet::play::{ClientInformation, ContainerInput, GameMode, HashedStack},
        };
        use uuid::Uuid;

        use crate::{
            inbound::PlayerId,
            inbound::PlayerSink,
            map::{
                Brightness, MapItemSavedData, create_map, make_filled_map, map_color, packed_color,
                stack_map_id,
            },
            menu::{ClickPacket, station},
            outgoing::Outgoing,
            player::Player,
            region::Region,
        };

        #[derive(Default, Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl PlayerSink for Sink {
            fn send(&mut self, frames: Outgoing) -> bool {
                self.0.lock().unwrap().extend(frames.to_raw_bytes());
                true
            }
            fn close(&mut self) {}
        }

        fn click(
            region: &mut Region,
            player: PlayerId,
            container_id: i32,
            slot: i16,
            button: i8,
            input: ContainerInput,
        ) {
            let state_id = region
                .menus
                .get(&player)
                .and_then(|m| m.station.as_ref())
                .map_or(0, |s| s.replay_view().state_id);
            station::click_menu(
                region,
                player,
                ClickPacket {
                    container_id,
                    state_id,
                    slot,
                    button,
                    input,
                    changed_slots: BTreeMap::new(),
                    carried: HashedStack::Empty,
                },
            );
        }

        let mut region = Region::new(2, 20, false);
        region.start_clock(crate::global::GameTime(100));
        let table = BlockPos::new(1, 64, 0);
        region.chunks.set_block(table, blocks::CARTOGRAPHY_TABLE);
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(p_id, Box::new(sink.clone()));

        // 创建 map 0 (scale=0, center=(0,0))，画一个像素并挂一个展示框标记。
        let mut map0 = create_map(
            &mut region.maps,
            0,
            0,
            0,
            true,
            false,
            "minecraft:overworld",
        );
        map0.count = 4;
        {
            let d0 = region.maps.get_mut(0).unwrap();
            d0.set_color(3, 4, packed_color(map_color::STONE, Brightness::High));
            d0.set_frame_marker(BlockPos::new(0, 64, 0), 90, 77, Some(100));
        }

        // 玩家快捷栏（菜单 30..=38 对应 inv 36..=44）：
        // 36=map0×4, 37=paper×4, 38=glass_pane×2, 39=empty map×2, 40=未知 map_id=999
        {
            let inv = &mut region.players.get_mut(&p_id).unwrap().inventory.slots;
            inv[36] = map0;
            inv[37] = stack(items::PAPER, 4);
            inv[38] = stack(items::GLASS_PANE, 2);
            inv[39] = stack(items::MAP, 2);
            inv[40] = make_filled_map(999);
        }

        assert!(station::try_open(&mut region, p_id, table));
        let cid = region.menus[&p_id].container_id;

        // 1. 未知 map_id=999 + 纸：结果格必须为空。
        click(&mut region, p_id, cid, 34, 0, ContainerInput::QuickMove); // inv[40] -> slot 0
        click(&mut region, p_id, cid, 31, 0, ContainerInput::QuickMove); // paper -> slot 1
        assert!(
            region.menus[&p_id]
                .station
                .as_ref()
                .unwrap()
                .replay_view()
                .slots[2]
                .is_empty(),
            "unknown map_id must produce empty result"
        );
        // 移走未知地图，放入真实 map 0 ×4。
        click(&mut region, p_id, cid, 0, 0, ContainerInput::QuickMove);
        click(&mut region, p_id, cid, 30, 0, ContainerInput::QuickMove); // map0×4 -> slot 0

        // 2. 放大预览：slot 2 应带 MAP_POST_PROCESSING = SCALE。
        let preview = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .slots[2]
            .clone();
        assert_eq!(preview.item, items::FILLED_MAP);
        assert_eq!(preview.count, 1);
        assert_eq!(stack_map_id(&preview), Some(0));
        assert_eq!(
            resolved(&preview, kinds::MAP_POST_PROCESSING),
            Some(ComponentValue::MapPostProcessing(SCALE))
        );

        // 左键取出：去掉 MAP_POST_PROCESSING，分配新 map_id=1（scale=1, center=(64,64), colors全0）。
        click(&mut region, p_id, cid, 2, 0, ContainerInput::Pickup);
        let carried = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .carried;
        assert_eq!(stack_map_id(&carried), Some(1));
        assert!(!has_post_processing(&carried));
        let d1 = region.maps.get(1).expect("scaled map 1 created");
        assert_eq!(
            (d1.scale, d1.center_x, d1.center_z, d1.locked),
            (1, 64, 64, false)
        );
        assert!(d1.colors.iter().all(|&b| b == 0));
        // 把光标放回背包空格（菜单槽 5，槽 3 已被退回的 999 地图占住）。
        click(&mut region, p_id, cid, 5, 0, ContainerInput::Pickup);

        // 3. Shift-click 结果格：剩下 map0×3 和 paper×3，连造 3 张新图（id=2, 3, 4），
        // 且同一刻只播 1 次 ui.cartography_table.take_result 音效。
        region.links.flush();
        sink.0.lock().unwrap().clear();
        region.tick = region.tick.plus(1);
        click(&mut region, p_id, cid, 2, 0, ContainerInput::QuickMove);
        region.links.flush();
        for new_id in [2, 3, 4] {
            assert_eq!(region.maps.get(new_id).expect("shift-scaled map").scale, 1);
            let found = region.players[&p_id]
                .inventory
                .slots
                .iter()
                .find(|s| s.item == items::FILLED_MAP && stack_map_id(s) == Some(new_id));
            let s = found.expect("scaled map moved to inventory");
            assert!(!has_post_processing(s));
        }
        let raw = std::mem::take(&mut *sink.0.lock().unwrap());
        let mut rest = raw.as_slice();
        let mut sound_count = 0usize;
        while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
            let end = bounds.header_len + bounds.payload_len;
            let mut r = Reader::new(&rest[bounds.header_len..end]);
            if r.varint().unwrap() == clientbound::SOUND {
                sound_count += 1;
            }
            rest = &rest[end..];
        }
        assert_eq!(
            sound_count, 1,
            "same-tick shift-craft must play cartography sound only once"
        );

        // 4. 锁定：放回 map0（新给一份到槽 36）+ 玻璃板 → 预览 LOCK → 取出得 id=5，
        // 继承颜色、清空 frame_markers、locked=true。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(0);
        click(&mut region, p_id, cid, 30, 0, ContainerInput::QuickMove); // map0 -> slot 0
        click(&mut region, p_id, cid, 32, 0, ContainerInput::QuickMove); // glass_pane×2 -> slot 1
        let lock_preview = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .slots[2]
            .clone();
        assert_eq!(
            resolved(&lock_preview, kinds::MAP_POST_PROCESSING),
            Some(ComponentValue::MapPostProcessing(LOCK))
        );
        click(&mut region, p_id, cid, 2, 0, ContainerInput::Pickup);
        let locked_stack = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .carried;
        assert_eq!(stack_map_id(&locked_stack), Some(5));
        assert!(!has_post_processing(&locked_stack));
        let d5 = region.maps.get(5).expect("locked map 5 created");
        assert!(d5.locked);
        assert_eq!(
            d5.colors[3 + 4 * 128],
            packed_color(map_color::STONE, Brightness::High)
        );
        assert!(d5.frame_markers().is_empty());

        // 锁定的地图放进槽 0 + 玻璃板 → 不能再锁定（槽 2 为空）。
        click(&mut region, p_id, cid, 0, 0, ContainerInput::Pickup);
        assert!(
            region.menus[&p_id]
                .station
                .as_ref()
                .unwrap()
                .replay_view()
                .slots[2]
                .is_empty(),
            "locked map cannot be locked again"
        );

        // 5. 复制：锁定图 id=5 + 空地图 → 槽 2 为 2 张 id=5，不分配新 ID。
        click(&mut region, p_id, cid, 1, 0, ContainerInput::QuickMove); // 退回玻璃板
        click(&mut region, p_id, cid, 33, 0, ContainerInput::QuickMove); // empty map -> slot 1
        let copy_preview = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .slots[2]
            .clone();
        assert_eq!(copy_preview.count, 2);
        assert_eq!(stack_map_id(&copy_preview), Some(5));
        assert!(!has_post_processing(&copy_preview));
        let last_id_before = region.maps.index.last_map_id;
        click(&mut region, p_id, cid, 2, 0, ContainerInput::Pickup);
        let copied = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .carried;
        assert_eq!(copied.count, 2);
        assert_eq!(stack_map_id(&copied), Some(5));
        assert_eq!(region.maps.index.last_map_id, last_id_before);
        click(&mut region, p_id, cid, 6, 0, ContainerInput::Pickup); // 放下

        // 6. scale=4 地图不能再放大；创造模式中键克隆（safeClone）会跑后处理并分配新 ID。
        let max_scale_data =
            MapItemSavedData::create_fresh(0.0, 0.0, 4, true, false, "minecraft:overworld");
        let max_id = region.maps.next_map_id(); // 6
        region.maps.insert(max_id, max_scale_data);
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(max_id);
        region.players.get_mut(&p_id).unwrap().inventory.slots[37] = stack(items::PAPER, 1);
        click(&mut region, p_id, cid, 1, 0, ContainerInput::QuickMove); // 退回空地图
        click(&mut region, p_id, cid, 30, 0, ContainerInput::QuickMove); // scale=4 map -> slot 0
        click(&mut region, p_id, cid, 31, 0, ContainerInput::QuickMove); // paper -> slot 1
        assert!(
            region.menus[&p_id]
                .station
                .as_ref()
                .unwrap()
                .replay_view()
                .slots[2]
                .is_empty(),
            "scale 4 map must not scale further"
        );

        // 换回 map 0 + paper，切创造模式中键克隆槽 2 → 分配 id=7 且无 MAP_POST_PROCESSING。
        click(&mut region, p_id, cid, 0, 0, ContainerInput::QuickMove);
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(0);
        click(&mut region, p_id, cid, 30, 0, ContainerInput::QuickMove);
        region.players.get_mut(&p_id).unwrap().game_mode = GameMode::Creative;
        click(&mut region, p_id, cid, 2, 2, ContainerInput::Clone);
        let cloned = region.menus[&p_id]
            .station
            .as_ref()
            .unwrap()
            .replay_view()
            .carried;
        assert_eq!(stack_map_id(&cloned), Some(7));
        assert_eq!(cloned.count, 64);
        assert!(!has_post_processing(&cloned));
        assert_eq!(region.maps.get(7).unwrap().scale, 1);
    }
}
