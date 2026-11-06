//! 把提取到的数据写成 Rust 源文件。

use std::{fmt::Write, fs, path::Path};

use crate::extract::VersionData;

const HEADER: &str = "// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。\n\n";
const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
const DIMENSION_TYPE_REGISTRY: &str = "minecraft:dimension_type";
const WORLD_CLOCK_REGISTRY: &str = "minecraft:world_clock";

pub fn write_all(data: &VersionData, output_dir: &Path) -> Result<(), String> {
    write_file(output_dir, "mod.rs", &module_file())?;
    write_file(output_dir, "version.rs", &version_file(data))?;
    write_file(output_dir, "packets.rs", &packets_file(data))?;
    write_file(output_dir, "blocks.rs", &blocks_file(data))?;
    write_file(output_dir, "registries.rs", &registries_file(data))?;
    write_file(output_dir, "tags.rs", &tags_file(data))
}

fn write_file(output_dir: &Path, name: &str, content: &str) -> Result<(), String> {
    let path = output_dir.join(name);
    fs::write(&path, format!("{HEADER}{content}"))
        .map_err(|error| format!("无法写入 {}：{error}", path.display()))
}

fn module_file() -> String {
    "pub mod blocks;\npub mod packets;\npub mod registries;\npub mod tags;\npub mod version;\n"
        .to_owned()
}

fn version_file(data: &VersionData) -> String {
    let version = &data.version;
    let mut out = String::new();
    let _ = writeln!(out, "/// 这份数据对应的游戏版本。");
    let _ = writeln!(out, "pub const GAME_VERSION: &str = {:?};", version.name);
    let _ = writeln!(out, "/// 这个版本的网络协议号。");
    let _ = writeln!(
        out,
        "pub const PROTOCOL_VERSION: i32 = {};",
        version.protocol
    );
    let _ = writeln!(out, "/// 这个版本的世界数据版本号。");
    let _ = writeln!(
        out,
        "pub const DATA_VERSION: i32 = {};",
        version.data_version
    );
    out
}

/// `minecraft:level_chunk_with_light` → `LEVEL_CHUNK_WITH_LIGHT`。
fn constant_name(identifier: &str) -> String {
    let path = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    path.replace(['/', '.', '-'], "_").to_uppercase()
}

fn packets_file(data: &VersionData) -> String {
    let mut out = String::new();
    for (state, directions) in &data.packets {
        let _ = writeln!(out, "pub mod {state} {{");
        for (direction, packets) in directions {
            let _ = writeln!(out, "    pub mod {direction} {{");
            for (name, packet) in packets {
                let _ = writeln!(
                    out,
                    "        pub const {}: i32 = {};",
                    constant_name(name),
                    packet.protocol_id
                );
            }
            let _ = writeln!(out, "    }}");
        }
        let _ = writeln!(out, "}}\n");
    }
    out
}

fn blocks_file(data: &VersionData) -> String {
    let states = &data.block_states;
    let mut out = String::new();
    let _ = writeln!(out, "/// 方块状态的总数，决定了全局调色板的位数。");
    let _ = writeln!(out, "pub const TOTAL_STATES: u32 = {};\n", states.total);
    let _ = writeln!(out, "/// 各方块默认状态的 ID。");
    for (name, id) in &states.defaults {
        let _ = writeln!(out, "pub const {}: u32 = {id};", constant_name(name));
    }
    out
}

fn registries_file(data: &VersionData) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "use crate::{{KnownPack, SyncedRegistry}};\n");

    let _ = writeln!(out, "/// 客户端需要认识的官方内容包。");
    let _ = writeln!(out, "pub const KNOWN_PACKS: &[KnownPack] = &[");
    for pack in &data.known_packs {
        let _ = writeln!(
            out,
            "    KnownPack {{ namespace: {:?}, id: {:?}, version: {:?} }},",
            pack.namespace, pack.id, pack.version
        );
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(out, "/// 启用的功能开关。");
    let _ = writeln!(
        out,
        "pub const ENABLED_FEATURES: &[&str] = &{:?};\n",
        data.enabled_features
    );

    let _ = writeln!(out, "/// 配置阶段要同步给客户端的注册表，顺序和官方一致。");
    let _ = writeln!(out, "pub const SYNCED_REGISTRIES: &[SyncedRegistry] = &[");
    for registry in &data.synced_registries {
        let _ = writeln!(
            out,
            "    SyncedRegistry {{ name: {:?}, entries: &{:?} }},",
            registry.name, registry.entries
        );
    }
    let _ = writeln!(out, "];\n");

    write_registry_ids(&mut out, data, "biome", BIOME_REGISTRY);
    write_registry_ids(&mut out, data, "dimension_type", DIMENSION_TYPE_REGISTRY);
    write_registry_ids(&mut out, data, "world_clock", WORLD_CLOCK_REGISTRY);
    out
}

/// 把一个注册表的条目写成常量，数字 ID 就是条目在同步列表里的位置。
fn write_registry_ids(out: &mut String, data: &VersionData, module: &str, registry: &str) {
    let entries = data
        .synced_registries
        .iter()
        .find(|candidate| candidate.name == registry)
        .map_or(&[][..], |found| found.entries.as_slice());

    let _ = writeln!(out, "pub mod {module} {{");
    let _ = writeln!(out, "    pub const TOTAL: u32 = {};", entries.len());
    for (id, name) in entries.iter().enumerate() {
        let _ = writeln!(out, "    pub const {}: i32 = {id};", constant_name(name));
    }
    let _ = writeln!(out, "}}\n");
}

fn tags_file(data: &VersionData) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "use crate::{{Tag, TagGroup}};\n");
    let _ = writeln!(out, "/// 配置阶段要发给客户端的标签。");
    let _ = writeln!(out, "pub const TAG_GROUPS: &[TagGroup] = &[");
    for group in &data.tags {
        let _ = writeln!(
            out,
            "    TagGroup {{ registry: {:?}, tags: &[",
            group.registry
        );
        for tag in &group.tags {
            let _ = writeln!(
                out,
                "        Tag {{ name: {:?}, ids: &{:?} }},",
                tag.name, tag.ids
            );
        }
        let _ = writeln!(out, "    ] }},");
    }
    let _ = writeln!(out, "];");
    out
}
