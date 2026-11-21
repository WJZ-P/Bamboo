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
