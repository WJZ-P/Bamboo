//! 载具和乘客写进区块实体文件的规则：每个实体在盘上只能有一份。
//!
//! 官方 `Entity.shouldBeSaved`：乘客不单独存（嵌在根载具的 `Passengers` 里）；
//! 整棵树里恰好有一名玩家的根载具也不存，它跟着玩家存档的 `RootVehicle` 走。
//!
//! How vehicles and passengers reach the chunk entity file: every entity exists on
//! disk once.
//!
//! Vanilla `Entity.shouldBeSaved`: a passenger is not saved on its own (it nests under
//! the root's `Passengers`), and a root whose tree holds exactly one player is not
//! saved either; it travels in the player file's `RootVehicle`.

use mc_bamboo_blocks::entity::{Entity, EntityKind, EntityRow, FIRST_ENTITY_ID, mob::MobType};
use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_data::registries::entity_type;

use super::{chunk_content, column::RegionColumn};

const CHUNK: ChunkPos = ChunkPos::new(0, 0);
const PLAYER: i32 = 1;

fn at() -> Vec3 {
    Vec3::new(4.5, 63.0, 4.5)
}

fn passengers_mut(entity: &mut Entity) -> &mut Vec<i32> {
    match &mut entity.kind {
        EntityKind::Boat(boat) => &mut boat.passengers,
        EntityKind::Minecart(cart) => &mut cart.passengers,
        EntityKind::Mob(mob) => &mut mob.passengers,
        _ => panic!("not a vehicle"),
    }
}

/// 让 `rider` 骑上 `vehicle`，两边的关系都建好。
///
/// Seat `rider` on `vehicle` with both sides of the link set.
fn seat(vehicle: &mut Entity, rider: &mut Entity) {
    rider.vehicle = Some(vehicle.id);
    passengers_mut(vehicle).push(rider.id);
}

fn mob(kind: MobType, id: i32) -> Entity {
    let mut entity = Entity::by_mob_type(kind, at());
    entity.id = id;
    entity
}

fn boat(id: i32) -> Entity {
    let mut entity = Entity::boat(entity_type::OAK_BOAT, at());
    entity.id = id;
    entity
}

/// 存一次再读回来，返回读到的实体行。
///
/// Save once, read back, and return the rows.
fn round_trip(entities: Vec<Entity>) -> Vec<EntityRow> {
    let mut column = RegionColumn::from_template();
    column.add_generated_entities(entities.into_iter().map(Entity::split).collect());
    let encoded = chunk_content::encode(&column, CHUNK).expect("encode");
    let decoded =
        chunk_content::decode(CHUNK, &encoded.chunk, encoded.entities.as_ref()).expect("decode");
    decoded.column.parts().entities.rows().to_vec()
}

fn count(rows: &[EntityRow], name: &str) -> usize {
    rows.iter()
        .filter(|row| entity_type::name_of(row.public.kind.entity_type()) == Some(name))
        .count()
}

/// 船里坐着一头牛：牛只嵌在船的 `Passengers` 里写一次，读回来还是一条船一头牛。
///
/// A cow in a boat is written once, nested under the boat, and comes back as one boat
/// and one cow.
#[test]
fn a_boat_passenger_is_saved_once() {
    let mut vehicle = boat(FIRST_ENTITY_ID);
    let mut cow = mob(MobType::Cow, FIRST_ENTITY_ID + 1);
    seat(&mut vehicle, &mut cow);

    let rows = round_trip(vec![vehicle, cow]);

    assert_eq!(count(&rows, "minecraft:cow"), 1);
    assert_eq!(count(&rows, "minecraft:oak_boat"), 1);
    let cow = rows
        .iter()
        .find(|row| row.public.vehicle.is_some())
        .expect("牛还坐在船里");
    assert!(
        rows.iter()
            .any(|row| Some(row.public.uid) == cow.public.vehicle)
    );
}

/// 矿车里的生物同样嵌在矿车里存，读回来还坐在车里。
///
/// A mob in a minecart nests under the cart too and is still seated after a reload.
#[test]
fn a_minecart_passenger_stays_seated() {
    let mut cart = Entity::minecart(
        mc_bamboo_blocks::entity::minecart::MinecartKind::Rideable,
        at(),
    );
    cart.id = FIRST_ENTITY_ID;
    let mut cow = mob(MobType::Cow, FIRST_ENTITY_ID + 1);
    seat(&mut cart, &mut cow);

    let rows = round_trip(vec![cart, cow]);

    assert_eq!(rows.len(), 2);
    let cart = rows
        .iter()
        .find(|row| row.public.vehicle.is_none())
        .expect("矿车是根");
    let cow = rows
        .iter()
        .find(|row| row.public.vehicle.is_some())
        .expect("牛还坐在矿车里");
    assert_eq!(cow.public.vehicle, Some(cart.public.uid));
}

/// 一名玩家骑着的驴不进区块文件：它已经写在玩家存档的 `RootVehicle` 里，
/// 区块里再存一份，下次进服就是两头驴、两份箱子。
///
/// A donkey ridden by one player stays out of the chunk file. It is already in the
/// player file's `RootVehicle`; a second copy in the chunk would come back as two
/// donkeys with two chests.
#[test]
fn a_vehicle_ridden_by_one_player_is_left_to_the_player_file() {
    let mut donkey = mob(MobType::Donkey, FIRST_ENTITY_ID);
    passengers_mut(&mut donkey).push(PLAYER);
    let bystander = mob(MobType::Cow, FIRST_ENTITY_ID + 1);

    let rows = round_trip(vec![donkey, bystander]);

    assert_eq!(count(&rows, "minecraft:donkey"), 0);
    assert_eq!(count(&rows, "minecraft:cow"), 1);
}

/// 玩家划着的船连同船上的其它乘客都跟玩家存档走。
///
/// A boat rowed by a player, and its other passengers, travel with the player file.
#[test]
fn the_other_passengers_of_a_ridden_boat_are_not_saved_either() {
    let mut vehicle = boat(FIRST_ENTITY_ID);
    passengers_mut(&mut vehicle).push(PLAYER);
    let mut cow = mob(MobType::Cow, FIRST_ENTITY_ID + 1);
    seat(&mut vehicle, &mut cow);

    let rows = round_trip(vec![vehicle, cow]);

    assert!(rows.is_empty(), "整棵树都在 RootVehicle 里：{}", rows.len());
}

/// 船上有两名玩家时 `hasExactlyOnePlayerPassenger` 为假：谁的存档都不带这条船，
/// 区块必须存它。
///
/// With two players aboard `hasExactlyOnePlayerPassenger` is false: neither player
/// file carries the boat, so the chunk must save it.
#[test]
fn a_boat_with_two_players_is_saved_with_the_chunk() {
    let mut vehicle = boat(FIRST_ENTITY_ID);
    passengers_mut(&mut vehicle).extend([PLAYER, PLAYER + 1]);

    let rows = round_trip(vec![vehicle]);

    assert_eq!(count(&rows, "minecraft:oak_boat"), 1);
}

/// 存盘再读盘，村民、驴、运输船和运输矿车都要回到实体表里，箱子里的东西还在。
/// 读盘以前只认一张写死的名单，名单外的实体读回来只是一段原始数据，世界里看不见。
///
/// After a save and a load, a villager, a donkey, a chest boat and a chest minecart
/// are live entities again and the chests keep their contents. Loading used to accept
/// a fixed allowlist only; anything else came back as raw data, absent from the world.
#[test]
fn saved_entities_come_back_alive() {
    use mc_bamboo_blocks::entity::{KindPublic, minecart::MinecartKind};
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::item::ItemStack;

    let mut chest_boat = Entity::boat(entity_type::OAK_CHEST_BOAT, at());
    chest_boat.id = FIRST_ENTITY_ID;
    if let EntityKind::Boat(state) = &mut chest_boat.kind {
        state.chest.as_mut().expect("运输船有箱子").items[3] = ItemStack::new(items::DIAMOND, 7);
    }
    let mut chest_cart = Entity::minecart(MinecartKind::Chest, at());
    chest_cart.id = FIRST_ENTITY_ID + 1;
    if let EntityKind::Minecart(state) = &mut chest_cart.kind {
        state.items[5] = ItemStack::new(items::EMERALD, 9);
    }
    let villager = mob(MobType::Villager, FIRST_ENTITY_ID + 2);
    let donkey = mob(MobType::Donkey, FIRST_ENTITY_ID + 3);

    let rows = round_trip(vec![chest_boat, chest_cart, villager, donkey]);

    assert_eq!(count(&rows, "minecraft:villager"), 1);
    assert_eq!(count(&rows, "minecraft:donkey"), 1);
    let boat_items = rows
        .iter()
        .find_map(|row| match &row.clone().into_entity().kind {
            EntityKind::Boat(state) => state.chest.as_ref().map(|chest| chest.items.clone()),
            _ => None,
        });
    assert_eq!(
        boat_items.expect("运输船回来了")[3],
        ItemStack::new(items::DIAMOND, 7)
    );
    let cart_items = rows
        .iter()
        .find_map(|row| match &row.clone().into_entity().kind {
            EntityKind::Minecart(state) => Some(state.items.clone()),
            _ => None,
        });
    assert_eq!(
        cart_items.expect("运输矿车回来了")[5],
        ItemStack::new(items::EMERALD, 9)
    );
    assert!(
        rows.iter()
            .all(|row| !matches!(row.public.kind, KindPublic::Item { .. })),
        "没有多出来的掉落物"
    );
}

/// 已经标记移除、还没从表里清掉的实体不写进存档（官方 `shouldBeSaved`）：被打掉的
/// 运输船已经把东西掉在地上了，再存一份船就是两份。
///
/// An entity marked removed but not yet purged is not saved (vanilla `shouldBeSaved`):
/// a destroyed chest boat already dropped its contents, so saving the boat as well
/// would double them.
#[test]
fn a_removed_entity_is_not_saved() {
    let mut cow = mob(MobType::Cow, FIRST_ENTITY_ID);
    cow.discard();

    let rows = round_trip(vec![cow]);

    assert!(rows.is_empty());
}
