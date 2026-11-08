//! 和官方服务端打交道的开发工具。
//!
//! ```text
//! mc-bamboo-vanilla generate <server.jar> [java 路径]
//!     从官方服务端生成 mc-bamboo-data 的版本数据
//! mc-bamboo-vanilla probe <地址> <端口> [java 路径]
//!     用官方网络代码连接正在运行的 Bamboo，检查它发的每个包
//! mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure]
//!                          [--case I] [--show N] [--show-blocked]
//!     让官方代码给方块行为出题和给答案，用 mc-bamboo-blocks 重放，比较两边的结果
//! mc-bamboo-vanilla light <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
//!     让官方光照引擎在随机场景里算光照，用 mc-bamboo-world 重放，逐段比较
//! mc-bamboo-vanilla menu <java 路径> [--cases N] [--seed S] [--case I] [--show N]
//!     让官方容器菜单跑随机点击，用 mc-bamboo-game 重放，逐槽、逐个 state_id 比较
//! mc-bamboo-vanilla components <java 路径> [--show N]
//!     让官方代码编码一大批物品组件，用 mc-bamboo-protocol 解码再编码，逐字节比较
//! mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
//!     让官方 RecipeManager 对大量网格和输入查配方，用 mc-bamboo-recipes 重放并比较
//! mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N]
//!                            [--biome-points N] [--show N]
//!     让官方代码算世界生成的噪声、密度函数和群系，用 mc-bamboo-worldgen 重算，浮点逐位比较
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
mod components;
mod extract;
mod generate;
#[cfg(feature = "oracle")]
mod light;
#[cfg(feature = "oracle")]
mod menu;
#[cfg(feature = "probe-online")]
mod online;
#[cfg(feature = "oracle")]
mod oracle;
#[cfg(feature = "oracle")]
mod recipe_oracle;
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
const MENU_SOURCE: &str = "tools/mc-bamboo-vanilla/java/MenuOracle.java";
#[cfg(feature = "oracle")]
const COMPONENTS_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ComponentOracle.java";
#[cfg(feature = "oracle")]
const RECIPE_ORACLE_SOURCE: &str = "tools/mc-bamboo-vanilla/java/RecipeOracle.java";
#[cfg(feature = "oracle")]
const WORLDGEN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/WorldgenOracle.java";
const USAGE: &str = "用法：mc-bamboo-vanilla generate <server.jar> [java 路径]\n      mc-bamboo-vanilla probe <地址> <端口> [java 路径]
      mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure] [--case I] [--show N] [--show-blocked]
      mc-bamboo-vanilla light <java 路径> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
      mc-bamboo-vanilla menu <java 路径> [--cases N] [--seed S] [--case I] [--show N]
      mc-bamboo-vanilla components <java 路径> [--show N]
      mc-bamboo-vanilla recipe-oracle <java 路径> [--cases N] [--seed S] [--show N]
      mc-bamboo-vanilla worldgen <java 路径> [--seeds N] [--salt S] [--points N] [--biome-points N] [--show N]";

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
        ["probe", host, port] => probe(host, port, "java"),
        ["probe", host, port, java] => probe(host, port, java),
        ["oracle", java, ref options @ ..] => oracle(java, options),
        ["light", java, ref options @ ..] => light(java, options),
        ["menu", java, ref options @ ..] => menu(java, options),
        ["components", java, ref options @ ..] => components(java, options),
        ["recipe-oracle", java, ref options @ ..] => recipe_oracle(java, options),
        ["worldgen", java, ref options @ ..] => worldgen(java, options),
        _ => Err(USAGE.to_owned()),
    }
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
fn components(java: &str, options: &[&str]) -> Result<(), String> {
    components::run(official_program(java, COMPONENTS_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn recipe_oracle(java: &str, options: &[&str]) -> Result<(), String> {
    recipe_oracle::run(official_program(java, RECIPE_ORACLE_SOURCE)?, options)
}

#[cfg(feature = "oracle")]
fn worldgen(java: &str, options: &[&str]) -> Result<(), String> {
    worldgen::run(official_program(java, WORLDGEN_SOURCE)?, options)
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
fn components(_java: &str, _options: &[&str]) -> Result<(), String> {
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

/// 工具位于 `tools/mc-bamboo-vanilla`，往上两级就是工作区根目录。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("工具应该位于工作区的 tools 目录下")
        .to_path_buf()
}
