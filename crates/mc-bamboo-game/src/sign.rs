//! 告示牌方块实体的数据，官方 `SignBlockEntity.getUpdateTag` 的结果。
//!
//! 区块包里的方块实体和之后的 `block_entity_data` 都用这一份字节。

use mc_bamboo_core::{
    BlockPos,
    sign::{LINES, SignContent, SignText, SignTextSlot, strip_formatting},
};
use mc_bamboo_nbt::Tag;
use tracing::warn;

use crate::{inbound::PlayerId, interaction, region::Region};

/// `SignBlockEntity.playerIsTooFarAwayToEdit` 传给 `isWithinBlockInteractionRange` 的余量。
const EDIT_BUFFER: f64 = 4.0;

/// 一面文字，官方 `SignText.CODEC` 的字段和顺序。
///
/// 离线模式不做过滤，`filtered_messages` 和 `messages` 相同，
/// 所以序列化时省略，官方 `SignText.filteredMessagesForSerialization`。
fn text_tag(text: &SignText) -> Tag {
    let messages = text
        .messages
        .iter()
        .cloned()
        .map(Tag::String)
        .collect::<Vec<_>>();
    Tag::Compound(vec![
        ("messages".to_owned(), Tag::List(messages)),
        (
            "color".to_owned(),
            Tag::String(text.color.name().to_owned()),
        ),
        ("has_glowing_text".to_owned(), Tag::Byte(text.glowing as i8)),
    ])
}

/// 告示牌的 `getUpdateTag`：两面文字和上蜡标志，没有 id、坐标这些元数据。
///
/// `allow_op_features` 只在为真时写，官方的默认值是假。
pub fn update_tag_value(content: &SignContent) -> Tag {
    Tag::Compound(vec![
        ("front_text".to_owned(), text_tag(&content.front)),
        ("back_text".to_owned(), text_tag(&content.back)),
        ("is_waxed".to_owned(), Tag::Byte(content.waxed as i8)),
    ])
}

/// [`update_tag_value`] 的网络字节。
pub fn update_tag(content: &SignContent) -> Vec<u8> {
    let mut out = Vec::new();
    update_tag_value(content).write_network(&mut out);
    out
}

/// `sign_update`：官方 `ServerGamePacketListenerImpl.handleSignUpdate` 和
/// `SignBlockEntity.updateSignText`。
///
/// 离线模式不做聊天过滤，每一行去掉格式码就是明文。只有没上蜡、而且编辑器登记的正是这名玩家时
/// 才写得进去。写完清空编辑器，并按 `markUpdated` 的结果让这一格重新广播。
pub(crate) fn on_sign_update(
    region: &mut Region,
    player: PlayerId,
    pos: BlockPos,
    lines: [String; LINES],
    slot: SignTextSlot,
) {
    let Some(uuid) = region
        .players
        .get(&player)
        .map(|player| *player.uuid.as_bytes())
    else {
        return;
    };
    // 区块一定在：世界里的区块都由模板生成。不是告示牌方块时 `sign` 返回 `None`。
    let Some(mut content) = region.chunks.sign(pos) else {
        return;
    };
    if content.waxed || content.editor != Some(uuid) {
        warn!(
            player = player.0,
            x = pos.x,
            y = pos.y,
            z = pos.z,
            "player just tried to change a non-editable sign"
        );
        return;
    }

    let text = content.text_mut(slot);
    for (line, new) in text.messages.iter_mut().zip(lines) {
        *line = strip_formatting(&new);
    }
    content.editor = None;
    // `updateSignText` 末尾总会 `sendBlockUpdated`，方块本身没变，但要重发方块实体数据。
    if region.chunks.set_sign(pos, content) {
        region.block_changes.changed.push(pos);
    }
}

/// 每 tick 清掉走远的编辑者，官方 `SignBlockEntity.tick` 和 `playerIsTooFarAwayToEdit`。
///
/// 官方的 `setAllowedPlayerEditor(null)` 不经过 `markUpdated`，所以清掉的编辑器不用发包。
pub(crate) fn tick_editors(region: &mut Region) {
    let mut stale = Vec::new();
    for (pos, editor) in region.chunks.sign_editors() {
        let in_range = region.players.values().any(|player| {
            *player.uuid.as_bytes() == editor
                && interaction::within_block_interaction_range(player, pos, EDIT_BUFFER)
        });
        if !in_range {
            stale.push(pos);
        }
    }
    for pos in stale {
        let Some(mut content) = region.chunks.sign(pos) else {
            continue;
        };
        content.editor = None;
        region.chunks.set_sign(pos, content);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{Direction, Hand, sign::DyeColor};
    use mc_bamboo_data::{
        blocks, items, packets::play::clientbound, registries::block_entity_type,
    };
    use mc_bamboo_protocol::{
        item::ItemStack,
        packet::{
            ClientboundPacket,
            play::{BlockEntityData, BlockUpdate, OpenSignEditor, PlayRequest},
        },
    };

    use super::*;
    use crate::{
        links::encode_frames,
        world::tests::{Frame, Harness, body_of, frames_with_id},
    };

    /// 空白的一面，`SignText.CODEC` 的默认值。
    const BLANK_SIDE: &str = "\
        09 00 08 6d 65 73 73 61 67 65 73 08 00 00 00 04 00 00 00 00 00 00 00 00 \
        08 00 05 63 6f 6c 6f 72 00 05 62 6c 61 63 6b \
        01 00 10 68 61 73 5f 67 6c 6f 77 69 6e 67 5f 74 65 78 74 00 00";

    /// 把 `"0a 00"` 这样的十六进制文本转成字节。
    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("十六进制"))
            .collect()
    }

    #[test]
    fn a_blank_sign_has_both_sides_and_the_waxed_flag() {
        // 网络格式的根节点没有名字：根的类型之后直接是第一个条目的类型。
        let expected = hex(&format!(
            "0a 0a 00 0a 66 72 6f 6e 74 5f 74 65 78 74 {BLANK_SIDE} \
             0a 00 09 62 61 63 6b 5f 74 65 78 74 {BLANK_SIDE} \
             01 00 08 69 73 5f 77 61 78 65 64 00 00"
        ));
        assert_eq!(update_tag(&SignContent::BLANK), expected);
    }

    #[test]
    fn lines_colour_glow_and_wax_all_reach_the_tag() {
        let mut content = SignContent::BLANK;
        content.front.messages[0] = "hi".to_owned();
        content.front.color = DyeColor::Red;
        content.front.glowing = true;
        content.waxed = true;

        let expected = hex(&format!(
            "0a 0a 00 0a 66 72 6f 6e 74 5f 74 65 78 74 \
             09 00 08 6d 65 73 73 61 67 65 73 08 00 00 00 04 \
             00 02 68 69 00 00 00 00 00 00 \
             08 00 05 63 6f 6c 6f 72 00 03 72 65 64 \
             01 00 10 68 61 73 5f 67 6c 6f 77 69 6e 67 5f 74 65 78 74 01 00 \
             0a 00 09 62 61 63 6b 5f 74 65 78 74 {BLANK_SIDE} \
             01 00 08 69 73 5f 77 61 78 65 64 01 00"
        ));
        assert_eq!(update_tag(&content), expected);
    }

    #[test]
    fn the_two_sides_keep_their_own_text() {
        let mut content = SignContent::BLANK;
        content.front.messages[0] = "front".to_owned();
        content.back.messages[0] = "back".to_owned();

        let bytes = update_tag(&content);
        // 先出现正面的那一行，背面在 `back_text` 后面。
        let front = find(&bytes, &[0x00, 0x05, b'f', b'r', b'o', b'n', b't']);
        let back = find(&bytes, &[0x00, 0x04, b'b', b'a', b'c', b'k']);
        let back_name = find(
            &bytes,
            &[
                0x00, 0x09, b'b', b'a', b'c', b'k', b'_', b't', b'e', b'x', b't',
            ],
        );
        assert!(front < back_name);
        assert!(back_name < back);
    }

    /// 第一次出现的位置。
    fn find(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|part| part == needle)
            .expect("字节片段存在")
    }

    /// 一个包在假客户端那边的样子，和 `Client::take_frames` 拆出来的一样。
    fn frame<P: ClientboundPacket>(packet: &P) -> Frame {
        Frame {
            id: P::ID,
            body: body_of(encode_frames(packet)),
        }
    }

    /// 两名玩家都站在出生点，告示牌放在出生点旁边的空地上。
    fn two_players_at_a_sign() -> (
        Harness,
        crate::world::tests::Client,
        crate::world::tests::Client,
    ) {
        let mut server = Harness::new(8);
        let first = server.join_and_settle("Alice");
        let second = server.join_and_settle("Bob");
        first.take_frames();
        second.take_frames();
        (server, first, second)
    }

    const SIGN: BlockPos = BlockPos::new(3, -60, 8);

    #[test]
    fn written_text_reaches_everyone_as_block_entity_data() {
        let (mut server, writer, watcher) = two_players_at_a_sign();
        let editor = *writer.uuid.as_bytes();
        server.region().chunks.set_block(SIGN, blocks::OAK_SIGN);
        let mut content = SignContent::BLANK;
        content.editor = Some(editor);
        assert!(server.region().chunks.set_sign(SIGN, content));

        server.request(
            &writer,
            PlayRequest::SignUpdate {
                pos: SIGN,
                lines: [
                    "§aHello".to_owned(),
                    String::new(),
                    "§l§r".to_owned(),
                    String::new(),
                ],
                slot: SignTextSlot::Front,
            },
        );
        server.tick();

        // 离线模式没有聊天过滤，格式码去掉以后就是明文。
        let content = server.region().chunks.sign(SIGN).expect("告示牌");
        assert_eq!(content.front.messages[0], "Hello");
        assert_eq!(content.front.messages[2], "");
        assert_eq!(content.editor, None);

        // 方块本身没变也要重发，后面跟着 `getUpdateTag` 的内容。
        let expected = [
            frame(&BlockUpdate {
                pos: SIGN,
                state: blocks::OAK_SIGN as i32,
            }),
            frame(&BlockEntityData {
                pos: SIGN,
                kind: block_entity_type::SIGN,
                data: &update_tag(&content),
            }),
        ];
        for client in [&writer, &watcher] {
            assert_eq!(client.take_frames(), expected);
        }
    }

    #[test]
    fn only_the_registered_editor_can_write() {
        let (mut server, writer, stranger) = two_players_at_a_sign();
        server.region().chunks.set_block(SIGN, blocks::OAK_SIGN);

        // 编辑器登记的是另一名玩家。
        let mut content = SignContent::BLANK;
        content.editor = Some(*stranger.uuid.as_bytes());
        assert!(server.region().chunks.set_sign(SIGN, content.clone()));

        let lines = || std::array::from_fn(|_| "hi".to_owned());
        server.request(
            &writer,
            PlayRequest::SignUpdate {
                pos: SIGN,
                lines: lines(),
                slot: SignTextSlot::Back,
            },
        );
        server.tick();

        assert_eq!(server.region().chunks.sign(SIGN), Some(content));
        assert!(writer.take_frames().is_empty());
        assert!(stranger.take_frames().is_empty());

        // 上了蜡的告示牌连登记的编辑者也写不进去。
        let mut waxed = SignContent::BLANK;
        waxed.waxed = true;
        waxed.editor = Some(*writer.uuid.as_bytes());
        assert!(server.region().chunks.set_sign(SIGN, waxed.clone()));
        server.request(
            &writer,
            PlayRequest::SignUpdate {
                pos: SIGN,
                lines: lines(),
                slot: SignTextSlot::Front,
            },
        );
        server.tick();

        assert_eq!(server.region().chunks.sign(SIGN), Some(waxed));
        assert!(writer.take_frames().is_empty());
    }

    #[test]
    fn the_editor_is_cleared_when_its_player_leaves_the_range() {
        let mut server = Harness::new(8);
        let client = server.join_and_settle("Kitten");
        client.take_frames();

        server.region().chunks.set_block(SIGN, blocks::OAK_SIGN);
        let mut content = SignContent::BLANK;
        content.editor = Some(*client.uuid.as_bytes());
        assert!(server.region().chunks.set_sign(SIGN, content));

        // 出生点旁边够得着，编辑器留着。
        server.tick();
        assert!(
            server
                .region()
                .chunks
                .sign(SIGN)
                .expect("告示牌")
                .editor
                .is_some()
        );

        server.move_to(&client, 100.0, 100.0);
        server.tick();
        assert_eq!(
            server.region().chunks.sign(SIGN).expect("告示牌").editor,
            None
        );
    }

    #[test]
    fn placing_a_sign_opens_the_editor_and_registers_its_placer() {
        let mut server = Harness::new(8);
        let client = server.join_and_settle("Kitten");
        client.take_frames();

        let ground = BlockPos::new(11, -61, 8);
        let pos = ground.above();
        server.request(
            &client,
            PlayRequest::SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::new(items::OAK_SIGN, 1),
            },
        );
        server.request(
            &client,
            PlayRequest::UseItemOn {
                hand: Hand::Main,
                pos: ground,
                face: Direction::Up,
                cursor: (0.5, 1.0, 0.5),
                inside: false,
                world_border_hit: false,
                sequence: 1,
            },
        );
        server.tick();

        let frames = client.take_frames();
        let expected = frame(&OpenSignEditor {
            pos,
            slot: SignTextSlot::Front,
        });
        assert_eq!(
            frames_with_id(&frames, clientbound::OPEN_SIGN_EDITOR),
            [&expected]
        );
        assert_eq!(server.region().chunks.block(pos), Some(blocks::OAK_SIGN));
        assert_eq!(
            server.region().chunks.sign(pos).expect("告示牌").editor,
            Some(*client.uuid.as_bytes())
        );

        // 编辑器登记的正是放置者，随后发来的文字写得进去。
        server.request(
            &client,
            PlayRequest::SignUpdate {
                pos,
                lines: [
                    "done".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                slot: SignTextSlot::Front,
            },
        );
        server.tick();
        assert_eq!(
            server
                .region()
                .chunks
                .sign(pos)
                .expect("告示牌")
                .front
                .messages[0],
            "done"
        );
    }
}
