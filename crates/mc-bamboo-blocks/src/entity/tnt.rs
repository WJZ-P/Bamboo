//! 点燃的 TNT，官方 `PrimedTnt`。
//!
//! 它也不调 `baseTick`：重力、移动、方块效果都在 `tick` 里自己做，引信归零时炸掉。
//! 实体伤害和掉落物不在范围里，爆炸只破坏方块、连锁点燃别的 TNT。

use mc_bamboo_core::Vec3;

use crate::{
    entity::{Entity, EntityKind, PrimedTnt, physics},
    world::World,
};

impl Entity {
    /// `PrimedTnt.tick()`。
    pub(crate) fn tick_tnt(&mut self, mut data: PrimedTnt, world: &mut World) {
        // `handlePortal` 不在范围里。
        self.apply_gravity();
        let movement = self.delta;
        self.move_entity(world, movement);
        self.apply_effects_from_blocks(world);
        self.delta = self.delta.scale(physics::air_drag());
        if self.on_ground {
            self.delta = self.delta.multiply(0.7, -0.5, 0.7);
        }

        data.fuse -= 1;
        if data.fuse <= 0 {
            self.discard();
            // 爆炸点是 `getY(0.0625)`，不是实体原点。
            let center = Vec3::new(self.position.x, self.y_at(0.0625), self.position.z);
            world.explode_tnt(center, 4.0);
        } else {
            // 官方接着调 `updateFluidInteraction`，它只影响水流推动和雨天灭火，
            // 两样都不在范围里，所以 `wasTouchingWater` 一直是假。
            self.kind = EntityKind::Tnt(data);
        }
        self.set_requires_precise_position(self.horizontal_collision || self.vertical_collision);
    }
}
