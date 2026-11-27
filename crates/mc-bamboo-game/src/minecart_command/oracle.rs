use mc_bamboo_blocks::{
    BlockState, World,
    archive::{read_entity, write_entity},
    entity::{Entity, EntityKind, minecart::MinecartKind},
    state::{POWERED, RAIL_SHAPE, RailShape},
};
use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{blocks, gamerules, packets::play::clientbound};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    codec::decode_varint,
    components::cursor::Cursor,
    frame::{MAX_FRAME_LEN, find_frame},
    packet::play::{ClientInformation, GameMode},
};

use crate::{
    boat_facts::Capture,
    gamerules::Stored,
    global::{GameTime, Global},
    global_port::{GlobalEffect, GlobalPort},
    inbound::PlayerId,
    player::Player,
    region::Region,
};

/// 命令矿车对拍的一刻。 / One command cart comparison step.
pub struct CommandMinecartSample {
    /// 官方命令状态字段。 / Saved command fields.
    pub nbt: Tag,
    /// 经探测铁轨读取的模拟信号。 / Detector rail's analogue signal.
    pub signal: i32,
    /// 本刻发布的消息和发送者组件。 / Chat message and sender components broadcast this tick.
    pub chat: Vec<(Tag, Tag)>,
}

/// 用游戏层的实体阶段、命令分发、比较器和编码逐刻重放命令矿车。
///
/// Replay a command cart through the game entity phase, dispatcher, comparator and encoder.
pub fn replay_command_minecart(command: &str) -> Result<Vec<CommandMinecartSample>, String> {
    let mut region = Region::new(2, 20, false);
    let mut global = Global::default();
    global
        .rules_mut()
        .set_saved(gamerules::COMMAND_BLOCKS_WORK, Stored::Bool(true));
    global.publish(&mut region);
    let id = PlayerId(1);
    let mut player = Player::new(
        id,
        uuid::Uuid::from_u128(1),
        "Witness".to_owned(),
        GameMode::Creative,
        ClientInformation::default(),
        Vec3::new(8.5, 64.0, 8.5),
    );
    player.permission_level = mc_bamboo_command::permission::GAMEMASTERS;
    region.insert_player(player);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    for z in -1..=4 {
        let at = BlockPos::new(0, 64, z);
        let mut state = BlockState(match z {
            1 => blocks::ACTIVATOR_RAIL,
            2 => blocks::DETECTOR_RAIL,
            _ => blocks::RAIL,
        })
        .with(RAIL_SHAPE, RailShape::NorthSouth);
        if z == 1 {
            state = state.with(POWERED, true);
        }
        region.chunks.set_block(at, state.0);
        region.chunks.set_block(at.below(), blocks::STONE);
    }
    let position = Vec3::new(0.5, 64.0625, 0.5);
    let mut cart = Entity::minecart(MinecartKind::CommandBlock, position);
    cart.delta.z = 0.3;
    if let EntityKind::Minecart(minecart) = &mut cart.kind {
        minecart.pitch = 490.0;
    }
    let mut initial = write_entity(&cart.split()).map_err(|error| error.to_string())?;
    if let Tag::Compound(fields) = &mut initial {
        fields.push(("LastOutput".to_owned(), Tag::String("previous".to_owned())));
    }
    let row = read_entity(&initial, 0)
        .map_err(|error| error.to_string())?
        .ok_or("command cart archive did not load")?;
    let uid = region.entities.spawn(row.into_entity());
    super::on_set(&mut region, id, uid, command.to_owned(), true);
    region.links.flush();
    capture.take();

    let mut samples = Vec::new();
    for tick in 0..=8 {
        region.start_clock(GameTime(tick));
        if tick > 0 {
            for activation in crate::interaction::run_entity_ticks(0, &mut region, None) {
                GlobalPort::Direct(&mut global)
                    .submit(&mut region, GlobalEffect::RunMinecartCommand(activation));
            }
        }
        let cart = region.entities.get(uid).ok_or("command cart disappeared")?;
        let nbt = write_entity(&cart.split()).map_err(|error| error.to_string())?;
        let detector = BlockPos::new(0, 64, 2);
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        let signal =
            world
                .block_state(detector)
                .analog_output_signal(&world, detector, Direction::East);
        region.links.flush();
        samples.push(CommandMinecartSample {
            nbt,
            signal,
            chat: read_chat(&capture.take())?,
        });
    }
    Ok(samples)
}

fn read_chat(mut rest: &[u8]) -> Result<Vec<(Tag, Tag)>, String> {
    let mut chat = Vec::new();
    while let Some(frame) = find_frame(rest, MAX_FRAME_LEN).map_err(|error| format!("{error:?}"))? {
        let end = frame.header_len + frame.payload_len;
        let payload = &rest[frame.header_len..end];
        let (id, used) = decode_varint(payload).map_err(|error| format!("{error:?}"))?;
        if id == clientbound::DISGUISED_CHAT {
            let mut reader = Cursor::new(&payload[used..]);
            let message = reader.nbt_tag().map_err(|error| format!("{error:?}"))?;
            let channel = reader.varint().map_err(|error| format!("{error:?}"))?;
            if channel != mc_bamboo_data::registries::chat_type::SAY_COMMAND + 1 {
                return Err(format!("unexpected chat type {channel}"));
            }
            let name = reader.nbt_tag().map_err(|error| format!("{error:?}"))?;
            chat.push((message, name));
        }
        rest = &rest[end..];
    }
    Ok(chat)
}
