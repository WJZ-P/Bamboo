//! 机器和容器：活塞、发射器、投掷器、侦测器、中继器、比较器、阳光探测器、漏斗、铁轨、红石线、
//! 红石火把、熔炉系列、箱子系列、木桶、末影箱、潜影盒、讲台、铁砧、砂轮、切石机、织布机、营火、
//! 蜂巢、南瓜灯、绊线、音符盒、命令方块系列。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。
//!
//! 红石信号和元件的计划刻都移植了。要方块实体内容或者实体才走得到的分支（发射物品、
//! 探测铁轨看矿车这些）不写，活塞推方块是下一步。要打开界面的方块只吃掉点击。

mod beehive;
mod campfire;
pub(crate) mod chest;
mod command_block;
mod containers;
mod daylight_detector;
mod diode;
mod lectern;
mod note_block;
mod observer;
mod piston;
mod pumpkin;
mod rail;
mod redstone_torch;
mod redstone_wire;
mod tripwire;
mod workstations;

use mc_bamboo_data::{Block, BlockClass};

use self::{
    beehive::Beehive,
    campfire::Campfire,
    chest::{Chest, CopperChest, EnderChest},
    command_block::Command,
    containers::{Barrel, Dispenser, Furnace, Hopper, ShulkerBox},
    daylight_detector::DaylightDetector,
    diode::{Comparator, Repeater},
    lectern::Lectern,
    note_block::Note,
    observer::Observer,
    piston::{MovingPiston, PistonBase, PistonHead},
    pumpkin::CarvedPumpkin,
    rail::Rail,
    redstone_torch::{RedstoneTorch, RedstoneWallTorch},
    redstone_wire::RedstoneWire,
    tripwire::TripWire,
    workstations::{Anvil, Grindstone, Workstation},
};
use super::{Behaviour, common::default_state};
use crate::{
    context::PlaceContext,
    state::{BlockState, FACING},
};

/// 这个家族里类到行为的分派。新版本多出来、还没移植的类是 `None`。
pub(super) fn of(class: BlockClass) -> Option<&'static dyn Behaviour> {
    use BlockClass as C;

    match class {
        C::AnvilBlock => Some(&Anvil),
        C::BarrelBlock => Some(&Barrel),
        C::BeehiveBlock => Some(&Beehive),
        C::BlastFurnaceBlock | C::FurnaceBlock | C::SmokerBlock => Some(&Furnace),
        C::CampfireBlock => Some(&Campfire),
        C::CarvedPumpkinBlock => Some(&CarvedPumpkin),
        C::ChestBlock | C::TrappedChestBlock => Some(&Chest),
        C::CommandBlock => Some(&Command),
        C::ComparatorBlock => Some(&Comparator),
        C::CopperChestBlock | C::WeatheringCopperChestBlock => Some(&CopperChest),
        C::DaylightDetectorBlock => Some(&DaylightDetector),
        C::DetectorRailBlock | C::PoweredRailBlock | C::RailBlock => Some(&Rail),
        C::DispenserBlock | C::DropperBlock => Some(&Dispenser),
        C::EnderChestBlock => Some(&EnderChest),
        C::GrindstoneBlock => Some(&Grindstone),
        C::HopperBlock => Some(&Hopper),
        C::LecternBlock => Some(&Lectern),
        C::LoomBlock | C::StonecutterBlock => Some(&Workstation),
        C::MovingPistonBlock => Some(&MovingPiston),
        C::NoteBlock => Some(&Note),
        C::ObserverBlock => Some(&Observer),
        C::PistonBaseBlock => Some(&PistonBase),
        C::PistonHeadBlock => Some(&PistonHead),
        C::RedstoneTorchBlock => Some(&RedstoneTorch),
        C::RedstoneWallTorchBlock => Some(&RedstoneWallTorch),
        C::RedstoneWireBlock => Some(&RedstoneWire),
        C::RepeaterBlock => Some(&Repeater),
        C::ShulkerBoxBlock => Some(&ShulkerBox),
        C::TripWireBlock => Some(&TripWire),
        _ => None,
    }
}

/// 三维的朝向：正对着玩家的视线，上下也算。
fn facing_the_view(block: &'static Block, ctx: &PlaceContext) -> BlockState {
    default_state(block).with(FACING, ctx.nearest_looking_direction().opposite())
}
