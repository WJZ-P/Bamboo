//! 贴在别的方块上的方块：火把、告示牌、悬挂告示牌、旗帜、头颅、梯子、拉杆、按钮、压力板、
//! 灯笼、末地烛、避雷针、钟、绊线钩、架子、雕纹书架。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。

mod banner;
mod bell;
mod button;
mod chiseled_bookshelf;
mod hanging_sign;
mod ladder;
mod lantern;
mod lever;
mod pressure_plate;
mod rod;
mod shelf;
mod sign;
mod skull;
mod slot_container;
mod torch;
pub mod tripwire_hook;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::BlockClass;

use self::{
    banner::{Banner, WallBanner},
    bell::Bell,
    button::Button,
    chiseled_bookshelf::ChiseledBookShelf,
    hanging_sign::{CeilingHangingSign, WallHangingSign},
    ladder::Ladder,
    lantern::Lantern,
    lever::Lever,
    pressure_plate::PressurePlate,
    rod::{EndRod, LightningRod},
    shelf::Shelf,
    sign::{StandingSign, WallSign},
    skull::{Skull, WallSkull},
    torch::WallTorch,
    tripwire_hook::TripWireHook,
};
use super::{
    Behaviour,
    common::{BaseTorch, FaceAttached},
};
use crate::{state::BlockState, world::World};

/// 这个家族里类到行为的分派。新版本多出来、还没移植的类是 `None`。
pub(super) fn of(class: BlockClass) -> Option<&'static dyn Behaviour> {
    use BlockClass as C;

    match class {
        C::BannerBlock => Some(&Banner),
        C::BellBlock => Some(&Bell),
        C::ButtonBlock => Some(&Button),
        C::CeilingHangingSignBlock => Some(&CeilingHangingSign),
        C::ChiseledBookShelfBlock => Some(&ChiseledBookShelf),
        C::EndRodBlock => Some(&EndRod),
        C::LadderBlock => Some(&Ladder),
        C::LanternBlock | C::WeatheringLanternBlock => Some(&Lantern),
        C::LeverBlock => Some(&Lever),
        C::LightningRodBlock | C::WeatheringLightningRodBlock => Some(&LightningRod),
        C::PlayerHeadBlock | C::SkullBlock | C::WitherSkullBlock => Some(&Skull),
        C::PiglinWallSkullBlock
        | C::PlayerWallHeadBlock
        | C::WallSkullBlock
        | C::WitherWallSkullBlock => Some(&WallSkull),
        C::PressurePlateBlock | C::WeightedPressurePlateBlock => Some(&PressurePlate),
        C::ShelfBlock => Some(&Shelf),
        C::StandingSignBlock => Some(&StandingSign),
        C::TorchBlock => Some(&BaseTorch),
        C::TripWireHookBlock => Some(&TripWireHook),
        C::WallBannerBlock => Some(&WallBanner),
        C::WallHangingSignBlock => Some(&WallHangingSign),
        C::WallSignBlock => Some(&WallSign),
        C::WallTorchBlock => Some(&WallTorch),
        _ => None,
    }
}

/// 拉杆和按钮的 `updateNeighbours`：通知自己周围，再通知支撑方块周围。
fn update_attached_neighbours(state: BlockState, world: &mut World, pos: BlockPos) {
    let front = FaceAttached::connected_direction(state).opposite();
    world.update_neighbors_at(pos, state.block());
    world.update_neighbors_at(pos.relative(front), state.block());
}

#[cfg(test)]
mod test_support {
    use mc_bamboo_core::{BlockPos, Direction, Vec3};

    use crate::BlockHit;

    /// 点在 `pos` 这一格 `face` 面的中间。
    pub(super) fn hit_side(pos: BlockPos, face: Direction) -> BlockHit {
        let (dx, dy, dz) = face.normal();
        BlockHit {
            pos,
            face,
            location: Vec3::new(
                f64::from(pos.x) + 0.5 + 0.5 * f64::from(dx),
                f64::from(pos.y) + 0.5 + 0.5 * f64::from(dy),
                f64::from(pos.z) + 0.5 + 0.5 * f64::from(dz),
            ),
            inside: false,
        }
    }
}
