//! 和官方服务端打交道的开发工具。
//!
//! ```text
//! mc-bamboo-vanilla generate <server.jar> [java 路径]
//!     从官方服务端生成 mc-bamboo-data 的版本数据
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
//! mc-bamboo-vanilla components <java 路径> [--show N]
//!     让官方代码编码一大批物品组件，用 mc-bamboo-protocol 解码再编码，逐字节比较
//! mc-bamboo-vanilla predicate-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 BlockPredicate 对随机方块状态和随机谓词出题，用 mc-bamboo-blocks 重放并比较
//! mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 RecipeManager 对大量网格和输入查配方，用 mc-bamboo-recipes 重放并比较
//! mc-bamboo-vanilla loot-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方战利品表对随机方块、工具、爆炸和箱子种子出题，用 mc-bamboo-loot 重放并比较
//! mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N]
//!                            [--biome-points N] [--show N]
//!     让官方代码算世界生成的噪声、密度函数和群系，用 mc-bamboo-worldgen 重算，浮点逐位比较
//! mc-bamboo-vanilla machine <java 路径>
//!     让官方熔炉和酿造台在假世界里逐刻跑，比较进度、物品、方块状态和世界随机源
//! mc-bamboo-vanilla entity-oracle <java 路径>
//!     让官方 `FallingBlockEntity.tick` 和 `PrimedTnt.tick` 在假世界里跑，
//!     用 Bamboo 重放，逐刻比较位置、速度和落地后的方块
//! mc-bamboo-vanilla move-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 `handleMovePlayer` 对一组移动包出题，用 Bamboo 重放，
//!     比较接受、拉回还是断开，以及最终位置和摔落距离
//! mc-bamboo-vanilla storage <java 路径> [--show N]
//!     用官方 SerializableChunkData / RegionFile / NbtIo 检查存档编解码层
//! mc-bamboo-vanilla player-data <java 路径> [--show N]
//!     用官方 ItemStackWithSlot / TagValueInput / PrimaryLevelData 检查物品、玩家数据和 level.dat
//! mc-bamboo-vanilla chunk-content <java 路径> [--show N]
//!     用官方 BlockEntity / SavedTick / Entity 检查区块上层内容的存档 NBT
//! mc-bamboo-vanilla ticket-oracle <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
//!     让官方 `DistanceManager` 在随机票操作里算区块等级，用 `TicketTable` 重放，逐步比较两套等级
//! mc-bamboo-vanilla border-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 `WorldBorder` 对随机的中心、大小和渐变出题，比对数学、逐刻推进和六个包的编码
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
mod chunk_content_oracle;
#[cfg(feature = "oracle")]
mod components;
#[cfg(feature = "oracle")]
mod entity_oracle;
mod extract;
mod generate;
#[cfg(feature = "oracle")]
mod light;
#[cfg(feature = "oracle")]
mod loot_oracle;
#[cfg(feature = "oracle")]
mod machine;
#[cfg(feature = "oracle")]
mod menu;
#[cfg(feature = "oracle")]
mod move_oracle;
#[cfg(feature = "probe-online")]
mod online;
#[cfg(feature = "oracle")]
mod oracle;
#[cfg(feature = "oracle")]
mod player_data_oracle;
#[cfg(feature = "oracle")]
mod predicate_oracle;
#[cfg(feature = "oracle")]
mod recipe_oracle;
#[cfg(feature = "oracle")]
mod saved_data;
#[cfg(feature = "oracle")]
mod storage_oracle;
#[cfg(feature = "oracle")]
mod ticket_oracle;
#[cfg(feature = "oracle")]
mod world_block;
#[cfg(feature = "oracle")]
mod world_border_oracle;
#[cfg(feature = "oracle")]
mod world_dir;
#[cfg(feature = "oracle")]
mod worldgen;

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
const LIGHT_SOURCE: &str = "tools/mc-bamboo-vanilla/java/LightOracle.java";
#[cfg(feature = "oracle")]
const MACHINE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MachineOracle.java";
#[cfg(feature = "oracle")]
const MENU_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MenuOracle.java";
#[cfg(feature = "oracle")]
const COMPONENTS_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ComponentOracle.java";
#[cfg(feature = "oracle")]
const PREDICATE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PredicateOracle.java";
#[cfg(feature = "oracle")]
const RECIPE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/RecipeOracle.java";
#[cfg(feature = "oracle")]
const WORLDGEN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/WorldgenOracle.java";
#[cfg(feature = "oracle")]
const LOOT_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/LootOracle.java";
#[cfg(feature = "oracle")]
const ENTITY_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/EntityOracle.java";
#[cfg(feature = "oracle")]
const MOVE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MovementOracle.java";
#[cfg(feature = "oracle")]
const STORAGE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/StorageOracle.java";
#[cfg(feature = "oracle")]
const CHUNK_CONTENT_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ChunkContentOracle.java";
#[cfg(feature = "oracle")]
const TICKET_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/TicketOracle.java";
#[cfg(feature = "oracle")]
const PLAYER_DATA_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/PlayerDataOracle.java";
#[cfg(feature = "oracle")]
const WORLD_BORDER_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/WorldBorderOracle.java";
const USAGE: &str = "用法：mc-bamboo-vanilla generate <server.jar> [java 路径]\n      mc-bamboo-vanilla extract [java 路径]\n      mc-bamboo-vanilla gamerules <java 路径>\n      mc-bamboo-vanilla probe <地址> <端口> [java 路径]
      mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure] [--case I] [--show N] [--show-blocked] [--rule 名字=值] [--weather clear|rain|thunder]
      mc-bamboo-vanilla light <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
      mc-bamboo-vanilla menu <java 路径> [--cases N] [--seed S] [--case I] [--show N]
      mc-bamboo-vanilla components <java 路径> [--show N]
      mc-bamboo-vanilla predicate-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla loot-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N] [--biome-points N] [--show N]
      mc-bamboo-vanilla machine <java 路径>
      mc-bamboo-vanilla entity-oracle <java 路径>
      mc-bamboo-vanilla move-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla storage <java 路径> [--show N]
      mc-bamboo-vanilla saved-data <java 路径>
      mc-bamboo-vanilla player-data <java 路径> [--show N]
      mc-bamboo-vanilla chunk-content <java 路径> [--show N]
      mc-bamboo-vanilla ticket-oracle <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
      mc-bamboo-vanilla border-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla world-dir <目录> <java 路径>
      mc-bamboo-vanilla world-block <目录> <x> <y> <z> <方块名> <java 路径>";

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
        ["light", java, ref options @ ..] => light(java, options),
        ["menu", java, ref options @ ..] => menu(java, options),
        ["machine", java] => machine(java),
        ["components", java, ref options @ ..] => components(java, options),
        ["predicate-oracle", java, ref options @ ..] => predicate_oracle(java, options),
        ["recipe-oracle", java, ref options @ ..] => recipe_oracle(java, options),
        ["loot-oracle", java, ref options @ ..] => loot_oracle(java, options),
        ["worldgen", java, ref options @ ..] => worldgen(java, options),
        ["entity-oracle", java] => entity_oracle(java),
        ["move-oracle", java, ref options @ ..] => move_oracle(java, options),
        ["storage", java, ref options @ ..] => storage_oracle(java, options),
        ["saved-data", java] => saved_data_cli(java),
        ["player-data", java, ref options @ ..] => player_data_oracle(java, options),
        ["chunk-content", java, ref options @ ..] => chunk_content_oracle(java, options),
        ["ticket-oracle", java, ref options @ ..] => ticket_oracle(java, options),
        ["border-oracle", java, ref options @ ..] => world_border_oracle(java, options),
        ["world-dir", dir, java] => world_dir_cli(dir, java),
        ["world-block", dir, x, y, z, block, java] => world_block_cli(dir, x, y, z, block, java),
        ["chunk-dump", dir, x, z, java] => chunk_dump_cli(dir, x, z, java),
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
    // 正版验证的检查要连另一个服务端，这两个服务活到探针跑完。
    let _online = online_probe(&mut command)?;

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
    let _online = online_probe(&mut command)?;
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
fn machine(java: &str) -> Result<(), String> {
    machine::run(official_program(java, MACHINE_SOURCE)?)
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
fn loot_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    loot_oracle::run(official_program(java, LOOT_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn entity_oracle(java: &str) -> Result<(), String> {
    entity_oracle::run(official_program(java, ENTITY_ORACLE_SOURCE)?)
}

#[cfg(feature = "oracle")]
fn move_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    move_oracle::run(official_program(java, MOVE_ORACLE_SOURCE)?, options)
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

#[cfg(not(feature = "oracle"))]
fn saved_data_cli(_java: &str) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(feature = "oracle")]
fn world_dir_cli(dir: &str, java: &str) -> Result<(), String> {
    world_dir::run(dir, java)
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
fn machine(_java: &str) -> Result<(), String> {
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
fn loot_oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
    Err("这个版本编译时关掉了 oracle 功能".to_owned())
}

#[cfg(not(feature = "oracle"))]
fn entity_oracle(_java: &str) -> Result<(), String> {
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
