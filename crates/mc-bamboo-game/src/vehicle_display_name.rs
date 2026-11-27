use mc_bamboo_blocks::{
    archive,
    entity::{EntityPublic, EntityUid, KindPublic},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::text::Component;

use crate::scoreboard::Scoreboard;

pub(crate) fn of(scoreboard: &Scoreboard, public: &EntityPublic) -> Option<Component> {
    let name = match &public.kind {
        KindPublic::Boat(boat) => boat.custom_name.as_ref(),
        KindPublic::Minecart(cart) => cart.custom_name.as_ref(),
        _ => return None,
    };
    let entity_type = mc_bamboo_data::registries::entity_type::name_of(public.kind.entity_type())?;
    let name = name.map_or_else(
        || Component::translate(format!("entity.{}", entity_type.replace(':', ".")), []),
        |tag| Component::trusted(without_click_actions(tag.clone())),
    );
    Some(styled(scoreboard, public.uid, entity_type, name))
}

fn without_click_actions(mut component: Tag) -> Tag {
    if let Tag::Compound(fields) = &mut component {
        fields.retain(|(key, _)| key != "click_event");
        if let Some((_, Tag::List(children))) = fields.iter_mut().find(|(name, _)| name == "extra")
        {
            for child in children {
                *child =
                    without_click_actions(std::mem::replace(child, Tag::String(String::new())));
            }
        }
        if let [(key, Tag::String(text))] = fields.as_mut_slice()
            && key == "text"
        {
            component = Tag::String(std::mem::take(text));
        }
    }
    component
}

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
