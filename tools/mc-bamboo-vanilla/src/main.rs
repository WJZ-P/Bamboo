//! 和官方服务端打交道的开发工具。
//!
//! ```text
//! mc-bamboo-vanilla generate <server.jar> [java 路径]
//!     从官方服务端生成 mc-bamboo-data 的版本数据和丛林神殿指令表
//! mc-bamboo-vanilla extract [java 路径]
//!     不重跑官方数据生成器，用 target/vanilla 里上次的报告重新导出数据
//! mc-bamboo-vanilla probe <地址> <端口> [java 路径]
//!     用官方网络代码连接正在运行的 Bamboo，检查它发的每个包
//! mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure]
//!                          [--case I] [--show N] [--show-blocked] [--rule 名字=值]
//!     让官方代码给方块行为出题和给答案，用 mc-bamboo-blocks 重放，比较两边的结果
//! mc-bamboo-vanilla light <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
//!     让官方光照引擎在随机场景里算光照，用 mc-bamboo-world 重放，逐段比较
//! mc-bamboo-vanilla menu <java 路径> [--cases N] [--seed S] [--case I] [--show N]
//!     让官方容器菜单跑随机点击，用 mc-bamboo-game 重放，逐槽、逐个 state_id 比较
//! mc-bamboo-vanilla command-oracle <java 路径> [--show N] [--break-self-test]
//!     把实体选择器的解析、过滤和排序交给官方 `EntitySelectorParser` 对拍
//! mc-bamboo-vanilla execute-oracle <java 路径> [--show N] [--break-self-test]
//!     用官方 `Commands` 跑 42 条整命令（/execute 与 /data），比较方块、分数、NBT 和反馈
//! mc-bamboo-vanilla components <java 路径> [--show N]
//!     让官方代码编码一大批物品组件，用 mc-bamboo-protocol 解码再编码，逐字节比较
//! mc-bamboo-vanilla predicate-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 BlockPredicate 对随机方块状态和随机谓词出题，用 mc-bamboo-blocks 重放并比较
//! mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 RecipeManager 对大量网格和输入查配方，用 mc-bamboo-recipes 重放并比较
//! mc-bamboo-vanilla loot-oracle <java 路径> [--entity-table ID] [--cases N] [--seed S] [--show N]
//!     让官方战利品表对随机方块、工具、爆炸和箱子种子出题，用 mc-bamboo-loot 重放并比较
//! mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N]
//!                            [--biome-points N] [--show N]
//!     让官方代码算世界生成的噪声、密度函数和群系，用 mc-bamboo-worldgen 重算，浮点逐位比较
//! mc-bamboo-vanilla structures <java 路径>
//!     用官方 RandomSpreadStructurePlacement 对拍结构放置、加权选择和频率判定
//! mc-bamboo-vanilla structure-rings <java 路径>
//!     用官方 getRingPositionsFor / findBiomeHorizontal 对拍要塞环位置与群系吸附
//! mc-bamboo-vanilla structure-validity <java 路径>
//!     用官方 Structure.GenerationContext 对拍 findValidGenerationPoint 的群系与高度判定
//! mc-bamboo-vanilla beard <java 路径> [--through noise|surface|carvers] [--break-self-test]
//!     用官方 Beardifier 对拍结构对地形密度的影响（kernel / sampleValue / sampleVolume / 方块）
//!
//!     Cross-check structure influence on terrain density against vanilla Beardifier
//!     (kernel / sampleValue / sampleVolume / blocks)
//!
//!     Compare findValidGenerationPoint's biome and height gates against vanilla
//!     Structure.GenerationContext
//! mc-bamboo-vanilla structure-generation <java 路径>
//!     用官方 Structure.findValidGenerationPoint 对拍四个代码生成结构的
//!     findGenerationPoint 前置判定与「这个起点是否真的生成」
//!
//!     Compare the four code-built structures' findGenerationPoint gates and
//!     "whether this start really generates" against vanilla
//! mc-bamboo-vanilla structure-pieces <java 路径>
//!     用官方结构件类对拍多件式结构的结构件布局（类型、包围盒、朝向、参数）；
//!     已移植废弃矿井 / 要塞 / 海底神殿三种
//! mc-bamboo-vanilla structure-single <java 路径>
//!     用官方结构件类对拍四个代码生成单件结构的结构件布局（类型、包围盒、朝向、参数）
//!
//!     Compare the piece layout of the four code-built single-piece structures against
//!     the official piece classes
//! mc-bamboo-vanilla structure-templated <java 路径>
//!     用官方模板结构件类对拍需要 `.nbt` 模板的单件结构布局（类型、包围盒、朝向、参数）；
//!     已移植雪屋
//!
//!     Compare the piece layout of template-based single-piece structures against the
//!     official template piece classes; the igloo is ported
//! mc-bamboo-vanilla structure-ruined-portal <java 路径>
//!     用官方 RuinedPortalStructure.findValidGenerationPoint 对拍废弃传送门的结构件布局
//!     （含 findSuitableY 的整列搜索与 Mirror.FRONT_BACK）
//!
//!     Compare the ruined portal piece layout against vanilla, covering findSuitableY's
//!     full-column scan and Mirror.FRONT_BACK
//! mc-bamboo-vanilla structure-templated-place <java 路径>
//!     用官方 postProcess 逐方块对拍雪屋 / 沉船 / 海底遗迹的模板放置
//!
//!     Compare the igloo / shipwreck / ocean ruin template placement block by block
//!     against the official postProcess
//! mc-bamboo-vanilla structure-save <java 路径>
//!     用官方 CompoundTag / PieceContainer 对拍区块 NBT 的 structures 键（starts / References）
//!
//!     Compare the chunk NBT structures key (starts / References) against vanilla codecs
//! mc-bamboo-vanilla structure-templates <java 路径>
//!     用官方 NbtIo 逐节点对拍导出的结构模板 NBT
//!
//!     Compare exported structure template NBT node by node with vanilla NbtIo
//! mc-bamboo-vanilla jigsaw <java 路径> [templates|jigsaws|pieces|piecesbiome|piecesexact|pools|save|load|processors|apply|capped|cappedraw|cappedfull|placepiece|waterpiece|entities|bemod|rc|shapes]
//!     用官方 StructureTemplateManager / JigsawPlacement 对拍 jigsaw 结构件
//!
//!     Compare jigsaw pieces with vanilla StructureTemplateManager / JigsawPlacement
//!     (`templates` = template sizes and block counts, `jigsaws` = getJigsaws after the
//!     position-based palette choice and rotation, `pieces` = the full addPieces output)
//! mc-bamboo-vanilla machine <java 路径> [--break-self-test 名字]
//!     让官方熔炉、酿造台、信标和潮涌核心在假世界里逐刻跑，比较进度、物品、方块状态和世界随机源
//! mc-bamboo-vanilla entity-oracle <java 路径>
//!     让官方 `FallingBlockEntity.tick` 和 `PrimedTnt.tick` 在假世界里跑，
//!     用 Bamboo 重放，逐刻比较位置、速度和落地后的方块
//! mc-bamboo-vanilla path-type <java 路径> [--break-self-test] [--show N]
//!     把每个方块状态预算的寻路类型和官方 WalkNodeEvaluator 逐项对拍
//! mc-bamboo-vanilla move-oracle <java 路径> [--cases N] [--seed S] [--show N]
//! mc-bamboo-vanilla mob-oracle <java 路径> [--only 前缀] [--show N]
//!     让官方 `handleMovePlayer` 对一组移动包出题，用 Bamboo 重放，
//!     比较接受、拉回还是断开，以及最终位置和摔落距离
//! mc-bamboo-vanilla storage <java 路径> [--show N]
//!     用官方 SerializableChunkData / RegionFile / NbtIo 检查存档编解码层
//! mc-bamboo-vanilla player-data <java 路径> [--show N]
//!     用官方 ItemStackWithSlot / TagValueInput / PrimaryLevelData 检查物品、玩家数据和 level.dat
//! mc-bamboo-vanilla poi-oracle <java 路径> [--show N] [--break-self-test NAME]
//!     用官方 `PoiManager` 对拍 POI 查询、票数、存档 NBT 与村庄距离
//! mc-bamboo-vanilla chunk-content <java 路径> [--show N]
//!     用官方 BlockEntity / SavedTick / Entity 检查区块上层内容的存档 NBT
//! mc-bamboo-vanilla ticket-oracle <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
//!     让官方 `DistanceManager` 在随机票操作里算区块等级，用 `TicketTable` 重放，逐步比较两套等级
//! mc-bamboo-vanilla border-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 `WorldBorder` 对随机的中心、大小和渐变出题，比对数学、逐刻推进和六个包的编码
//! mc-bamboo-vanilla spawn <java 路径> [--seeds N] [--salt S] [--show N] [--level-type normal|large_biomes|amplified] [--break-self-test]
//!     多种子下把新世界的出生区块和出生点与官方逐个对拍；--break-self-test 故意改坏一处
//!     Compare fresh-world spawn chunks and positions against vanilla across seeds;
//!     --break-self-test deliberately corrupts one spawn.
//! mc-bamboo-vanilla natural-spawn <java 路径> [--show N] [--break-self-test]
//!     对每个群系的每个 MobCategory 用固定种子重放官方的加权刷怪选择和组大小
//!     Replay vanilla weighted spawn picks, group sizes and the chunk shuffle per seed.
//! mc-bamboo-vanilla flat-world <java 路径> [--show N] [--break-self-test]
//!     把 generator-settings 的五个字段和超平坦地形逐方块对拍官方 FlatLevelSource
//!     Compare the five generator-settings fields and flat terrain block-by-block
//!     with vanilla FlatLevelSource; --break-self-test corrupts one block.
//! ```
//!
//! 升级游戏版本时，换一个 `server.jar` 运行 `generate`，看生成文件的差异，
//! 然后运行 `probe` 确认协议没有问题。这些命令都需要 Java 25 或更新版本，
//! `probe`、`oracle`、`light` 和 `menu` 要先运行过 `generate`。
//!
//! `server.jar` 是官方下载的那个文件，不是 `target/vanilla/versions` 里解压出来的内层 jar。
//!
//! `probe` 连的是你自己启动的那个 Bamboo，它跑的是离线模式。正版验证的那一项检查连不了
//! 这个服务端（`online_mode` 要在启动时定下来），所以 `probe` 会在自己的进程里再起一个开了
//! 正版验证的服务端，外加一个假的 session server 顶替 Mojang，全程只走 127.0.0.1。
//!
//! `oracle` 和 `probe-online` 依赖生成的数据。生成文件坏了、这个工具编译不过时，
//! 用 `--no-default-features` 编译，`generate` 不需要它们。

#[cfg(feature = "oracle")]
mod advancement_command_oracle;
#[cfg(feature = "oracle")]
mod advancement_oracle;
#[cfg(feature = "oracle")]
mod advancement_trigger_oracle;
#[cfg(feature = "oracle")]
mod beard;
#[cfg(feature = "oracle")]
mod chunk_content_oracle;
#[cfg(feature = "oracle")]
mod chunkgen;
#[cfg(feature = "oracle")]
mod clock_oracle;
#[cfg(feature = "oracle")]
mod command_oracle;
#[cfg(feature = "oracle")]
mod components;
#[cfg(feature = "oracle")]
mod entity_oracle;
#[cfg(feature = "oracle")]
mod execute_oracle;
mod extract;
#[cfg(feature = "oracle")]
mod feature_merge;
#[cfg(feature = "oracle")]
mod flat_world;
#[cfg(feature = "oracle")]
mod gen_light;
#[cfg(feature = "oracle")]
mod gen_world;
mod generate;
#[cfg(feature = "oracle")]
mod jigsaw;
#[cfg(feature = "oracle")]
mod jigsaw_shapes;
mod jungle_temple_ops;
#[cfg(feature = "oracle")]
mod light;
#[cfg(feature = "oracle")]
mod light_pipeline;
#[cfg(feature = "oracle")]
mod long_set;
#[cfg(feature = "oracle")]
mod loot_oracle;
#[cfg(feature = "oracle")]
mod machine;
#[cfg(feature = "oracle")]
mod map_oracle;
#[cfg(feature = "oracle")]
mod menu;
#[cfg(feature = "oracle")]
mod mob_oracle;
#[cfg(feature = "oracle")]
mod move_oracle;
#[cfg(feature = "oracle")]
mod natural_spawn;
#[cfg(feature = "probe-online")]
mod online;
#[cfg(feature = "oracle")]
mod oracle;
#[cfg(feature = "oracle")]
mod path_type_oracle;
#[cfg(feature = "oracle")]
mod player_attributes_oracle;
#[cfg(feature = "oracle")]
mod player_data_oracle;
#[cfg(feature = "oracle")]
mod poi_oracle;
#[cfg(feature = "oracle")]
mod post_process_oracle;
#[cfg(feature = "oracle")]
mod predicate_oracle;
#[cfg(feature = "oracle")]
mod recipe_oracle;
#[cfg(feature = "oracle")]
mod resume_world;
#[cfg(feature = "oracle")]
mod saved_data;
#[cfg(feature = "oracle")]
mod seed_world;
#[cfg(feature = "oracle")]
mod sleep_oracle;
#[cfg(feature = "oracle")]
mod spawn;
#[cfg(feature = "oracle")]
mod storage_oracle;
#[cfg(feature = "oracle")]
mod structure_e2e;
#[cfg(feature = "oracle")]
mod structure_generation;
#[cfg(feature = "oracle")]
mod structure_load;
#[cfg(feature = "oracle")]
mod structure_pieces;
#[cfg(feature = "oracle")]
mod structure_pipeline;
#[cfg(feature = "oracle")]
mod structure_place;
#[cfg(feature = "oracle")]
mod structure_post_process;
#[cfg(feature = "oracle")]
mod structure_rings;
#[cfg(feature = "oracle")]
mod structure_ruined_portal;
#[cfg(feature = "oracle")]
mod structure_save;
#[cfg(feature = "oracle")]
mod structure_single;
#[cfg(feature = "oracle")]
mod structure_templated;
#[cfg(feature = "oracle")]
mod structure_templated_place;
mod structure_templates;
#[cfg(feature = "oracle")]
mod structure_templates_oracle;
#[cfg(feature = "oracle")]
mod structure_validity;
#[cfg(feature = "oracle")]
mod structures;
#[cfg(feature = "oracle")]
mod ticket_oracle;
#[cfg(feature = "oracle")]
mod trade_oracle;
#[cfg(feature = "oracle")]
mod transform;
#[cfg(feature = "oracle")]
mod world_block;
#[cfg(feature = "oracle")]
mod world_border_oracle;
#[cfg(feature = "oracle")]
mod world_dir;
#[cfg(feature = "oracle")]
mod worldgen;

#[cfg(feature = "oracle")]
mod wg_perf;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const WORK_DIR: &str = "target/vanilla";
const OUTPUT_DIR: &str = "crates/mc-bamboo-data/src/generated";
const PROBE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/Probe.java";
#[cfg(feature = "oracle")]
const ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/BlockOracle.java";
#[cfg(feature = "oracle")]
const ADVANCEMENT_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/AdvancementOracle.java";
#[cfg(feature = "oracle")]
const ADVANCEMENT_TRIGGER_ORACLE_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/AdvancementTriggerOracle.java";
#[cfg(feature = "oracle")]
const LIGHT_SOURCE: &str = "tools/mc-bamboo-vanilla/java/LightOracle.java";
#[cfg(feature = "oracle")]
const JAVA_SET_SOURCE: &str = "tools/mc-bamboo-vanilla/java/JavaSetOracle.java";
#[cfg(feature = "oracle")]
const MACHINE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MachineOracle.java";
#[cfg(feature = "oracle")]
const MENU_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MenuOracle.java";
#[cfg(feature = "oracle")]
const COMMAND_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/CommandOracle.java";
#[cfg(feature = "oracle")]
const EXECUTE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ExecuteOracle.java";
#[cfg(feature = "oracle")]
const COMPONENTS_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ComponentOracle.java";
#[cfg(feature = "oracle")]
const PREDICATE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PredicateOracle.java";
#[cfg(feature = "oracle")]
const RECIPE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/RecipeOracle.java";
#[cfg(feature = "oracle")]
const WORLDGEN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/WorldgenOracle.java";
#[cfg(feature = "oracle")]
const CHUNKGEN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ChunkgenOracle.java";
#[cfg(feature = "oracle")]
const BEARD_SOURCE: &str = "tools/mc-bamboo-vanilla/java/BeardifierOracle.java";
#[cfg(feature = "oracle")]
const GEN_LIGHT_SOURCE: &str = "tools/mc-bamboo-vanilla/java/GenLightOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURES_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructurePlacementOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_RINGS_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructureRingsOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_VALIDITY_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructureValidityOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_TEMPLATES_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructureTemplateOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_PIECES_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructurePiecesOracle.java";
#[cfg(feature = "oracle")]
const JIGSAW_SOURCE: &str = "tools/mc-bamboo-vanilla/java/JigsawOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_GENERATION_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructureGenerationOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_PIPELINE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructurePipelineOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_SINGLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructureSingleOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_TEMPLATED_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructureTemplatedOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_RUINED_PORTAL_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructureRuinedPortalOracle.java";
// `StructurePlaceOracle.java` 是多件式结构件放置（矿井 / 要塞的 `postProcess`），
// `StructurePiecePlaceOracle.java` 是单件结构的 `postProcess`；两个分支给它们起的名字
// 正好互换，合并时按内容重新对应。`structure-single-place` 是
// `structure-post-process` 的历史别名，两者现在跑同一份 Java 源和同一套查询。
// `StructurePlaceOracle.java` is the multi-piece placement oracle (mineshaft / stronghold
// `postProcess`); `StructurePiecePlaceOracle.java` is the single-piece `postProcess` one.
// The two branches named them the other way round, so the merge re-pairs them by content.
// `structure-single-place` is a historical alias of `structure-post-process`: both now run
// the same Java source and the same queries.
#[cfg(feature = "oracle")]
const STRUCTURE_PLACE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructurePlaceOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_POST_PROCESS_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructurePiecePlaceOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_SAVE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructureSaveOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_LOAD_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StructureLoadOracle.java";
#[cfg(feature = "oracle")]
const TRANSFORM_SOURCE: &str = "tools/mc-bamboo-vanilla/java/TransformOracle.java";
#[cfg(feature = "oracle")]
const STRUCTURE_TEMPLATED_PLACE_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/StructureTemplatedPlaceOracle.java";
#[cfg(feature = "oracle")]
const LOOT_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/LootOracle.java";
#[cfg(feature = "oracle")]
const TRADE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/TradeOracle.java";
#[cfg(feature = "oracle")]
const ENTITY_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/EntityOracle.java";
#[cfg(feature = "oracle")]
const MOVE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MovementOracle.java";
#[cfg(feature = "oracle")]
const MOB_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MobOracle.java";
#[cfg(feature = "oracle")]
const NATURAL_SPAWN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/NaturalSpawnOracle.java";
#[cfg(feature = "oracle")]
const PATH_TYPE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PathTypeOracle.java";
#[cfg(feature = "oracle")]
const STORAGE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StorageOracle.java";
#[cfg(feature = "oracle")]
const POI_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PoiOracle.java";
#[cfg(feature = "oracle")]
const CHUNK_CONTENT_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ChunkContentOracle.java";
#[cfg(feature = "oracle")]
const TICKET_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/TicketOracle.java";
#[cfg(feature = "oracle")]
const PLAYER_ATTRIBUTES_ORACLE_SOURCE: &str =
    "tools/mc-bamboo-vanilla/java/PlayerAttributesOracle.java";
#[cfg(feature = "oracle")]
const PLAYER_DATA_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PlayerDataOracle.java";
#[cfg(feature = "oracle")]
const WORLD_BORDER_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/WorldBorderOracle.java";
#[cfg(feature = "oracle")]
const SPAWN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/SpawnOracle.java";
#[cfg(feature = "oracle")]
const FLAT_WORLD_SOURCE: &str = "tools/mc-bamboo-vanilla/java/FlatWorldOracle.java";
#[cfg(feature = "oracle")]
const CLOCK_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ClockOracle.java";
#[cfg(feature = "oracle")]
const SLEEP_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/SleepOracle.java";
const USAGE: &str = "用法：mc-bamboo-vanilla generate <server.jar> [java 路径]\n      mc-bamboo-vanilla extract [java 路径]\n      mc-bamboo-vanilla gamerules <java 路径>\n      mc-bamboo-vanilla probe <地址> <端口> [java 路径]
      mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure] [--case I] [--show N] [--show-blocked] [--rule 名字=值] [--weather clear|rain|thunder]
      mc-bamboo-vanilla light <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
      mc-bamboo-vanilla menu <java 路径> [--cases N] [--seed S] [--case I] [--show N]
      mc-bamboo-vanilla components <java 路径> [--show N]
      mc-bamboo-vanilla command-oracle <java 路径> [--show N] [--break-self-test]
      mc-bamboo-vanilla execute-oracle <java 路径> [--show N] [--break-self-test]
      mc-bamboo-vanilla predicate-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla loot-oracle <java 路径> [--entity-table ID] [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N] [--biome-points N] [--show N]
      mc-bamboo-vanilla chunkgen <java 路径> [--seeds N] [--chunks N] [--salt S] [--through STAGE] [--biomes-only] [--show N] [--chunk X Z] [--scan N]
      mc-bamboo-vanilla structures <java 路径>
      mc-bamboo-vanilla gen-world <java 路径> [--seeds N] [--chunks N] [--salt S] [--show N] [--break-self-test]
      mc-bamboo-vanilla structure-rings <java 路径>
      mc-bamboo-vanilla structure-validity <java 路径>
      mc-bamboo-vanilla structure-templates <java 路径>
      mc-bamboo-vanilla structure-pieces <java 路径>
      mc-bamboo-vanilla jigsaw <java 路径>
      mc-bamboo-vanilla spawn <java 路径> [--seeds N] [--salt S] [--show N] [--level-type normal|large_biomes|amplified] [--break-self-test]
      mc-bamboo-vanilla natural-spawn <java 路径> [--show N] [--break-self-test]
      mc-bamboo-vanilla flat-world <java 路径> [--show N] [--break-self-test]
      mc-bamboo-vanilla beard <java 路径> [--through noise|surface|carvers] [--break-self-test]
      mc-bamboo-vanilla gen-light <java 路径> [--scenes N] [--seed S] [--show N] [--break-self-test]
      mc-bamboo-vanilla light-pipeline <java 路径> [--seeds N] [--chunks N] [--salt S] [--structures] [--show N] [--break-carvers]
      mc-bamboo-vanilla wg-perf [--seeds N] [--chunks N] [--biomes a,b,c] [--repeat N] [--threads N] [--json] [--skip-cut] [--skip-sky] [--skip-threads] [--skip-aux] [--structure-timing]
      mc-bamboo-vanilla structure-generation <java 路径> [--break-self-test]
      mc-bamboo-vanilla structure-pipeline <java 路径> [--break-self-test]
      mc-bamboo-vanilla structure-e2e <java 路径> [--break-self-test] [--structure-types IDS]
      mc-bamboo-vanilla feature-merge <java 路径> [--structures] [--target SEED X Z]... [--orders O1,O2] [--threads N] [--break-drop-neighbor DX DZ | --break-drop-neighbor-marks DX DZ | --break-largest-state | --break-drop-block-entities] | --scan SEED X0 Z0 SIDE
      mc-bamboo-vanilla post-process <java 路径> [--target SEED X Z STRUCTURES]... [--break-skip-fluid | --break-keep-table | --break-send-early]
      mc-bamboo-vanilla structure-single <java 路径>
      mc-bamboo-vanilla structure-templated <java 路径>
      mc-bamboo-vanilla structure-templated-place <java 路径>
      mc-bamboo-vanilla structure-ruined-portal <java 路径>
      mc-bamboo-vanilla structure-place <java 路径>
      mc-bamboo-vanilla structure-save <java 路径>
      mc-bamboo-vanilla structure-load <java 路径>
      mc-bamboo-vanilla structure-single-place <java 路径>
          structure-post-process 的历史别名，跑同一套单件结构放置对拍
      mc-bamboo-vanilla structure-post-process <java 路径>
      mc-bamboo-vanilla transform <java 路径>
      mc-bamboo-vanilla machine <java 路径> [--break-self-test 名字]
      mc-bamboo-vanilla entity-oracle <java 路径>
      mc-bamboo-vanilla path-type <java 路径> [--break-self-test] [--show N]
      mc-bamboo-vanilla move-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla mob-oracle <java 路径> [--only 前缀] [--show N]
      mc-bamboo-vanilla poi-oracle <java 路径> [--show N] [--break-self-test NAME]
      mc-bamboo-vanilla storage <java 路径> [--show N]
      mc-bamboo-vanilla saved-data <java 路径>
      mc-bamboo-vanilla map-oracle <java 路径> [--break color|save|packet|fluid|banner-toggle|banner-check|map-extend]
      mc-bamboo-vanilla player-attributes <java 路径> [--cases N] [--mining-cases N] [--seed S] [--show N] [--break-self-test]
      mc-bamboo-vanilla player-data <java 路径> [--show N]
      mc-bamboo-vanilla chunk-content <java 路径> [--show N]
      mc-bamboo-vanilla ticket-oracle <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
      mc-bamboo-vanilla border-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla clock-oracle <java 路径> [--break-self-test] [--show N]
      mc-bamboo-vanilla sleep-oracle <java 路径> [--break-self-test] [--show N]
      mc-bamboo-vanilla world-dir <目录> <java 路径>
      mc-bamboo-vanilla world-block <目录> <x> <y> <z> <方块名> <java 路径>
      mc-bamboo-vanilla seed-world <目录> [半径]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args[..] {
        ["generate", server_jar] => generate(server_jar, "java"),
        ["generate", server_jar, java] => generate(server_jar, java),
        ["extract"] => extract("java"),
        ["extract", java] => extract(java),
        ["gamerules", java] => dump_gamerules(java),
        ["probe", host, port] => probe(host, port, "java"),
        ["probe", host, port, java] => probe(host, port, java),
        // 崩溃测试用：探针的额外两个参数是标记方块的注册名和 /setblock 命令。
        ["probe", host, port, java, marker_block, marker_command] => {
            probe_with_marker(host, port, java, marker_block, marker_command)
        }
        ["oracle", java, ref options @ ..] => oracle(java, options),
        ["advancement-oracle", java, ref options @ ..] => advancement_oracle_cli(java, options),
        ["advancement-command-oracle", java, ref options @ ..] => {
            advancement_command_oracle_cli(java, options)
        }
        ["advancement-trigger-oracle", java, ref options @ ..] => {
            advancement_trigger_oracle_cli(java, options)
        }
        ["light", java, ref options @ ..] => light(java, options),
        ["menu", java, ref options @ ..] => menu(java, options),
        ["machine", java, ref options @ ..] => machine(java, options),
        ["components", java, ref options @ ..] => components(java, options),
        ["command-oracle", java, ref options @ ..] => command_oracle_cli(java, options),
        ["execute-oracle", java, ref options @ ..] => execute_oracle_cli(java, options),
        ["predicate-oracle", java, ref options @ ..] => predicate_oracle(java, options),
        ["recipe-oracle", java, ref options @ ..] => recipe_oracle(java, options),
        ["loot-oracle", java, ref options @ ..] => loot_oracle(java, options),
        ["trade-oracle", java, ref options @ ..] => trade_oracle(java, options),
        ["worldgen", java, ref options @ ..] => worldgen(java, options),
        ["chunkgen", java, ref options @ ..] => chunkgen_cli(java, options),
        ["structures", java, ref options @ ..] => structures_cli(java, options),
        ["gen-world", java, ref options @ ..] => gen_world_cli(java, options),
        ["long-set", java, ref options @ ..] => long_set_cli(java, options),
        ["structure-e2e", java, ref options @ ..] => structure_e2e_cli(java, options),
        ["resume-world", java, ref options @ ..] => resume_world_cli(java, options),
        ["feature-merge", java, ref options @ ..] => feature_merge_cli(java, options),
        ["post-process", java, ref options @ ..] => post_process_cli(java, options),
        ["structure-rings", java, ref options @ ..] => structure_rings_cli(java, options),
        ["structure-validity", java, ref options @ ..] => structure_validity_cli(java, options),
        ["structure-templates", java, ref options @ ..] => structure_templates_cli(java, options),
        ["structure-pieces", java, ref options @ ..] => structure_pieces_cli(java, options),
        ["jigsaw", java, ref options @ ..] => jigsaw_cli(java, options),
        ["spawn", java, ref options @ ..] => spawn_cli(java, options),
        ["natural-spawn", java, ref options @ ..] => natural_spawn_cli(java, options),
        ["flat-world", java, ref options @ ..] => flat_world_cli(java, options),
        ["beard", java, ref options @ ..] => beard_cli(java, options),
        ["gen-light", java, ref options @ ..] => gen_light_cli(java, options),
        ["light-pipeline", java, ref options @ ..] => light_pipeline_cli(java, options),
        #[cfg(feature = "oracle")]
        ["wg-perf", ref options @ ..] => wg_perf::run(options),
        ["structure-generation", java, ref options @ ..] => structure_generation_cli(java, options),
        ["structure-pipeline", java, ref options @ ..] => structure_pipeline_cli(java, options),
        ["structure-single", java, ref options @ ..] => structure_single_cli(java, options),
        ["structure-templated", java, ref options @ ..] => structure_templated_cli(java, options),
        ["structure-templated-place", java, ref options @ ..] => {
            structure_templated_place_cli(java, options)
        }
        ["structure-ruined-portal", java, ref options @ ..] => {
            structure_ruined_portal_cli(java, options)
        }
        ["structure-place", java, ref options @ ..] => structure_place_cli(java, options),
        ["structure-single-place", java, ref options @ ..] => {
            structure_post_process_cli(java, options)
        }
        ["structure-save", java, ref options @ ..] => structure_save_cli(java, options),
        ["structure-load", java, ref options @ ..] => structure_load_cli(java, options),
        ["structure-post-process", java, ref options @ ..] => {
            structure_post_process_cli(java, options)
        }
        ["transform", java, ref options @ ..] => transform_cli(java, options),
        ["entity-oracle", java, ref options @ ..] => entity_oracle(java, options),
        ["move-oracle", java, ref options @ ..] => move_oracle(java, options),
        ["mob-oracle", java, ref options @ ..] => mob_oracle(java, options),
        ["path-type", java, ref options @ ..] => path_type_oracle(java, options),
        ["storage", java, ref options @ ..] => storage_oracle(java, options),
        ["poi-oracle", java, ref options @ ..] => poi_oracle_cli(java, options),
        ["saved-data", java] => saved_data_cli(java),
        ["map-oracle", java, ref options @ ..] => map_oracle_cli(java, options),
        ["player-attributes", java, ref options @ ..] => player_attributes_oracle(java, options),
        ["player-data", java, ref options @ ..] => player_data_oracle(java, options),
        ["chunk-content", java, ref options @ ..] => chunk_content_oracle(java, options),
        ["ticket-oracle", java, ref options @ ..] => ticket_oracle(java, options),
        ["border-oracle", java, ref options @ ..] => world_border_oracle(java, options),
        ["clock-oracle", java, ref options @ ..] => clock_oracle_cli(java, options),
        ["sleep-oracle", java, ref options @ ..] => sleep_oracle_cli(java, options),
        ["world-dir", dir, java] => world_dir_cli(dir, java),
        ["world-block", dir, x, y, z, block, java] => world_block_cli(dir, x, y, z, block, java),
        ["chunk-dump", dir, x, z, java] => chunk_dump_cli(dir, x, z, java),
        ["seed-world", dir] => seed_world_cli(dir, None),
        ["seed-world", dir, radius] => seed_world_cli(dir, Some(radius)),
        _ => Err(USAGE.to_owned()),
    }
}

/// 只从官方注册表导出游戏规则，写 `gamerules.rs`，不动别的生成文件。
fn dump_gamerules(java: &str) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let classpath = extract::classpath(&work_dir)?;
    let source = root.join("tools/mc-bamboo-vanilla/java/Extractor.java");
    let output = Command::new(java)
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(&source)
        .arg("--game-rules")
        .output()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "导出游戏规则失败：{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let rules: Vec<extract::GameRuleFact> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("游戏规则 JSON 解析失败：{error}"))?;
    if rules.is_empty() {
        return Err("官方没有导出任何游戏规则".to_owned());
    }
    let output_dir = root.join(OUTPUT_DIR);
    fs::create_dir_all(&output_dir).map_err(|error| format!("无法创建输出目录：{error}"))?;
    generate::write_gamerules(&rules, &output_dir)?;
    println!("已生成 {} 条游戏规则", rules.len());
    Ok(())
}

fn generate(server_jar: &str, java: &str) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    fs::create_dir_all(&work_dir).map_err(|error| format!("无法创建工作目录：{error}"))?;

    let server_jar =
        fs::canonicalize(server_jar).map_err(|error| format!("找不到 {server_jar}：{error}"))?;
    let data = extract::run(java, &server_jar, &work_dir, &root)?;

    let output_dir = root.join(OUTPUT_DIR);
    fs::create_dir_all(&output_dir).map_err(|error| format!("无法创建输出目录：{error}"))?;
    generate::write_all(&data, &output_dir)?;
    structure_templates::export(&work_dir, &output_dir)?;
    jungle_temple_ops::export(java, &work_dir, &root)?;

    println!(
        "已生成 Minecraft {} 的数据（协议 {}）",
        data.version.name, data.version.protocol
    );
    Ok(())
}

/// 用上次 `generate` 留下的报告重跑 `Extractor.java`，再写一遍生成文件。
fn extract(java: &str) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let data = extract::extract_existing(java, &work_dir, &root)?;

    let output_dir = root.join(OUTPUT_DIR);
    fs::create_dir_all(&output_dir).map_err(|error| format!("无法创建输出目录：{error}"))?;
    generate::write_all(&data, &output_dir)?;
    structure_templates::export(&work_dir, &output_dir)?;
    jungle_temple_ops::export(java, &work_dir, &root)?;

    println!(
        "已用现有的报告重新生成 Minecraft {} 的数据（协议 {}）",
        data.version.name, data.version.protocol
    );
    Ok(())
}

fn probe(host: &str, port: &str, java: &str) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let classpath = extract::classpath(&work_dir)?;

    let mut command = Command::new(java);
    command
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(root.join(PROBE_SOURCE))
        .args([host, port]);
    // 正版验证的检查要连另一个服务端；`OnlineProbe` 的析构会关掉它，
    // 所以必须把它留到 Java 进程结束。不开这个特性时 `online_probe` 返回 `()`。
    #[cfg(feature = "probe-online")]
    let _online = online_probe(&mut command)?;
    #[cfg(not(feature = "probe-online"))]
    online_probe(&mut command)?;

    let status = command
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("探针发现了问题：{status}"))
    }
}

/// 崩溃测试专用：跑完整探针，另外让官方客户端发一条 `/setblock` 留一个标记方块。
/// 探针主流程会清掉它自己放的方块，只有这一格留到 `kill -9` 之后复查。
fn probe_with_marker(
    host: &str,
    port: &str,
    java: &str,
    marker_block: &str,
    marker_command: &str,
) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let classpath = extract::classpath(&work_dir)?;
    let mut command = Command::new(java);
    command
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(root.join(PROBE_SOURCE))
        .args([host, port]);
    #[cfg(feature = "probe-online")]
    let _online = online_probe(&mut command)?;
    #[cfg(not(feature = "probe-online"))]
    online_probe(&mut command)?;
    command.args([marker_block, marker_command]);
    let status = command
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("探针发现了问题：{status}"))
    }
}

/// 在这个进程里起一个开了正版验证的服务端和一个假 session server，地址交给探针。
#[cfg(feature = "probe-online")]
fn online_probe(command: &mut Command) -> Result<online::OnlineProbe, String> {
    let probe = online::start()?;
    command.args([probe.server_address().as_str(), probe.session_url()]);
    Ok(probe)
}

#[cfg(not(feature = "probe-online"))]
fn online_probe(_command: &mut Command) -> Result<(), String> {
    println!("这个版本编译时关掉了 probe-online 功能，跳过正版验证的检查");
    Ok(())
}

#[cfg(feature = "oracle")]
fn oracle(java: &str, options: &[&str]) -> Result<(), String> {
    oracle::run(official_program(java, ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn light(java: &str, options: &[&str]) -> Result<(), String> {
    light::run(official_program(java, LIGHT_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn menu(java: &str, options: &[&str]) -> Result<(), String> {
    menu::run(official_program(java, MENU_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn machine(java: &str, options: &[&str]) -> Result<(), String> {
    machine::run(official_program(java, MACHINE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn execute_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    execute_oracle::run(official_program(java, EXECUTE_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn execute_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn advancement_trigger_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    advancement_trigger_oracle::run(
        official_program(java, ADVANCEMENT_TRIGGER_ORACLE_SOURCE)?,
        options,
    )
}

#[cfg(not(feature = "oracle"))]
fn advancement_trigger_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn advancement_command_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    advancement_command_oracle::run(official_program(java, EXECUTE_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn advancement_command_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn advancement_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    advancement_oracle::run(official_program(java, ADVANCEMENT_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn advancement_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn command_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    command_oracle::run(official_program(java, COMMAND_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn command_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn components(java: &str, options: &[&str]) -> Result<(), String> {
    components::run(official_program(java, COMPONENTS_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn predicate_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    predicate_oracle::run(official_program(java, PREDICATE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn recipe_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    recipe_oracle::run(official_program(java, RECIPE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn worldgen(java: &str, options: &[&str]) -> Result<(), String> {
    worldgen::run(official_program(java, WORLDGEN_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn chunkgen_cli(java: &str, options: &[&str]) -> Result<(), String> {
    chunkgen::run(official_program(java, CHUNKGEN_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn structures_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let worldgen_path = root.join("crates/mc-bamboo-data/src/generated/worldgen.json");
    let queries_path = root.join(WORK_DIR).join("structures_queries.txt");
    structures::run(
        official_program(java, STRUCTURES_SOURCE)?,
        &worldgen_path,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn gen_world_cli(java: &str, options: &[&str]) -> Result<(), String> {
    gen_world::run(official_program(java, CHUNKGEN_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn long_set_cli(java: &str, options: &[&str]) -> Result<(), String> {
    long_set::run(official_program(java, JAVA_SET_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn feature_merge_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("feature_merge_queries.txt");
    feature_merge::run(
        official_program(java, CHUNKGEN_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn post_process_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    post_process_oracle::run(
        official_program(java, ORACLE_SOURCE)?,
        &root.join(WORK_DIR),
        options,
    )
}

#[cfg(feature = "oracle")]
fn resume_world_cli(java: &str, options: &[&str]) -> Result<(), String> {
    resume_world::run(options, java)
}

#[cfg(feature = "oracle")]
fn structure_e2e_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_e2e_queries.txt");
    structure_e2e::run(
        official_program(java, CHUNKGEN_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_rings_cli(java: &str, options: &[&str]) -> Result<(), String> {
    structure_rings::run(official_program(java, STRUCTURE_RINGS_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn structure_validity_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let worldgen_path = root.join("crates/mc-bamboo-data/src/generated/worldgen.json");
    let queries_path = root.join(WORK_DIR).join("structure_validity_queries.txt");
    structure_validity::run(
        official_program(java, STRUCTURE_VALIDITY_SOURCE)?,
        &worldgen_path,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_pipeline_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_pipeline_queries.txt");
    structure_pipeline::run(
        official_program(java, STRUCTURE_PIPELINE_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_generation_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let worldgen_path = root.join("crates/mc-bamboo-data/src/generated/worldgen.json");
    let queries_path = root.join(WORK_DIR).join("structure_generation_queries.txt");
    structure_generation::run(
        official_program(java, STRUCTURE_GENERATION_SOURCE)?,
        &worldgen_path,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_pieces_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_pieces_queries.txt");
    structure_pieces::run(
        official_program(java, STRUCTURE_PIECES_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn jigsaw_cli(java: &str, options: &[&str]) -> Result<(), String> {
    jigsaw::run(
        official_program(java, JIGSAW_SOURCE)?,
        &workspace_root().join(WORK_DIR),
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_single_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_single_queries.txt");
    structure_single::run(
        official_program(java, STRUCTURE_SINGLE_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_templated_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let worldgen_path = root.join("crates/mc-bamboo-data/src/generated/worldgen.json");
    let queries_path = root.join(WORK_DIR).join("structure_templated_queries.txt");
    structure_templated::run(
        official_program(java, STRUCTURE_TEMPLATED_SOURCE)?,
        &worldgen_path,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_templated_place_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root
        .join(WORK_DIR)
        .join("structure_templated_place_queries.txt");
    structure_templated_place::run(
        official_program(java, STRUCTURE_TEMPLATED_PLACE_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_ruined_portal_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let worldgen_path = root.join("crates/mc-bamboo-data/src/generated/worldgen.json");
    let queries_path = root
        .join(WORK_DIR)
        .join("structure_ruined_portal_queries.txt");
    structure_ruined_portal::run(
        official_program(java, STRUCTURE_RUINED_PORTAL_SOURCE)?,
        &worldgen_path,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_place_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_place_queries.txt");
    structure_place::run(
        official_program(java, STRUCTURE_PLACE_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_save_cli(java: &str, options: &[&str]) -> Result<(), String> {
    structure_save::run(
        official_program(java, STRUCTURE_SAVE_SOURCE)?,
        java,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_load_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("structure_load_queries.txt");
    structure_load::run(
        official_program(java, STRUCTURE_LOAD_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_post_process_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root
        .join(WORK_DIR)
        .join("structure_post_process_queries.txt");
    structure_post_process::run(
        official_program(java, STRUCTURE_POST_PROCESS_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn transform_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("transform_queries.txt");
    transform::run(
        official_program(java, TRANSFORM_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn structure_templates_cli(java: &str, options: &[&str]) -> Result<(), String> {
    structure_templates_oracle::run(
        official_program(java, STRUCTURE_TEMPLATES_SOURCE)?,
        &workspace_root().join(WORK_DIR),
        options,
    )
}

#[cfg(feature = "oracle")]
fn spawn_cli(java: &str, options: &[&str]) -> Result<(), String> {
    spawn::run(official_program(java, SPAWN_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn natural_spawn_cli(java: &str, options: &[&str]) -> Result<(), String> {
    natural_spawn::run(official_program(java, NATURAL_SPAWN_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn flat_world_cli(java: &str, options: &[&str]) -> Result<(), String> {
    flat_world::run(official_program(java, FLAT_WORLD_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn beard_cli(java: &str, options: &[&str]) -> Result<(), String> {
    beard::run(official_program(java, BEARD_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn gen_light_cli(java: &str, options: &[&str]) -> Result<(), String> {
    gen_light::run(official_program(java, GEN_LIGHT_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn light_pipeline_cli(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let queries_path = root.join(WORK_DIR).join("light_pipeline_queries.txt");
    light_pipeline::run(
        official_program(java, CHUNKGEN_SOURCE)?,
        &queries_path,
        options,
    )
}

#[cfg(feature = "oracle")]
fn loot_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    loot_oracle::run(official_program(java, LOOT_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn trade_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    trade_oracle::run(official_program(java, TRADE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn entity_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    let mut break_name = None;
    let mut i = 0;
    while i < options.len() {
        match options[i] {
            "--break-self-test" => {
                i += 1;
                break_name = Some(
                    options
                        .get(i)
                        .copied()
                        .ok_or("--break-self-test needs a name")?,
                );
            }
            other => return Err(format!("unknown entity-oracle option {other}")),
        }
        i += 1;
    }
    entity_oracle::run(official_program(java, ENTITY_ORACLE_SOURCE)?, break_name)
}

#[cfg(feature = "oracle")]
fn path_type_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    path_type_oracle::run(official_program(java, PATH_TYPE_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn path_type_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn mob_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    mob_oracle::run(official_program(java, MOB_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn mob_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn move_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    move_oracle::run(official_program(java, MOVE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn poi_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    poi_oracle::run(official_program(java, POI_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn storage_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    storage_oracle::run(official_program(java, STORAGE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn chunk_content_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    chunk_content_oracle::run(
        official_program(java, CHUNK_CONTENT_ORACLE_SOURCE)?,
        options,
    )
}

#[cfg(not(feature = "oracle"))]
fn storage_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn chunk_content_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn player_attributes_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    player_attributes_oracle::run(
        official_program(java, PLAYER_ATTRIBUTES_ORACLE_SOURCE)?,
        options,
    )
}

#[cfg(not(feature = "oracle"))]
fn player_attributes_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn player_data_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    player_data_oracle::run(official_program(java, PLAYER_DATA_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn player_data_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn ticket_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    ticket_oracle::run(official_program(java, TICKET_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn world_border_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    world_border_oracle::run(official_program(java, WORLD_BORDER_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn world_border_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn clock_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    clock_oracle::run(official_program(java, CLOCK_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn clock_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn poi_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn sleep_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    sleep_oracle::run(official_program(java, SLEEP_ORACLE_SOURCE)?, options)
}

#[cfg(not(feature = "oracle"))]
fn sleep_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

/// 用官方解压出来的 jar 当 classpath，运行一个单文件 Java 程序。
#[cfg(feature = "oracle")]
fn official_program(java: &str, source: &str) -> Result<Command, String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let classpath = extract::classpath(&work_dir)?;

    let mut command = Command::new(java);
    command
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(root.join(source));
    Ok(command)
}

#[cfg(feature = "oracle")]
fn saved_data_cli(java: &str) -> Result<(), String> {
    saved_data::run(java)
}

#[cfg(feature = "oracle")]
fn map_oracle_cli(java: &str, options: &[&str]) -> Result<(), String> {
    map_oracle::run(java, options)
}

#[cfg(not(feature = "oracle"))]
fn map_oracle_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn saved_data_cli(_java: &str) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn world_dir_cli(dir: &str, java: &str) -> Result<(), String> {
    world_dir::run(dir, java)
}

#[cfg(feature = "oracle")]
fn seed_world_cli(dir: &str, radius: Option<&str>) -> Result<(), String> {
    seed_world::run(dir, radius)
}

#[cfg(not(feature = "oracle"))]
fn seed_world_cli(_dir: &str, _radius: Option<&str>) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn world_dir_cli(_dir: &str, _java: &str) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn world_block_cli(
    dir: &str,
    x: &str,
    y: &str,
    z: &str,
    block: &str,
    java: &str,
) -> Result<(), String> {
    world_block::run(dir, x, y, z, block, java)
}

#[cfg(feature = "oracle")]
fn chunk_dump_cli(dir: &str, x: &str, z: &str, java: &str) -> Result<(), String> {
    let chunk = mc_bamboo_core::ChunkPos::new(
        x.parse().map_err(|_| format!("bad x {x:?}"))?,
        z.parse().map_err(|_| format!("bad z {z:?}"))?,
    );
    let path = fs::canonicalize(dir).map_err(|error| format!("cannot resolve {dir:?}: {error}"))?;
    let region = path
        .join("dimensions/minecraft/overworld/region")
        .join(format!("r.{}.{}.mca", chunk.x >> 5, chunk.z >> 5));
    let text = world_block::dump_chunk(java, &region, chunk)?;
    println!("CHUNK DUMP {chunk:?}: {text}");
    Ok(())
}

#[cfg(not(feature = "oracle"))]
fn chunk_dump_cli(_dir: &str, _x: &str, _z: &str, _java: &str) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn world_block_cli(
    _dir: &str,
    _x: &str,
    _y: &str,
    _z: &str,
    _block: &str,
    _java: &str,
) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn light(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn menu(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn machine(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn components(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn predicate_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn recipe_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn worldgen(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn chunkgen_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structures_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn gen_world_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn long_set_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn feature_merge_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn post_process_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn resume_world_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_e2e_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_rings_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn spawn_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn natural_spawn_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_validity_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_templates_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_generation_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_pipeline_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_pieces_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn jigsaw_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn flat_world_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_single_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn beard_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_templated_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_templated_place_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_ruined_portal_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_place_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_save_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_load_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn structure_post_process_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn transform_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn gen_light_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn light_pipeline_cli(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn loot_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn trade_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn entity_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn move_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn ticket_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

/// 工具位于 `tools/mc-bamboo-vanilla`，往上两级就是工作区根目录。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("工具应该位于工作区的 tools 目录下")
        .to_path_buf()
}
