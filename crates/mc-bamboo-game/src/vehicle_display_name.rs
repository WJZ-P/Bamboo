use mc_bamboo_blocks::{archive, entity::EntityUid};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::text::Component;

use crate::scoreboard::Scoreboard;

/// `Entity.getDisplayName` 的队伍样式、悬停信息和 UUID 插入文本。
///
/// Team formatting, hover tooltip and UUID insertion from `Entity.getDisplayName`.
pub(crate) fn styled(
    scoreboard: &Scoreboard,
    uid: EntityUid,
    entity_type: &str,
    name: Component,
) -> Component {
    let uuid = uuid::Uuid::from_u128(archive::entity_uuid(uid));
    let (most, least) = uuid.as_u64_pair();
    let tooltip = Tag::Compound(vec![
        ("action".to_owned(), Tag::String("show_entity".to_owned())),
        ("id".to_owned(), Tag::String(entity_type.to_owned())),
        ("name".to_owned(), name.tag().clone()),
        (
            "uuid".to_owned(),
            Tag::IntArray(vec![
                (most >> 32) as i32,
                most as i32,
                (least >> 32) as i32,
                least as i32,
            ]),
        ),
    ]);
    let holder = uuid.to_string();
    scoreboard
        .format_holder_name(&holder, name)
        .with_entry("hover_event", tooltip)
        .insertion(holder)
}
