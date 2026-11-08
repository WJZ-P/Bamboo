//! 连接任务的解码入口在 1 MB 栈上不能把进程打崩。
//!
//! tokio worker 默认栈大约 2 MB。这里用一半，留出余量。到上限的结果和官方一致
//! （物品嵌套和谓词比官方严，到我们的上限仍收下）。超过上限返回错误，这条连接断开。
//!
//! The connection-task decode entry must not abort the process on a 1 MB stack.
//!
//! A tokio worker has about 2 MB. This uses half of that, leaving a margin. At the
//! limit the result matches official (item nesting and predicates are stricter, and
//! our own limit is still accepted). Past the limit the decode returns an error and
//! that connection drops.

use mc_bamboo_data::{items, packets::play::serverbound, registries::data_component_type as kinds};
use mc_bamboo_nbt::{MAX_DEPTH, Tag};

use crate::{
    codec::{DecodeError, write_i16, write_varint},
    components::{cursor::MAX_STACK_DEPTH, nbt_wire::MAX_PREDICATE_DEPTH},
    packet::play::PlayRequest,
};

/// 1 MB，tokio 默认栈的一半。
const SMALL_STACK: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum Slot {
    /// 物品收下了。
    ///
    /// The item was accepted.
    Accepted,
    /// 这格被忽略，连接还在。
    ///
    /// The slot was ignored and the connection stays up.
    Ignored,
    /// 解码失败，连接要断。
    ///
    /// Decoding failed and the connection drops.
    Rejected,
}

/// 和连接任务同一个入口：`PlayRequest::decode` 一个 `set_creative_mode_slot`。
fn decode_on_1mb(payload: Vec<u8>) -> Slot {
    let result = std::thread::Builder::new()
        .name("creative-1mb".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(move || PlayRequest::decode(&payload))
        .expect("1 MB thread")
        .join()
        .expect("1 MB thread overflowed");
    match result {
        Ok(PlayRequest::SetCreativeModeSlot { stack, .. }) => {
            assert!(!stack.added.is_empty());
            Slot::Accepted
        }
        Ok(PlayRequest::Other(id)) => {
            assert_eq!(id, serverbound::SET_CREATIVE_MODE_SLOT);
            Slot::Ignored
        }
        Ok(other) => panic!("unexpected packet {other:?}"),
        Err(error) => {
            assert!(is_nesting(&error), "{error:?}");
            Slot::Rejected
        }
    }
}

fn is_nesting(error: &DecodeError) -> bool {
    matches!(
        error,
        DecodeError::Nbt(mc_bamboo_nbt::DecodeError::TooDeep { .. })
            | DecodeError::StacksTooDeep { .. }
    )
}

fn creative(kind: i32, value: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_varint(&mut payload, serverbound::SET_CREATIVE_MODE_SLOT);
    write_i16(&mut payload, 36);
    write_varint(&mut payload, 1);
    write_varint(&mut payload, items::STONE);
    write_varint(&mut payload, 1);
    write_varint(&mut payload, 0);
    write_varint(&mut payload, kind);
    write_varint(&mut payload, value.len() as i32);
    payload.extend_from_slice(value);
    payload
}

fn nbt_bytes(tag: Tag) -> Vec<u8> {
    let mut bytes = Vec::new();
    tag.write_network(&mut bytes);
    Tag::release(tag);
    bytes
}

/// `levels` 层复合标签，最里面是空复合标签。
fn compounds(levels: usize) -> Tag {
    let mut tag = Tag::Compound(Vec::new());
    for _ in 1..levels {
        tag = Tag::Compound(vec![("a".to_owned(), tag)]);
    }
    tag
}

/// `levels` 层列表，最里面是字符串。字符串不计深度。
fn lists(levels: usize) -> Tag {
    let mut tag = Tag::String("a".to_owned());
    for _ in 0..levels {
        tag = Tag::List(vec![tag]);
    }
    tag
}

/// `levels` 层 `extra`。每层是一个复合标签加一个列表，所以 256 层刚好到 NBT 的 512。
fn extra(levels: usize) -> Tag {
    let mut tag = Tag::String("a".to_owned());
    for _ in 0..levels {
        tag = Tag::Compound(vec![
            ("text".to_owned(), Tag::String("a".to_owned())),
            ("extra".to_owned(), Tag::List(vec![tag])),
        ]);
    }
    tag
}

/// `levels` 层 `minecraft:container` 谓词。`items` 是 `CollectionPredicate`，
/// `contains` 里的每一项再进一层。最里面是空谓词，深度就是 `levels`。
///
/// `levels` of `minecraft:container`. `items` is a `CollectionPredicate`, and each
/// `contains` entry is one level deeper. The innermost predicate is empty, so the
/// depth is `levels`.
fn predicate(levels: u32) -> Tag {
    let mut tag = Tag::Compound(Vec::new());
    for _ in 0..levels {
        tag = Tag::Compound(vec![(
            "predicates".to_owned(),
            Tag::Compound(vec![(
                "minecraft:container".to_owned(),
                Tag::Compound(vec![(
                    "items".to_owned(),
                    Tag::Compound(vec![("contains".to_owned(), Tag::List(vec![tag]))]),
                )]),
            )]),
        )]);
    }
    tag
}

/// 一个 `bundle_contents`，里面一件物品，这件物品自己再套 `levels` 层。
fn nest_bundle(levels: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, items::BUNDLE);
    write_varint(&mut bytes, 1);
    if levels == 0 {
        write_varint(&mut bytes, 0);
        write_varint(&mut bytes, 0);
        return bytes;
    }
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, kinds::BUNDLE_CONTENTS);
    bytes.extend(nest_bundle(levels - 1));
    bytes
}

/// 一个 `container`，一个槽里放一件物品，这件物品自己再套 `levels` 层。
fn nest_container(levels: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 1);
    bytes.push(1);
    write_varint(&mut bytes, items::CHEST);
    write_varint(&mut bytes, 1);
    if levels == 0 {
        write_varint(&mut bytes, 0);
        write_varint(&mut bytes, 0);
        return bytes;
    }
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, kinds::CONTAINER);
    bytes.extend(nest_container(levels - 1));
    bytes
}

#[test]
fn creative_slot_nesting_stays_on_a_1mb_stack() {
    let depth = MAX_DEPTH as usize;

    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_DATA, &nbt_bytes(compounds(depth)))),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::CUSTOM_DATA,
            &nbt_bytes(compounds(depth + 1))
        )),
        Slot::Rejected
    );

    // 根是列表时 `custom_data` 要复合标签，官方拒绝。512 层列表本身合法，所以只忽略这格。
    // 513 层在 NBT 里就越界了，这条连接要断。
    //
    // A root list is not a compound, so official `custom_data` rejects it. A
    // 512-deep list is legal NBT, and this slot is ignored. 513 is past the NBT
    // limit, so the connection drops.
    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_DATA, &nbt_bytes(lists(depth)))),
        Slot::Ignored
    );
    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_DATA, &nbt_bytes(lists(depth + 1)))),
        Slot::Rejected
    );

    // 同一串列表作为文本组件是合法的，官方收下。
    //
    // The same list is a legal text component, and official code accepts it.
    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_NAME, &nbt_bytes(lists(depth)))),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_NAME, &nbt_bytes(lists(depth + 1)))),
        Slot::Rejected
    );

    // 每层 `extra` 占两层 NBT。256 层刚好 512，257 层越界。
    //
    // Each `extra` layer is two NBT levels. 256 layers are exactly 512, and 257
    // is past the limit.
    let extra_limit = depth / 2;
    assert_eq!(
        decode_on_1mb(creative(kinds::CUSTOM_NAME, &nbt_bytes(extra(extra_limit)))),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::CUSTOM_NAME,
            &nbt_bytes(extra(extra_limit + 1))
        )),
        Slot::Rejected
    );

    // 最外层的创造物品不是模板。`nest(15)` 再往里 15 层，一共 16 层，是我们的上限。
    //
    // The outer creative item is not a template. `nest(15)` plus that item is 16
    // levels, which is our cap.
    assert_eq!(
        decode_on_1mb(creative(
            kinds::BUNDLE_CONTENTS,
            &nest_bundle(MAX_STACK_DEPTH - 1)
        )),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::BUNDLE_CONTENTS,
            &nest_bundle(MAX_STACK_DEPTH)
        )),
        Slot::Rejected
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::CONTAINER,
            &nest_container(MAX_STACK_DEPTH - 1)
        )),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(kinds::CONTAINER, &nest_container(MAX_STACK_DEPTH))),
        Slot::Rejected
    );

    assert_eq!(
        decode_on_1mb(creative(
            kinds::LOCK,
            &nbt_bytes(predicate(MAX_PREDICATE_DEPTH))
        )),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::LOCK,
            &nbt_bytes(predicate(MAX_PREDICATE_DEPTH + 1))
        )),
        Slot::Rejected
    );

    // 锁里的 `custom_data` 会整棵重排再写成 SNBT。总深度 512 仍是合法 NBT。
    //
    // A lock's `custom_data` is reordered and written as SNBT. A total depth of 512
    // is still legal NBT.
    assert_eq!(
        decode_on_1mb(creative(
            kinds::LOCK,
            &nbt_bytes(lock_custom_data(MAX_DEPTH as usize))
        )),
        Slot::Accepted
    );
    assert_eq!(
        decode_on_1mb(creative(
            kinds::LOCK,
            &nbt_bytes(lock_custom_data(MAX_DEPTH as usize + 1))
        )),
        Slot::Rejected
    );
}

/// 锁谓词，里面一份 `custom_data`。`levels` 是从根数起的复合标签层数。
///
/// A lock predicate whose body is `custom_data`. `levels` counts compound layers
/// from the root.
fn lock_custom_data(levels: usize) -> Tag {
    Tag::Compound(vec![(
        "predicates".to_owned(),
        Tag::Compound(vec![(
            "minecraft:custom_data".to_owned(),
            compounds(levels - 2),
        )]),
    )])
}
