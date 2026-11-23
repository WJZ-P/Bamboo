//! 把一段调用栈归到一个「归属」上的规则。
//!
//! 规则按顺序看，第一条有任意一个模式出现在任意一层函数名里的规则胜出，所以具体的
//! 规则放前面，兜底的放后面。
//!
//! Rules that map a call stack to an owner. Rules are tried in order and the
//! first one with any pattern found in any frame wins, so specific rules come
//! first and catch-alls last.

/// `(归属, 模式列表)`。
///
/// `(owner, patterns)`.
const RULES: &[(&str, &[&str])] = &[
    (
        "worldgen: structure templates (parsed NBT)",
        &["structure::template::"],
    ),
    (
        "worldgen data: shared Worldgen (raw JSON, noise settings, density, biomes)",
        &["Worldgen::parse", "Worldgen::load", "Worldgen::shared"],
    ),
    (
        "worldgen per thread: feature registry copy (cloned JSON)",
        &["feature::registry::Registry::parse"],
    ),
    (
        "worldgen per thread: biome climate search tree copy",
        &["climate::bucketize", "climate::build"],
    ),
    (
        "worldgen per thread: template pools and structure state",
        &[
            "TemplatePools::parse",
            "WorldStructureState",
            "worldgen_structures::with_thread",
        ],
    ),
    (
        "worldgen per thread: generators (seeded noises, compiled samplers)",
        &[
            "Generator::with_settings",
            "sampler::Compiler",
            "NoiseStackBuilder",
            "SeededRouter",
        ],
    ),
    (
        "worldgen cache: terrain (hot and cold tiers)",
        &[
            "worldgen_compact::",
            "worldgen::cached_terrain",
            "Generator::terrain_inner",
            "worldgen_cache::",
        ],
    ),
    (
        "worldgen cache: feature writes",
        &[
            "worldgen_features::center_writes",
            "feature::world::last_writes",
            "FeatureWorld::new",
            "run_features_filtered",
            "worldgen_features::",
        ],
    ),
    (
        "worldgen: other (in-flight generation, small tables)",
        &["mc_bamboo_worldgen::", "mc_bamboo_game::worldgen"],
    ),
    (
        "chunk packets: encoded and compressed frames",
        &[
            "chunk_encoding::",
            "SharedFrames",
            "chunk_tracking::send_chunks",
        ],
    ),
    (
        "save path: chunks packed for disk, region files",
        &["packed_for_disk", "mc_bamboo_storage::", "save_session::"],
    ),
    ("loaded chunks: light", &["mc_bamboo_world::light::"]),
    (
        "loaded chunks: blocks, biomes, heightmaps, columns",
        &[
            "mc_bamboo_world::chunk::",
            "mc_bamboo_world::column::",
            "mc_bamboo_world::palette::",
            "mc_bamboo_world::store::",
            "RegionColumn",
            "DirectorHost",
        ],
    ),
    (
        "chunk director and tickets",
        &["mc_bamboo_world::director::", "mc_bamboo_world::ticket::"],
    ),
    (
        "item component defaults",
        &["mc_bamboo_protocol::components::"],
    ),
    ("command graph", &["mc_bamboo_command::"]),
    ("loot tables", &["mc_bamboo_loot::"]),
    ("recipes", &["mc_bamboo_recipes::"]),
    ("tick profiler", &["mc_bamboo_game::profiler::"]),
    (
        "sin/cos lookup tables",
        &["sin_table", "mth::cos", "mth::sin", "mth_sin"],
    ),
    (
        "world thread: regions, players, entities, everything else in game",
        &["mc_bamboo_game::", "mc_bamboo_blocks::"],
    ),
    (
        "network: connections, tokio runtime, TLS",
        &["mc_bamboo_server::", "tokio::", "mc_bamboo_auth::"],
    ),
    (
        "logging and profiler bookkeeping",
        &["tracing", "mc_bamboo_memprof::"],
    ),
];

pub fn owner_of(names: &[String]) -> &'static str {
    for (owner, patterns) in RULES {
        let hit = patterns
            .iter()
            .any(|pattern| names.iter().any(|name| name.contains(pattern)));
        if hit {
            return owner;
        }
    }
    "other (no rule matched)"
}
