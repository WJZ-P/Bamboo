//! `/version` 里那些固定字段和官方 jar 里的 `version.json` 逐个对照。

use std::{fs, path::PathBuf, process::Command};

use mc_bamboo_data::version::DATA_VERSION;
use mc_bamboo_game::admin::{BUILD_TIME, PACK_DATA, PACK_RESOURCE, SERIES};
use mc_bamboo_protocol::{GAME_VERSION, PROTOCOL_VERSION};
use serde_json::Value;

#[test]
fn version_constants_match_the_official_version_json() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/vanilla");
    let jar = root.join("versions/26.3/server-26.3.jar");
    assert!(
        jar.exists(),
        "官方服务端 jar 应该已经下载：{}",
        jar.display()
    );
    // 和 `uri_oracle.rs` 一样依赖 `target/vanilla`，但这里只要读一个文本文件。
    let output = Command::new("unzip")
        .arg("-p")
        .arg(&jar)
        .arg("version.json")
        .output()
        .expect("unzip 应该能运行");
    assert!(output.status.success(), "unzip 读 version.json 失败");
    let official: Value =
        serde_json::from_slice(&output.stdout).expect("version.json 应该是合法 JSON");
    assert_eq!(official["id"].as_str(), Some(GAME_VERSION));
    assert_eq!(official["name"].as_str(), Some(GAME_VERSION));
    assert_eq!(
        official["world_version"].as_i64(),
        Some(i64::from(DATA_VERSION))
    );
    assert_eq!(
        official["protocol_version"].as_i64(),
        Some(i64::from(PROTOCOL_VERSION))
    );
    assert_eq!(official["series_id"].as_str(), Some(SERIES));
    assert_eq!(official["build_time"].as_str(), Some(BUILD_TIME));
    assert_eq!(official["stable"].as_bool(), Some(true));
    let pack = &official["pack_version"];
    assert_eq!(
        format!(
            "{}.{}",
            pack["resource_major"].as_i64().unwrap(),
            pack["resource_minor"].as_i64().unwrap()
        ),
        PACK_RESOURCE
    );
    assert_eq!(
        format!(
            "{}.{}",
            pack["data_major"].as_i64().unwrap(),
            pack["data_minor"].as_i64().unwrap()
        ),
        PACK_DATA
    );
    let _ = fs::metadata(&root).expect("target/vanilla 应该存在");
}
