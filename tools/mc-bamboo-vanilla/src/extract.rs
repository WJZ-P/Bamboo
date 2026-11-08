//! 运行官方数据生成器和 `Extractor.java`，读回它们的输出。

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

const BLOCK_REGISTRY: &str = "minecraft:block";
const ITEM_REGISTRY: &str = "minecraft:item";
/// 官方数据生成器的输出目录，相对于工作目录。
const GENERATED_DIR: &str = "generated";

/// 官方报告里一个连接状态的数据包：方向 → 包名 → ID。
pub type PacketReport = BTreeMap<String, BTreeMap<String, PacketEntry>>;

#[derive(Debug, Deserialize)]
pub struct PacketEntry {
    pub protocol_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
    pub blocks: Vec<Block>,
    pub items: Vec<ExtractedItem>,
    pub state_tables: StateTables,
    pub light_tables: LightTables,
    pub block_maps: BlockMaps,
    pub block_transformers: Vec<BlockTransformer>,
    pub heightmaps: Vec<HeightmapType>,
    pub component_enums: ComponentEnums,
    /// 每个物品的默认组件，按 `DataComponentPatch.STREAM_CODEC` 编成十六进制。
    pub item_components: Vec<String>,
}

/// 按 ID 查表的枚举：连续的只有常量个数，稀疏的是整张 ID 表。
#[derive(Debug, Deserialize)]
pub struct ComponentEnums {
    /// 枚举名 → 常量个数。
    pub counts: BTreeMap<String, u32>,
    /// 枚举名 → 按声明顺序的 ID。
    pub sparse: BTreeMap<String, Vec<i32>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub name: String,
    pub protocol: i32,
    pub data_version: i32,
}

#[derive(Debug, Deserialize)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncedRegistry {
    pub name: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TagGroup {
    pub registry: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub name: String,
    pub ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BlockStates {
    pub total: u32,
    pub defaults: BTreeMap<String, u32>,
}

/// 一个方块。列表里的位置就是它的注册表 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub name: String,
    /// 全部状态的 ID，顺序和官方一致。
    pub states: Vec<u32>,
    pub default_state: u32,
    pub item: i32,
    pub has_collision: bool,
    pub has_fluid: bool,
    pub two_blocks: bool,
    /// 活塞推动时的反应，官方 `PushReaction` 名字的小写形式。
    pub push_reaction: String,
    /// 破坏速度是 -1：活塞推不动，也打不碎。
    pub unbreakable: bool,
    pub block_entity_type: Option<i32>,
    /// 官方的类名，比如 `SlabBlock`。
    pub class: String,
    /// 这个类、它的父类和方块包里的接口。
    pub ancestors: Vec<String>,
    pub properties: Vec<Property>,
    pub fields: Vec<BlockField>,
    pub instrument: String,
    pub dynamic_shape: bool,
    pub offset: Option<BlockOffset>,
}

/// 一个方块属性，顺序和取值都是官方 `StateDefinition` 里的。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
}

/// 类的一个构造参数或者只读方法的结果。方法名后面带括号。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BlockField {
    pub name: String,
    pub value: FieldValue,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Double(f64),
    Block(i32),
    State(u32),
    Item(i32),
    Direction(i32),
    Tag(String),
    Name(String),
}

/// 方块的渲染偏移，形状跟着它走的方块碰撞也和位置有关。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockOffset {
    pub vertical: bool,
    pub max_horizontal: f32,
    pub max_vertical: f32,
    pub moves_shape: bool,
}

/// 每个状态一项的表和去重后的形状表。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateTables {
    pub facts: Vec<u64>,
    pub fluid: Vec<u8>,
    pub light_dampening: Vec<u8>,
    pub collision: Vec<u16>,
    pub outline: Vec<u16>,
    pub support: Vec<u16>,
    /// `placementContext` 下和普通碰撞不同的状态：状态 ID 和形状下标。
    pub placement_collision: Vec<(u32, u16)>,
    /// 形状跟着偏移走的状态在几个位置上的 `isFaceSturdy`：状态 ID、坐标和位。
    pub offset_sturdy_samples: Vec<(u32, i32, i32, i32, u32)>,
    pub fluid_states: Vec<FluidState>,
    /// 每个形状的盒子，一个盒子是 `min_x, min_y, min_z, max_x, max_y, max_z`。
    pub shapes: Vec<Vec<[f64; 6]>>,
    pub shape_faces: Vec<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FluidState {
    pub fluid: i32,
    pub source: bool,
    pub amount: u8,
    pub falling: bool,
    pub legacy_block: u32,
}

/// 光照引擎要的表，`Extractor.java` 的 `LightTables`。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightTables {
    /// 每个状态的 `getLightEmission`。
    pub emission: Vec<u8>,
    /// 每个状态在 `occlusion_faces` 里的下标。
    pub occlusion: Vec<u16>,
    /// 去重后的 6 个面的遮挡形状，按 `Direction` 的 ID 排列，第一项全是空形状。
    pub occlusion_faces: Vec<[u16; 6]>,
    /// 去重后的面形状个数。
    pub face_count: usize,
    /// 两两之间的 `faceShapeOccludes`，Java 的 `long` 是有符号的。
    pub face_occludes: Vec<i64>,
}

/// `Heightmap.Types` 的一种。区块包按 `id` 排列，`id` 也是网络上的键。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeightmapType {
    /// 官方的序列化名，比如 `MOTION_BLOCKING_NO_LEAVES`。
    pub name: String,
    pub id: i32,
    /// `sendToClient`：区块包带上这一种。
    pub send_to_client: bool,
    /// `keepAfterWorldgen`：区块里一直留着这一种。
    pub keep_after_worldgen: bool,
}

/// 官方写在静态表里的方块对应关系，都是方块注册表 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockMaps {
    pub flower_pots: Vec<(i32, i32)>,
    pub candle_cakes: Vec<(i32, i32)>,
    pub weathering: Vec<(i32, i32)>,
    pub waxables: Vec<(i32, i32)>,
    /// 方块、`igniteOdds`、`burnOdds`。
    pub fire_odds: Vec<(i32, i32, i32)>,
    /// `ignitedByLava` 的方块 ID，升序。
    pub ignited_by_lava: Vec<i32>,
    /// 物品注册表 ID。
    pub campfire_inputs: Vec<i32>,
    /// 数据包 `context_int_provider/compostable` 下的提供器原文，键是名字。
    pub compost_layers: BTreeMap<String, serde_json::Value>,
}

/// 世界层注册表里的一个 `BlockTransformer`，只有改方块的部分。
#[derive(Debug, Deserialize)]
pub struct BlockTransformer {
    pub name: String,
    pub transforms: Vec<BlockTransform>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockTransform {
    pub provider: StateProvider,
    /// `Direction` 的 ID。
    pub disallowed_faces: Vec<i32>,
    pub update_from_neighbors: bool,
}

/// `Extractor.java` 只导出这几种状态提供器，别的在那边就报错。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateProvider {
    /// 状态 ID。
    Simple(u32),
    CopyProperties(Box<StateProvider>),
    RuleBased {
        rules: Vec<StateRule>,
        fallback: Option<Box<StateProvider>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateRule {
    pub if_true: BlockPredicate,
    pub then: StateProvider,
}

/// `Extractor.java` 只导出这几种方块判定，别的在那边就报错。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockPredicate {
    /// 方块注册表 ID。
    MatchingBlocks {
        offset: [i32; 3],
        blocks: Vec<i32>,
    },
    MatchingBlockTag {
        offset: [i32; 3],
        tag: String,
    },
    AllOf(Vec<BlockPredicate>),
}

/// `Extractor.java` 导出的物品。物品组件在官方报告里，见 [`Item`]。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedItem {
    pub name: String,
    /// 官方的类名，比如 `BlockItem`。
    pub class: String,
    pub block: Option<i32>,
    pub wall_block: Option<i32>,
    pub attachment: Option<i32>,
    pub overrides_can_destroy_block: bool,
}

/// 一个物品。列表里的位置就是它的注册表 ID。
pub struct Item {
    pub name: String,
    pub class: String,
    pub block: Option<i32>,
    pub wall_block: Option<i32>,
    pub attachment: Option<i32>,
    pub max_stack_size: i32,
    pub can_destroy_blocks_in_creative: bool,
    pub consumable: bool,
    /// `compostable` 组件的 `layers`。
    pub compostable: Option<CompostLayers>,
    pub jukebox_playable: bool,
    /// `block_transformer` 组件指向的注册表条目。
    pub block_transformer: Option<String>,
    pub blocks_attacks: bool,
}

/// `reports/minecraft/components/item` 里一个物品的报告，只取用到的组件。
#[derive(Debug, Deserialize)]
struct ItemReport {
    components: ItemComponents,
}

#[derive(Debug, Deserialize)]
struct ItemComponents {
    #[serde(rename = "minecraft:max_stack_size")]
    max_stack_size: i32,
    #[serde(rename = "minecraft:tool")]
    tool: Option<ToolComponent>,
    #[serde(rename = "minecraft:consumable")]
    consumable: Option<serde_json::Value>,
    #[serde(rename = "minecraft:compostable")]
    compostable: Option<CompostableComponent>,
    #[serde(rename = "minecraft:jukebox_playable")]
    jukebox_playable: Option<serde_json::Value>,
    /// 报告里写注册表条目的名字。直接内联的写法读不了，生成会失败。
    #[serde(rename = "minecraft:block_transformer")]
    block_transformer: Option<String>,
    #[serde(rename = "minecraft:blocks_attacks")]
    blocks_attacks: Option<serde_json::Value>,
}

/// 字段等于默认值 `true` 时报告不写它。
#[derive(Debug, Deserialize)]
struct ToolComponent {
    can_destroy_blocks_in_creative: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CompostableComponent {
    layers: serde_json::Value,
}

/// 往堆肥桶里放一个物品加几层。只认原版数据包用到的两种写法，别的写法生成失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompostLayers {
    /// 固定的层数，不取随机数。
    Constant(i32),
    /// 堆肥桶空着时加 `when_empty` 层，否则按权重抽一项：（层数，权重）。
    Weighted {
        when_empty: i32,
        distribution: Vec<(i32, i32)>,
    },
}

/// 整数提供器里堆肥用到的写法：内联常数、引用数据包条目、带 `type` 的对象。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IntProvider {
    Constant(i32),
    Reference(String),
    Typed(TypedIntProvider),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum TypedIntProvider {
    #[serde(rename = "minecraft:constant")]
    Constant { value: i32 },
    #[serde(rename = "minecraft:number_dispatcher")]
    Dispatcher {
        cases: Vec<DispatcherCase>,
        default: Box<IntProvider>,
    },
    #[serde(rename = "minecraft:weighted_list")]
    WeightedList { distribution: Vec<WeightedEntry> },
}

#[derive(Debug, Deserialize)]
struct DispatcherCase {
    condition: serde_json::Value,
    value: IntProvider,
}

#[derive(Debug, Deserialize)]
struct WeightedEntry {
    data: IntProvider,
    weight: i32,
}

/// `compostable` 组件的 `layers` 解析成 [`CompostLayers`]。
fn compost_layers(
    layers: &serde_json::Value,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<CompostLayers, String> {
    match resolve_int_provider(parse_int_provider(layers)?, providers)? {
        IntProvider::Typed(TypedIntProvider::Dispatcher { cases, default }) => {
            let [case] = <[DispatcherCase; 1]>::try_from(cases)
                .map_err(|_| format!("unsupported compostable layers: {layers}"))?;
            if !is_empty_composter(&case.condition) {
                return Err(format!("unsupported compostable condition: {layers}"));
            }
            let IntProvider::Typed(TypedIntProvider::WeightedList { distribution }) =
                resolve_int_provider(*default, providers)?
            else {
                return Err(format!("unsupported compostable default: {layers}"));
            };
            let distribution = distribution
                .into_iter()
                .map(|entry| Ok((constant_int(entry.data, providers)?, entry.weight)))
                .collect::<Result<_, String>>()?;
            Ok(CompostLayers::Weighted {
                when_empty: constant_int(case.value, providers)?,
                distribution,
            })
        }
        provider => Ok(CompostLayers::Constant(constant_int(provider, providers)?)),
    }
}

fn parse_int_provider(value: &serde_json::Value) -> Result<IntProvider, String> {
    IntProvider::deserialize(value).map_err(|error| format!("bad int provider {value}: {error}"))
}

/// 引用换成数据包里的条目，别的原样返回。
fn resolve_int_provider(
    provider: IntProvider,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<IntProvider, String> {
    let IntProvider::Reference(name) = provider else {
        return Ok(provider);
    };
    let value = providers
        .get(&name)
        .ok_or_else(|| format!("unknown int provider {name}"))?;
    resolve_int_provider(parse_int_provider(value)?, providers)
}

fn constant_int(
    provider: IntProvider,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<i32, String> {
    match resolve_int_provider(provider, providers)? {
        IntProvider::Constant(value) | IntProvider::Typed(TypedIntProvider::Constant { value }) => {
            Ok(value)
        }
        other => Err(format!("expected a constant int provider, got {other:?}")),
    }
}

/// 条件是“方块是 `level` 为 0 的堆肥桶”。
fn is_empty_composter(condition: &serde_json::Value) -> bool {
    *condition
        == serde_json::json!({
            "type": "minecraft:match_block",
            "blocks": "minecraft:composter",
            "state": { "level": "0" },
        })
}

/// `registries.json` 里的一个注册表：条目名 → ID。
#[derive(Debug, Deserialize)]
struct RegistryReport {
    entries: BTreeMap<String, RegistryEntry>,
}

#[derive(Debug, Deserialize)]
struct RegistryEntry {
    protocol_id: usize,
}

/// 生成代码需要的全部输入。
pub struct VersionData {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub state_tables: StateTables,
    pub light_tables: LightTables,
    pub block_maps: BlockMaps,
    /// 按名字排序。
    pub block_transformers: Vec<BlockTransformer>,
    /// 按官方 `Heightmap.Types.values()` 的顺序。
    pub heightmaps: Vec<HeightmapType>,
    /// 内置注册表名 → 按 ID 排列的条目。
    pub builtin_registries: BTreeMap<String, Vec<String>>,
    /// 连接状态 → 方向 → 包名 → ID。
    pub packets: BTreeMap<String, PacketReport>,
    /// 组件里按 ID 查表的枚举。
    pub component_enums: ComponentEnums,
    /// 每个物品的默认组件，按 `DataComponentPatch.STREAM_CODEC` 编成十六进制。
    pub item_components: Vec<String>,
}

pub fn run(
    java: &str,
    server_jar: &Path,
    work_dir: &Path,
    workspace_root: &Path,
) -> Result<VersionData, String> {
    run_reports(java, server_jar, work_dir)?;
    let extracted_path = work_dir.join("extracted.json");
    run_extractor(java, work_dir, workspace_root, &extracted_path)?;

    let extracted: Extracted = read_json(&extracted_path)?;
    let reports = work_dir.join(GENERATED_DIR).join("reports");
    let packets = read_packet_report(&reports.join("packets.json"))?;
    let builtin_registries = read_registry_report(&reports.join("registries.json"))?;
    check_registry_order(
        &builtin_registries,
        BLOCK_REGISTRY,
        extracted.blocks.iter().map(|block| &block.name),
    )?;
    check_registry_order(
        &builtin_registries,
        ITEM_REGISTRY,
        extracted.items.iter().map(|item| &item.name),
    )?;
    let items = read_items(
        extracted.items,
        &reports.join("minecraft/components/item"),
        &extracted.block_maps.compost_layers,
    )?;
    Ok(VersionData {
        version: extracted.version,
        known_packs: extracted.known_packs,
        enabled_features: extracted.enabled_features,
        synced_registries: extracted.synced_registries,
        tags: extracted.tags,
        block_states: extracted.block_states,
        blocks: extracted.blocks,
        items,
        state_tables: extracted.state_tables,
        light_tables: extracted.light_tables,
        block_maps: extracted.block_maps,
        block_transformers: extracted.block_transformers,
        heightmaps: extracted.heightmaps,
        builtin_registries,
        packets,
        component_enums: extracted.component_enums,
        item_components: extracted.item_components,
    })
}

/// 运行官方的数据生成器。它同时会把依赖库解压到工作目录里。
fn run_reports(java: &str, server_jar: &Path, work_dir: &Path) -> Result<(), String> {
    // 传入解压出来的内层 jar 时，这条命令进的是服务端入口，什么也不生成，退出码却是 0。
    // 先删掉旧输出，否则后面的步骤会拿旧报告生成数据，还报告成功。
    let output = work_dir.join(GENERATED_DIR);
    if output.exists() {
        fs::remove_dir_all(&output)
            .map_err(|error| format!("无法清理 {}：{error}", output.display()))?;
    }

    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-DbundlerMainClass=net.minecraft.data.Main")
        .arg("-jar")
        .arg(server_jar)
        .args(["--reports", "--output", GENERATED_DIR])
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if !status.success() {
        return Err(format!("官方数据生成器退出失败：{status}"));
    }
    if output.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "{} 没有生成报告，这里要用官方下载的 server.jar，不是解压出来的内层 jar",
            server_jar.display()
        ))
    }
}

fn run_extractor(
    java: &str,
    work_dir: &Path,
    workspace_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let source = workspace_root.join("tools/mc-bamboo-vanilla/java/Extractor.java");
    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-cp")
        .arg(classpath(work_dir)?)
        .arg(source)
        .arg(output)
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Extractor.java 退出失败：{status}"))
    }
}

/// 官方数据生成器解压出来的全部 jar，运行 Java 工具时用作 classpath。
pub fn classpath(work_dir: &Path) -> Result<OsString, String> {
    let mut jars = Vec::new();
    collect_jars(&work_dir.join("libraries"), &mut jars)?;
    collect_jars(&work_dir.join("versions"), &mut jars)?;
    env::join_paths(jars).map_err(|error| error.to_string())
}

fn collect_jars(directory: &Path, jars: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("无法读取 {}：{error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_jars(&path, jars)?;
        } else if path.extension().is_some_and(|extension| extension == "jar") {
            jars.push(path);
        }
    }
    Ok(())
}

fn read_packet_report(path: &Path) -> Result<BTreeMap<String, PacketReport>, String> {
    read_json(path)
}

fn read_registry_report(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let report: BTreeMap<String, RegistryReport> = read_json(path)?;
    report
        .into_iter()
        .map(|(name, registry)| {
            let entries = entries_by_id(&name, registry)?;
            Ok((name, entries))
        })
        .collect()
}

/// 按 ID 排列条目。生成的代码把条目的位置当作 ID，所以 ID 必须从 0 开始连续。
fn entries_by_id(name: &str, registry: RegistryReport) -> Result<Vec<String>, String> {
    let mut entries: Vec<_> = registry.entries.into_iter().collect();
    entries.sort_by_key(|(_, entry)| entry.protocol_id);
    if entries
        .iter()
        .enumerate()
        .any(|(id, (_, entry))| entry.protocol_id != id)
    {
        return Err(format!("{name} 的 ID 不是从 0 开始连续的"));
    }
    Ok(entries.into_iter().map(|(entry, _)| entry).collect())
}

/// `Extractor.java` 是按遍历顺序导出的，这里确认这个顺序就是注册表 ID。
fn check_registry_order<'a>(
    registries: &BTreeMap<String, Vec<String>>,
    registry: &str,
    names: impl Iterator<Item = &'a String>,
) -> Result<(), String> {
    let entries = registries
        .get(registry)
        .ok_or_else(|| format!("注册表报告里没有 {registry}"))?;
    if names.eq(entries) {
        Ok(())
    } else {
        Err(format!("{registry} 的导出顺序和注册表报告不一致"))
    }
}

/// 给物品补上组件报告里的数据。
fn read_items(
    extracted: Vec<ExtractedItem>,
    reports: &Path,
    compost_providers: &BTreeMap<String, serde_json::Value>,
) -> Result<Vec<Item>, String> {
    extracted
        .into_iter()
        .map(|item| {
            let file = item.name.strip_prefix("minecraft:").unwrap_or(&item.name);
            let report: ItemReport = read_json(&reports.join(format!("{file}.json")))?;
            let components = report.components;
            let tool_allows = components
                .tool
                .is_none_or(|tool| tool.can_destroy_blocks_in_creative != Some(false));
            Ok(Item {
                max_stack_size: components.max_stack_size,
                can_destroy_blocks_in_creative: tool_allows && !item.overrides_can_destroy_block,
                consumable: components.consumable.is_some(),
                compostable: components
                    .compostable
                    .map(|component| compost_layers(&component.layers, compost_providers))
                    .transpose()?,
                jukebox_playable: components.jukebox_playable.is_some(),
                block_transformer: components.block_transformer,
                blocks_attacks: components.blocks_attacks.is_some(),
                name: item.name,
                class: item.class,
                block: item.block,
                wall_block: item.wall_block,
                attachment: item.attachment,
            })
        })
        .collect()
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} 格式不对：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(entries: &[(&str, usize)]) -> RegistryReport {
        let entries = entries
            .iter()
            .map(|&(name, protocol_id)| (name.to_owned(), RegistryEntry { protocol_id }))
            .collect();
        RegistryReport { entries }
    }

    #[test]
    fn sorts_registry_entries_by_id() {
        let entries = entries_by_id("test", registry(&[("a", 1), ("b", 0)]));
        assert_eq!(entries, Ok(vec!["b".to_owned(), "a".to_owned()]));
    }

    #[test]
    fn rejects_registry_ids_that_are_not_positions() {
        assert!(entries_by_id("test", registry(&[("a", 0), ("b", 2)])).is_err());
        assert!(entries_by_id("test", registry(&[("a", 1)])).is_err());
    }

    #[test]
    fn export_order_has_to_match_the_report() {
        let first = "a".to_owned();
        let second = "b".to_owned();
        let report = BTreeMap::from([("test".to_owned(), vec![first.clone(), second.clone()])]);

        let check = |registry, names: &[&String]| {
            check_registry_order(&report, registry, names.iter().copied())
        };
        assert_eq!(check("test", &[&first, &second]), Ok(()));
        assert!(check("test", &[&second, &first]).is_err());
        assert!(check("test", &[&first]).is_err());
        assert!(check("missing", &[&first, &second]).is_err());
    }
}
