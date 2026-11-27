//! `scoreboard-oracle`：`/trigger`、统计类准则和击杀类准则的官方对拍。
//!
//! Java 侧是 `ExecuteOracle.java scoreboard`：在假世界里逐步执行一道题（官方命令，或者
//! 直接调 `ServerPlayer.awardStat` / `resetStat` / `awardKillScore` /
//! `Player.killedEntity`），输出每一步的反馈展开文本和最后整张计分板（分数、锁、
//! 有没有显示名）。Rust 侧用 `mc_bamboo_game::replay_scoreboard` 在同样的场景里重放，
//! 逐题比较这两段文本。`--break NAME` 故意改坏一处，确认比较会失败。
//!
//! `scoreboard-oracle`: the official comparison for `/trigger`, stat criteria and kill
//! criteria. The Java side is `ExecuteOracle.java scoreboard`: it runs each case step by
//! step in the fake world (official commands, or direct calls to
//! `ServerPlayer.awardStat` / `resetStat` / `awardKillScore` / `Player.killedEntity`) and
//! emits every step's decomposed feedback plus the final scoreboard (scores, locks,
//! whether a display name is set). The Rust side replays the same case with
//! `mc_bamboo_game::replay_scoreboard` and both texts are compared per case.
//! `--break NAME` corrupts one thing on purpose to confirm the comparison fails.

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str = "usage: scoreboard-oracle <java> [--show N] [--break NAME]";

/// 改坏确认：名字和对 Bamboo 这一侧输入做的改动。
///
/// Break checks: a name and the change applied to the input Bamboo replays.
const BREAKS: &[(&str, fn(&str) -> String)] = &[
    // 触发以后不重新上锁：第二次 `/trigger` 本该失败。
    // The score is unlocked again before every trigger, so a second use succeeds.
    ("trigger_relock", |line| {
        line.replace(
            " ;; trigger t ;; trigger t",
            " ;; trigger t ;; scoreboard players enable @s t ;; trigger t",
        )
    }),
    // 统计加分少记一次。
    // One stat award goes missing.
    ("stat_award", |line| line.replacen(" ;; !used bow 1", "", 1)),
    // `resetStat` 不清零。
    // `resetStat` does not zero the score.
    ("stat_reset", |line| {
        line.replace(" ;; !reset time_since_rest", "")
    }),
    // 击杀不记分。
    // A kill is not scored.
    ("kill_score", |line| line.replacen(" ;; !killscore", "", 1)),
];

/// 和 `ExecuteOracle.java` 的 `SCOREBOARD_CASES` 一致。
///
/// Must match `SCOREBOARD_CASES` in `ExecuteOracle.java`.
const SCENARIOS: &[&str] = &[
    "scoreboard objectives add t trigger ;; trigger t",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t add 5",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t set -3",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t ;; trigger t",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t add 4 ;; scoreboard players enable @s t ;; trigger t add 6",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; scoreboard players enable @s t",
    "scoreboard objectives add t trigger ;; scoreboard players set @s t 7 ;; trigger t",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; scoreboard players set @s t 7 ;; trigger t add 2",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; trigger t set 2147483647 ;; scoreboard players enable @s t ;; trigger t add 1",
    "trigger result",
    "trigger nope",
    "scoreboard objectives add t trigger ;; scoreboard objectives add u trigger ;; scoreboard players enable @s t ;; trigger u",
    "scoreboard objectives add t trigger ;; scoreboard players enable @s t ;; scoreboard players reset @s t ;; trigger t",
    "scoreboard objectives add bow minecraft.used:minecraft.bow ;; !used bow 1 ;; !used bow 1 ;; !used egg 1",
    "scoreboard objectives add a minecraft.used:minecraft.bow ;; scoreboard objectives add b minecraft.used:minecraft.bow ;; scoreboard objectives add c minecraft.used:minecraft.egg ;; !used bow 1",
    "!used bow 1 ;; scoreboard objectives add bow minecraft.used:minecraft.bow ;; !used bow 1",
    "scoreboard objectives add dealt minecraft.custom:minecraft.damage_dealt ;; !custom damage_dealt 35 ;; !custom damage_dealt 0 ;; !custom damage_dealt 7",
    "scoreboard objectives add dealt minecraft.custom:minecraft.damage_dealt ;; !custom damage_dealt 0",
    "scoreboard objectives add talk minecraft.custom:minecraft.talked_to_villager ;; scoreboard objectives add trade minecraft.custom:minecraft.traded_with_villager ;; !custom talked_to_villager 1 ;; !custom traded_with_villager 1 ;; !custom talked_to_villager 1",
    "scoreboard objectives add fall minecraft.custom:minecraft.fall_one_cm ;; !custom fall_one_cm 312 ;; !custom aviate_one_cm 40",
    "scoreboard objectives add rest minecraft.custom:minecraft.time_since_rest ;; !custom time_since_rest 9 ;; !reset time_since_rest",
    "scoreboard objectives add rest minecraft.custom:minecraft.time_since_rest ;; !reset time_since_rest",
    "scoreboard objectives add rest minecraft.custom:minecraft.time_since_rest ;; !reset time_since_death",
    "scoreboard objectives add big minecraft.custom:minecraft.damage_dealt ;; scoreboard players set @s big 2147483647 ;; !custom damage_dealt 1",
    "scoreboard objectives add all totalKillCount ;; scoreboard objectives add pk playerKillCount ;; scoreboard objectives add mobs minecraft.custom:minecraft.mob_kills ;; !killscore",
    "scoreboard objectives add all totalKillCount ;; !killscore ;; !killscore",
    "scoreboard objectives add cows minecraft.killed:minecraft.cow ;; scoreboard objectives add pigs minecraft.killed:minecraft.pig ;; !killed ;; !killed",
    "scoreboard objectives add cows minecraft.killed:minecraft.cow ;; scoreboard objectives add all totalKillCount ;; !killscore ;; !killed",
    "scoreboard objectives add red teamkill.red ;; scoreboard objectives add blue teamkill.blue ;; team add herd ;; team modify herd color red ;; team join herd @e[type=cow,limit=1,sort=nearest] ;; !killscore",
    "scoreboard objectives add red teamkill.red ;; team add herd ;; team join herd @e[type=cow,limit=1,sort=nearest] ;; !killscore",
    "scoreboard objectives add red teamkill.red ;; team add herd ;; team modify herd color red ;; team join herd @e[type=cow,limit=1,sort=furthest] ;; !killscore",
    "scoreboard objectives add bow minecraft.used:minecraft.bow ;; scoreboard objectives modify bow displayautoupdate true ;; !used bow 1",
    "scoreboard objectives add by minecraft.killed_by:minecraft.player ;; scoreboard objectives add by2 minecraft.killed_by:minecraft.zombie ;; scoreboard objectives list",
    "scoreboard objectives add t trigger ;; scoreboard players enable @e[type=cow] t ;; scoreboard players set @e[type=cow,limit=1,sort=nearest] t 3",
    "team add red ;; team join red @s ;; team join red Other ;; !teamsallow player other ;; !teamsallow other player",
    "team add red ;; team join red @s ;; team join red Other ;; team modify red friendlyFire false ;; !teamsallow player other ;; !teamsallow other player ;; !teamsallow player player",
    "team add red ;; team add blue ;; team join red @s ;; team join blue Other ;; team modify red friendlyFire false ;; team modify blue friendlyFire false ;; !teamsallow player other ;; !teamsallow other player",
    "team add red ;; team join red @s ;; team modify red friendlyFire false ;; !teamsallow player other ;; !teamsallow other player",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; team modify red friendlyFire false ;; !teamsallow player cow ;; !teamsallow cow player ;; !teamsallow player far",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule never ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule never ;; !pushable player cow ;; !pushable far cow",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify blue collisionRule never ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOtherTeams ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOtherTeams ;; !pushable player cow ;; !pushable far cow",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify blue collisionRule pushOtherTeams ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOwnTeam ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOwnTeam ;; !pushable player cow ;; !pushable far cow",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify blue collisionRule pushOwnTeam ;; !pushable player cow ;; !pushable far cow ;; !pushable cow far",
    "team add red ;; team add blue ;; team join red @s ;; team join blue @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOwnTeam ;; team modify blue collisionRule pushOtherTeams ;; !pushable player cow",
    "team add red ;; team join red @s ;; team join red @e[type=cow,limit=1,sort=nearest] ;; team modify red collisionRule pushOwnTeam ;; team leave @e[type=cow,limit=1,sort=nearest] ;; !pushable player cow",
];

struct Options {
    show: usize,
    broken: Option<fn(&str) -> String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: 8,
            broken: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--break" => {
                    let name = *args.next().ok_or(USAGE)?;
                    let (_, change) = BREAKS
                        .iter()
                        .find(|(known, _)| *known == name)
                        .ok_or_else(|| format!("unknown break {name}"))?;
                    options.broken = Some(*change);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    command.arg("scoreboard");
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ExecuteOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // 调试用：设了这个环境变量就把 Java 侧每一步抛出的异常打印出来。
    // For debugging: with this variable set, print the exceptions the Java steps threw.
    if std::env::var_os("SCOREBOARD_ORACLE_DEBUG").is_some() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;
    let mut cases = 0usize;
    let mut mismatches = Vec::new();
    for line in text
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
    {
        let value: Json =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        match value.get("type").and_then(Json::as_str) {
            Some("header") => check_version(&value)?,
            Some("scoreboard") => {
                cases += 1;
                if let Some(mismatch) = compare(&value, &options)? {
                    mismatches.push(mismatch);
                }
            }
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    for scenario in SCENARIOS {
        let quoted = serde_json::to_string(scenario).unwrap_or_default();
        if !text.contains(&format!("\"input\":{quoted}")) {
            return Err(format!("oracle did not emit scenario {scenario}"));
        }
    }
    if cases != SCENARIOS.len() {
        return Err(format!(
            "oracle emitted {cases} cases, expected {}",
            SCENARIOS.len()
        ));
    }
    for mismatch in mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if mismatches.is_empty() {
        println!("scoreboard-oracle: {cases} cases, all feedback and scores matched");
        Ok(())
    } else {
        Err(format!(
            "scoreboard-oracle: {} mismatch(es) in {cases} cases",
            mismatches.len()
        ))
    }
}

fn check_version(value: &Json) -> Result<(), String> {
    if value.get("version").and_then(Json::as_str) == Some(mc_bamboo_data::version::GAME_VERSION) {
        Ok(())
    } else {
        Err("oracle version does not match the generated data".to_owned())
    }
}

/// 比较一道题。一致时返回 `None`。
///
/// Compare one case; `None` when both sides agree.
fn compare(value: &Json, options: &Options) -> Result<Option<String>, String> {
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Json::as_str)
            .ok_or_else(|| format!("scoreboard line missing {name}"))
    };
    let input = field("input")?;
    let expected = (field("feedback")?, field("scores")?);
    let replayed = match options.broken {
        Some(change) => change(input),
        None => input.to_owned(),
    };
    let ours = mc_bamboo_game::replay_scoreboard(&replayed);
    if (ours.0.as_str(), ours.1.as_str()) == expected {
        return Ok(None);
    }
    Ok(Some(format!(
        "{input}\n  java feedback: {:?}\n  rust feedback: {:?}\n  java scores: {}\n  rust scores: {}",
        expected.0, ours.0, expected.1, ours.1
    )))
}
