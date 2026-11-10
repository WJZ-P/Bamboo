//! 点燃的 TNT，官方 `PrimedTnt`。
//!
//! 它也不调 `baseTick`：重力、移动、方块效果都在 `tick` 里自己做，引信归零时炸掉。
//! 实体伤害和掉落物不在范围里，爆炸只破坏方块、连锁点燃别的 TNT。

use mc_bamboo_core::Vec3;

use crate::entity::{
    Entity, EntityKind, PrimedTnt,
    ctx::{Command, EntityCtx},
    physics,
};

/// `PrimedTnt.tick()`。爆炸整段是一条意图命令，处理函数里有完整的 `&mut World`，
/// 所以射线、洗牌、连锁引信和掉落物的 `level.random` 取数顺序和精确模式一致。
pub(crate) fn tick_tnt(me: &mut Entity, mut data: PrimedTnt, ctx: &mut dyn EntityCtx) {
    // `handlePortal` 不在范围里。
    me.apply_gravity();
    let movement = me.delta;
    me.move_entity(ctx.blocks(), movement);
    me.apply_effects_from_blocks(ctx.blocks());
    me.delta = me.delta.scale(physics::air_drag());
    if me.on_ground {
        me.delta = me.delta.multiply(0.7, -0.5, 0.7);
    }

    data.fuse -= 1;
    // 归零的那一拍也要把引信写回去：实体这一拍还在，同步和比对都看得到 0。
    me.kind = EntityKind::Tnt(data);
    // 官方接着调 `updateFluidInteraction`，它只影响水流推动和雨天灭火，
    // 两样都不在范围里，所以 `wasTouchingWater` 一直是假。
    if data.fuse <= 0 {
        me.discard();
        // 爆炸点是 `getY(0.0625)`，不是实体原点。
        let center = Vec3::new(me.position.x, me.y_at(0.0625), me.position.z);
        ctx.emit(
            Command::Explode {
                center,
                radius: 4.0,
            },
            me,
        );
    }
    me.set_requires_precise_position(me.horizontal_collision || me.vertical_collision);
}
