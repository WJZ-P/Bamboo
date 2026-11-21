use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    mob::{MobType, equine},
};
use mc_bamboo_data::registries::attribute;

use super::*;

const KINDS: [MobType; 4] = [
    MobType::Horse,
    MobType::Donkey,
    MobType::Mule,
    MobType::Llama,
];
const ATTRIBUTES: [i32; 3] = [
    attribute::MAX_HEALTH,
    attribute::JUMP_STRENGTH,
    attribute::MOVEMENT_SPEED,
];
type State = [i64; 6];

/// 逐刻场景里的类型编号：4 马、5 驴、6 骡、7 羊驼。
///
/// Tick-scene kind numbers: 4 horse, 5 donkey, 6 mule, 7 llama.
pub(super) fn scene_kind(kind: i32) -> MobType {
    KINDS[(kind - 4) as usize]
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnCase {
    seed: i64,
    kind: usize,
    egg: bool,
    state: State,
    left_handed: bool,
    yaw: i32,
    level_seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BreedCase {
    seed: i64,
    kind: usize,
    partner: usize,
    parent_seed: i64,
    after_seed: i64,
    parent_state: State,
    partner_state: State,
    child: Option<State>,
    child_kind: i32,
}

/// 一题右键交互：哪种马科、哪种初始状态、手里拿什么，以及官方交互之后的样子。
///
/// One right-click case: the kind, the starting setup, the held item, and the
/// mount's state after vanilla's interaction.
#[derive(Debug, Deserialize)]
pub(super) struct InteractCase {
    kind: usize,
    setup: i32,
    item: i32,
    creative: bool,
    sneak: bool,
    seed: i64,
    swing: String,
    count: i32,
    after: [i64; 9],
}

/// Java 一侧 `setupEquine` 的六种初始状态。
///
/// The six starting setups of the Java `setupEquine`.
fn setup_equine(entity: &mut Entity, setup: i32) {
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};
    entity.with_mob(|me, mob| {
        let kind = mob.mob_type;
        let equine = mob.equine.as_mut().unwrap();
        equine.tame = (2..=4).contains(&setup);
        if setup == 1 {
            equine.temper = if kind == MobType::Llama { 28 } else { 95 };
        }
        if setup == 3 && kind != MobType::Horse {
            equine.chested = true;
        }
        if matches!(setup, 1 | 3 | 4) {
            mob.health = 10.0;
        }
        if setup >= 4 {
            // `setAge(-24000)`：跨过 0，换成幼年尺寸。
            // `setAge(-24000)` crosses zero and switches to the baby size.
            mob.age = -24000;
            let (width, height, eye) = kind.dimensions(true);
            me.dimensions = (width, height);
            mob.eye_height = eye;
        }
        if setup == 3 {
            if kind != MobType::Llama {
                mob.equipment
                    .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
            }
            let body = match kind {
                MobType::Horse => items::IRON_HORSE_ARMOR,
                MobType::Llama => items::WHITE_CARPET,
                _ => return,
            };
            mob.equipment
                .set(EquipmentSlot::Body, ItemStack::new(body, 1));
        }
    });
}

fn after(entity: &Entity) -> [i64; 9] {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    let mob = entity.kind.mob().unwrap();
    let equine = mob.equine.as_ref().unwrap();
    let item = |slot| {
        let stack = mob.equipment.get(slot);
        if stack.is_empty() {
            0
        } else {
            i64::from(stack.item)
        }
    };
    [
        equine.flags().into(),
        equine.temper.into(),
        i64::from(mob.health.to_bits() as i32),
        mob.in_love.into(),
        mob.age.into(),
        i64::from(equine.chested),
        item(EquipmentSlot::Saddle),
        item(EquipmentSlot::Body),
        entity.random.state(),
    ]
}

fn state(entity: &Entity) -> State {
    let mob = entity.kind.mob().unwrap();
    let [health, jump, speed] = ATTRIBUTES.map(|id| mob.attributes.base(id).to_bits() as i64);
    let equine = mob.equine.as_ref().unwrap();
    [
        health,
        jump,
        speed,
        equine.variant.into(),
        equine.strength.into(),
        mob.eye_height.to_bits().into(),
    ]
}

fn make_parent(kind: MobType, fixture: State) -> Entity {
    let mut entity = Entity::by_mob_type(kind, Vec3::default());
    entity.with_mob(|_, mob| {
        for (id, bits) in ATTRIBUTES.into_iter().zip(fixture) {
            mob.attributes.instance(id).unwrap().base = f64::from_bits(bits as u64);
        }
        let equine = mob.equine.as_mut().unwrap();
        equine.variant = fixture[3] as i32;
        equine.strength = fixture[4] as i32;
    });
    entity
}

impl Report {
    pub(super) fn replay_equine_spawn(&mut self, case: SpawnCase) {
        self.cases += 1;
        let kind = KINDS[case.kind];
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(case.seed),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts::default(),
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(1),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = if case.egg {
            spawn::spawn_egg(&mut world, kind, BlockPos::new(0, 64, 0), false, None)
        } else {
            spawn::summon_mob(&mut world, kind, Vec3::new(0.5, 64.0, 0.5), None).unwrap()
        };
        let entity = world.entity(id).unwrap();
        let actual = (
            state(&entity),
            entity.kind.mob().unwrap().left_handed,
            if case.egg {
                entity.y_rot.to_bits() as i32
            } else {
                0
            },
            world.random().seed(),
        );
        let expected = (case.state, case.left_handed, case.yaw, case.level_seed);
        if actual != expected {
            self.mismatch(format!(
                "{kind:?} spawn seed {} egg {}: official {expected:?}, bamboo {actual:?}",
                case.seed, case.egg
            ));
        }
    }

    pub(super) fn replay_equine_interact(&mut self, case: InteractCase) {
        use mc_bamboo_blocks::{InteractionResult, entity::mob::interact};
        self.cases += 1;
        let mut entity = Entity::by_mob_type(KINDS[case.kind], Vec3::new(0.5, 64.0, 0.5));
        entity.random = JavaRandom::new(case.seed);
        setup_equine(&mut entity, case.setup);
        let mut uuid = [0u8; 16];
        uuid[..8].copy_from_slice(&1_u64.to_be_bytes());
        uuid[8..].copy_from_slice(&2_u64.to_be_bytes());
        let request = interact::MobInteractRequest {
            player_uid: 1,
            player_uuid: uuid,
            hand: mc_bamboo_core::Hand::Main,
            held: mc_bamboo_protocol::item::ItemStack::new(case.item, 1),
            creative: case.creative,
            position: Vec3::new(1.5, 64.0, 0.5),
            sneaking: case.sneak,
        };
        let outcome = entity
            .with_mob(|me, mob| interact::interact(me, mob, &request))
            .unwrap();
        let swing = match outcome.result {
            InteractionResult::Success => "PREDICTED",
            InteractionResult::SuccessServer => "SERVER_ONLY",
            InteractionResult::Consume => "NONE",
            _ => "PASS",
        };
        let count = if outcome.shrink_held { 0 } else { 1 };
        let actual = (swing, count, after(&entity));
        let expected = (case.swing.as_str(), case.count, case.after);
        if actual != expected {
            self.mismatch(format!(
                "equine interact kind {} setup {} item {} creative {} sneak {}: official {expected:?}, bamboo {actual:?}",
                case.kind, case.setup, case.item, case.creative, case.sneak
            ));
        }
    }

    pub(super) fn replay_equine_breed(&mut self, case: BreedCase) {
        self.cases += 1;
        let first = make_parent(KINDS[case.kind], case.parent_state);
        let second = make_parent(KINDS[case.partner], case.partner_state);
        let mut random = JavaRandom::from_state(case.parent_seed);
        let child = equine::offspring(
            &mut random,
            first.kind.mob().unwrap(),
            second.kind.mob().unwrap(),
            Vec3::default(),
        );
        let child_kind = child.as_ref().map_or(-1, |entity| {
            let kind = entity.kind.mob().unwrap().mob_type;
            KINDS.iter().position(|entry| *entry == kind).unwrap() as i32
        });
        let actual = (child.as_ref().map(state), child_kind, random.state());
        let expected = (case.child, case.child_kind, case.after_seed);
        if actual != expected {
            self.mismatch(format!(
                "equine breed seed {} parents {}/{}: official {expected:?}, bamboo {actual:?}",
                case.seed, case.kind, case.partner
            ));
        }
    }
}
