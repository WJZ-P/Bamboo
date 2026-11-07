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
//! ```
//!
//! 升级游戏版本时，换一个 `server.jar` 运行 `generate`，看生成文件的差异，
//! 然后运行 `probe` 确认协议没有问题。这些命令都需要 Java 25 或更新版本，
//! `probe` 和 `oracle` 要先运行过 `generate`。
//!
//! `server.jar` 是官方下载的那个文件，不是 `target/vanilla/versions` 里解压出来的内层 jar。
//!
//! `oracle` 依赖生成的数据。生成文件坏了、这个工具编译不过时，用 `--no-default-features`
//! 编译，`generate` 不需要它们。

mod extract;
mod generate;
#[cfg(feature = "oracle")]
mod oracle;

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
const USAGE: &str = "用法：mc-bamboo-vanilla generate <server.jar> [java 路径]\n      mc-bamboo-vanilla probe <地址> <端口> [java 路径]
      mc-bamboo-vanilla oracle <java 路径> [--cases N] [--seed S] [--only <方块或类名>] [--pure] [--case I] [--show N] [--show-blocked]";

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

    let status = Command::new(java)
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(root.join(PROBE_SOURCE))
        .args([host, port])
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("探针发现了问题：{status}"))
    }
}

#[cfg(feature = "oracle")]
fn oracle(java: &str, options: &[&str]) -> Result<(), String> {
    let root = workspace_root();
    let work_dir = root.join(WORK_DIR);
    let classpath = extract::classpath(&work_dir)?;

    let mut command = Command::new(java);
    command
        .current_dir(&work_dir)
        .arg("-cp")
        .arg(classpath)
        .arg(root.join(ORACLE_SOURCE));
    oracle::run(command, options)
}

#[cfg(not(feature = "oracle"))]
fn oracle(_java: &str, _options: &[&str]) -> Result<(), String> {
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
