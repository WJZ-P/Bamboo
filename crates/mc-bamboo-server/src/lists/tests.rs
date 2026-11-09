//! 名单文件的读写格式，全部落在临时目录里。

use super::*;
use mc_bamboo_game::admin::{AdminChanges, ban_date};
use uuid::Uuid;

const STEVE: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
const ALEX: &str = "853c80ef-3c37-49fd-aa49-938b674adae6";

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("bamboo-lists-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).expect("临时目录应该能建");
    path
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_files_are_empty_lists() {
    let dir = temp_dir();
    let _cleanup = Cleanup(dir.clone());
    assert_eq!(ListDirectory::at(&dir).load(), AdminSnapshot::default());
}

#[test]
fn startup_reads_all_four_official_formats() {
    let dir = temp_dir();
    let _cleanup = Cleanup(dir.clone());
    fs::write(
        dir.join(OPS_FILE),
        format!(r#"[{{"uuid":"{STEVE}","name":"Steve","level":4,"bypassesPlayerLimit":true}}]"#),
    )
    .unwrap();
    fs::write(
        dir.join(PLAYER_BANS_FILE),
        format!(
            r#"[{{"uuid":"{ALEX}","name":"Alex","created":"2026-09-15 11:20:48 +0000","source":"Steve","expires":"forever","reason":"spam"}}]"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join(IP_BANS_FILE),
        r#"[{"ip":"127.0.0.1","created":"2026-09-15 11:20:48 +0000","source":"(Unknown)","expires":"forever","reason":null}]"#,
    )
    .unwrap();
    fs::write(
        dir.join(WHITELIST_FILE),
        format!(r#"[{{"uuid":"{STEVE}","name":"Steve"}}]"#),
    )
    .unwrap();

    let snapshot = ListDirectory::at(&dir).load();
    assert_eq!(
        snapshot.ops[&Uuid::parse_str(STEVE).unwrap()].entry.level,
        4
    );
    assert!(
        snapshot.ops[&Uuid::parse_str(STEVE).unwrap()]
            .entry
            .bypasses_player_limit
    );
    let alex = Uuid::parse_str(ALEX).unwrap();
    assert_eq!(snapshot.player_bans[&alex].entry.source, "Steve");
    assert_eq!(
        snapshot.player_bans[&alex].entry.reason.as_deref(),
        Some("spam")
    );
    assert_eq!(snapshot.player_bans[&alex].entry.expires, None);
    assert_eq!(snapshot.ip_bans["127.0.0.1"].entry.source, "(Unknown)");
    assert_eq!(
        snapshot.whitelist[&Uuid::parse_str(STEVE).unwrap()].name,
        "Steve"
    );
}

#[test]
fn writing_ops_uses_the_official_field_order() {
    let dir = temp_dir();
    let _cleanup = Cleanup(dir.clone());
    let mut snapshot = AdminSnapshot::default();
    let uuid = Uuid::parse_str(STEVE).unwrap();
    snapshot.ops.insert(
        uuid,
        OpListEntry {
            name: "Steve".to_owned(),
            entry: mc_bamboo_game::admin::OpEntry {
                level: 4,
                bypasses_player_limit: false,
            },
        },
    );
    let lists = ListDirectory::at(&dir);
    lists.save(
        &snapshot,
        AdminChanges {
            ops: true,
            ..Default::default()
        },
    );
    let text = fs::read_to_string(dir.join(OPS_FILE)).unwrap();
    assert_eq!(
        text,
        format!(
            "[\n  {{\n    \"uuid\": \"{STEVE}\",\n    \"name\": \"Steve\",\n    \"level\": 4,\n    \"bypassesPlayerLimit\": false\n  }}\n]\n"
        )
    );
    // 写回去还能读回来。
    assert_eq!(lists.load(), snapshot);
}

#[test]
fn banned_players_round_trip_with_forever_and_reason() {
    let dir = temp_dir();
    let _cleanup = Cleanup(dir.clone());
    let uuid = Uuid::parse_str(ALEX).unwrap();
    let mut snapshot = AdminSnapshot::default();
    snapshot.player_bans.insert(
        uuid,
        BanListEntry {
            name: "Alex".to_owned(),
            entry: BanEntry {
                created: "2026-09-15 11:20:48 +0000".to_owned(),
                source: "Steve".to_owned(),
                expires: None,
                expires_millis: None,
                reason: Some("spam".to_owned()),
            },
        },
    );
    let lists = ListDirectory::at(&dir);
    lists.save(
        &snapshot,
        AdminChanges {
            player_bans: true,
            ..Default::default()
        },
    );
    let text = fs::read_to_string(dir.join(PLAYER_BANS_FILE)).unwrap();
    assert!(text.contains("\"expires\": \"forever\""));
    assert!(text.contains("\"reason\": \"spam\""));
    assert_eq!(lists.load().player_bans, snapshot.player_bans);
}

#[test]
fn ban_dates_parse_and_format_like_the_official_pattern() {
    let text = "2026-09-15 11:20:48 +0000";
    let millis = parse_ban_date(text).expect("官方日期格式应该能解析");
    assert_eq!(ban_date(millis), text);
    assert_eq!(parse_ban_date("forever"), None);
    assert_eq!(
        parse_ban_date("2026-09-15 11:20:48 -0500"),
        Some(millis + 5 * 3_600_000)
    );
}
