//! `mc-bamboo-vanilla seed-world <目录> [半径]`：给对拍脚本造一份「盘上已经有
//! 官方格式区块、其它状态全新」的世界目录。
//!
//! 只写 `region/r.*.*.mca`，每格区块都是和 Bamboo 超平坦模板一致的 NBT
//! （`Status = minecraft:full`），不写 `level.dat` / `data/` / `players/`。
//! 服务端启动时 `SaveSession` 会按全新世界补齐那些文件，而 `ChunkIo` 的启动扫描
//! 会认出这些区块，进服时它们全部走异步读。半径缺省 8，覆盖视距 8 的跟踪范围。
//!
//! Seed a world directory with pristine on-disk chunks in the official Anvil
//! format for the `--loaded` probe mode. Only region files are written.

use std::path::Path;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionFile},
    chunk_nbt::{ChunkEncodeExtra, encode_bytes},
    world_dir::WorldDir,
};
use mc_bamboo_world::column::ChunkColumn;

/// 半径按切比雪夫距离算：视距 8 的跟踪范围最远到 ±9。
const DEFAULT_RADIUS: i32 = 8;

pub fn run(dir: &str, radius: Option<&str>) -> Result<(), String> {
    let radius: i32 = match radius {
        Some(text) => text
            .parse::<i32>()
            .map_err(|_| format!("bad radius {text:?}"))?
            .clamp(0, 64),
        None => DEFAULT_RADIUS,
    };
    let root = Path::new(dir);
    let world = WorldDir::new(root);
    world
        .create_layout()
        .map_err(|error| format!("cannot create world layout: {error}"))?;
    let region_dir = world.region_dir();
    let template = ChunkColumn::from_template();
    let mut files = std::collections::BTreeMap::new();
    for x in -radius..=radius {
        for z in -radius..=radius {
            let pos = ChunkPos::new(x, z);
            let bytes = encode_bytes(
                &template,
                &ChunkEncodeExtra {
                    chunk_pos: pos,
                    ..ChunkEncodeExtra::default()
                },
            )
            .map_err(|error| format!("encode template chunk {pos:?}: {error}"))?;
            let region_pos = mc_bamboo_storage::anvil::RegionPos::of(pos);
            let file = files.entry(region_pos.file_name()).or_insert_with(|| {
                RegionFile::open(
                    region_dir.join(region_pos.file_name()),
                    &region_dir,
                    RegionCompression::Zlib,
                )
            });
            let region = file
                .as_mut()
                .map_err(|error| format!("open {}: {error}", region_pos.file_name()))?;
            region
                .write(pos, Some(&bytes), 1)
                .map_err(|error| format!("write {pos:?}: {error}"))?;
        }
    }
    for (name, file) in &mut files {
        let region = file
            .as_mut()
            .map_err(|error| format!("open {name}: {error}"))?;
        region
            .sync()
            .map_err(|error| format!("sync {name}: {error}"))?;
    }
    println!(
        "SEED-WORLD wrote {} chunks into {} region files at {}",
        (2 * radius + 1) * (2 * radius + 1),
        files.len(),
        region_dir.display()
    );
    Ok(())
}
