//! 把当前 git 提交号编进库里，崩溃报告用它标出是哪一份构建。
//!
//! 取不到 git（源码包、没装 git、不在仓库里）时写 `unknown`，不让构建失败。
//!
//! Embed the current git commit into the library so a crash report can name the
//! build. When git is unavailable (source tarball, git missing, not a repo) the
//! value is `unknown` and the build still succeeds.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    // 不写 rerun-if-changed：Cargo 于是按缺省「包内任意文件变动就重跑」，提交号跟着
    // 这份 crate 每次重编刷新；发布构建里就是编译那一刻的 HEAD。
    // No rerun-if-changed: Cargo's default reruns the script whenever any file
    // in the package changes, so the commit refreshes on each rebuild of this
    // crate; in a release build it is HEAD at compile time.
    println!("cargo:rustc-env=BAMBOO_GIT_COMMIT={commit}");
}
