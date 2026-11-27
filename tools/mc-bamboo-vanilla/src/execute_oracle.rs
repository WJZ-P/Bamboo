//! `execute-oracle`：整条 `/execute` 命令的副作用和反馈对拍。
//!
//! Java 侧 `ExecuteOracle.java` 在假世界里用官方 `Commands.performCommand`
//! 跑 148 条固定脚本，输出每条命令以后的方块、计分板和「翻译键 + 参数 + 子组件」
//! 展开的反馈；Rust 侧用 `mc-bamboo-game::replay_execute` 在同样的小世界里重放并逐项比较。
//! `--break-self-test` 改坏一项副作用；`--break-group NAME` 按类别改坏一个字段。
//!
//! `execute-oracle`: compare whole `/execute` commands by side effects and
//! feedback. `--break-self-test` deliberately corrupts one effect.

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str =
    "usage: execute-oracle <java> [--show N] [--break-self-test] [--break-group NAME]";

/// 一类改坏确认：名字、命中这类场景的判定、要改坏的字段下标。
type BreakGroup = (&'static str, fn(&str) -> bool, usize);

/// 每类改坏确认：把这一类的一个字段按位翻转，确认对拍会失败。
const BREAK_GROUPS: &[BreakGroup] = &[
    ("run", |line| line.contains("run setblock"), 0),
    (
        "conditions",
        |line| line.contains(" if ") || line.contains(" unless "),
        4,
    ),
    ("as", |line| line.contains(" as "), 0),
    (
        "at",
        |line| line.contains(" at ") || line.contains(" at @s"),
        1,
    ),
    ("positioned", |line| line.contains(" positioned "), 0),
    ("rotated", |line| line.contains(" rotated "), 0),
    ("facing", |line| line.contains(" facing "), 0),
    ("align", |line| line.contains(" align "), 0),
    ("anchored", |line| line.contains(" anchored "), 0),
    ("blocks", |line| line.contains(" blocks "), 0),
    ("store", |line| line.contains(" store "), 3),
    // `/effect` 的改坏组：反馈哈希（5）、效果状态哈希（6）。
    ("effect_feedback", |line| line.starts_with("effect "), 5),
    ("effect_state", |line| line.starts_with("effect give"), 6),
    (
        "effect_clear_state",
        |line| line.contains("effect clear"),
        6,
    ),
    // 条件写在链尾：反馈哈希（5）。
    // Conditions at the tail: the feedback hash (5).
    (
        "tail",
        |line| line.starts_with("execute ") && !line.contains(" run "),
        5,
    ),
    // `if predicate` 和 `if biome|dimension|loaded`：链尾方块有没有放下（0）。
    // `if predicate` and `if biome|dimension|loaded`: whether the tail block landed (0).
    (
        "predicate",
        |line| line.contains(" predicate ") && line.contains(" run "),
        0,
    ),
    (
        "world_conditions",
        |line| {
            [" biome ", " dimension ", " loaded "]
                .iter()
                .any(|word| line.contains(word))
                && line.contains(" run ")
        },
        0,
    ),
];

/// Java 和 Rust 都按这个脚本跑；每条命令从同一份干净场景开始。
const SCENARIOS: &[&str] = &[
    "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
    "execute if block 4 -60 6 stone run setblock 4 -60 6 stone",
    "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
    "execute if block 2 -60 2 stone run setblock 4 -60 6 stone",
    "execute unless block 2 -60 2 stone run setblock 4 -60 6 stone",
    "execute if entity @e[type=cow] run setblock 4 -60 6 stone",
    "execute unless entity @e[type=cow] run setblock 4 -60 6 stone",
    "execute unless entity @e[type=slime] run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s run setblock ~ ~ ~ stone",
    "execute at @e[type=cow] run setblock ~ ~ ~ stone",
    "execute as @e[type=cow,sort=nearest,limit=2] at @s run setblock ~ ~ ~ stone",
    "execute as @e[type=cow,sort=nearest,limit=1] at @s run setblock 4 -60 6 stone",
    "execute as @e[type=cow,sort=furthest,limit=1] at @s run setblock 4 -60 6 stone",
    "execute as @s at @s run setblock 4 -60 6 stone",
    "execute positioned 2 -60 2 run setblock ~ ~ ~ stone",
    "execute positioned as @e[type=cow,limit=1,sort=nearest] run setblock 4 -60 6 stone",
    "execute rotated 90 0 run setblock 4 -60 6 stone",
    "execute rotated as @s run setblock 4 -60 6 stone",
    "execute facing 4 -60 6 run setblock 4 -60 6 stone",
    "execute facing entity @e[type=cow,limit=1,sort=nearest] eyes run setblock 4 -60 6 stone",
    "execute facing entity @e[type=cow,limit=1,sort=nearest] feet run setblock 4 -60 6 stone",
    "execute align xyz run setblock 4 -60 6 stone",
    "execute anchored eyes run setblock 4 -60 6 stone",
    "execute anchored feet run setblock 4 -60 6 stone",
    "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all run setblock 4 -60 6 stone",
    "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked run setblock 4 -60 6 stone",
    "execute store result score #total result run setblock 4 -60 6 stone",
    "execute store success score #total result run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s store success score #total result run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s store result score #total result run setblock ~ ~ ~ stone",
    "execute if score #total result matches 0 run setblock 4 -60 6 stone",
    "execute unless score #total result matches 0 run setblock 4 -60 6 stone",
    "execute if score #total result = #total result run setblock 4 -60 6 stone",
    "execute if block 4 -60 6 stone store success score #total result run setblock 4 -60 6 stone",
    "execute unless block 4 -60 6 stone store result score #total result run setblock 4 -60 6 stone",
    "data merge storage minecraft:test {Value:7}",
    "data get storage minecraft:test Value",
    "data remove storage minecraft:test Value",
    "data modify storage minecraft:test Other set value 3",
    "execute if data storage minecraft:test Value run setblock 4 -60 6 stone",
    "execute unless data storage minecraft:test Missing run setblock 4 -60 6 stone",
    "execute store result storage minecraft:test Value int 1 run setblock 4 -60 6 stone",
    // `/effect give|clear` 的副作用与反馈
    "effect give @s minecraft:speed 30 1",
    "effect give @s minecraft:speed infinite 2",
    "effect give @s minecraft:speed 1000000 255",
    "effect give @s minecraft:speed 3 0 true",
    "effect give @e[type=cow] minecraft:regeneration 12 1",
    "effect give @s minecraft:instant_health 5 2",
    "effect clear @s",
    "effect clear @s minecraft:speed",
    "effect clear @e[type=cow]",
    "effect give @s minecraft:speed 30 1 ;; effect clear @s minecraft:haste",
    "effect give @s minecraft:speed 30 1 ;; effect clear @s",
    "effect give @s minecraft:speed 30 1 ;; effect clear @s minecraft:speed",
    "effect give @e[type=item] minecraft:speed 10",
    // 条件写在链尾：官方走条件节点自己的 executes
    "execute if block 4 -60 6 air",
    "execute unless block 4 -60 6 air",
    "execute if entity @e[type=cow]",
    "execute unless entity @e[type=cow]",
    "execute unless entity @e[type=slime]",
    "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all",
    "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked",
    "execute if score #total result matches 0",
    "execute unless score #total result matches 0",
    "execute as @e[type=cow] if entity @s",
    // biome / dimension / loaded / stopwatch
    "execute if biome 4 -60 6 minecraft:plains run setblock 4 -60 6 stone",
    "execute unless biome 4 -60 6 minecraft:plains run setblock 4 -60 6 stone",
    "execute if biome 4 -60 6 minecraft:desert run setblock 4 -60 6 stone",
    "execute if biome 4 -60 6 #minecraft:is_overworld run setblock 4 -60 6 stone",
    "execute if biome 4 -60 6 #minecraft:is_nether run setblock 4 -60 6 stone",
    "execute unless biome 4 -60 6 #minecraft:is_nether run setblock 4 -60 6 stone",
    "execute if biome 4 -60 6 minecraft:plains",
    "execute if biome 4 -60 6 minecraft:desert",
    "execute if dimension minecraft:overworld run setblock 4 -60 6 stone",
    "execute unless dimension minecraft:overworld run setblock 4 -60 6 stone",
    "execute if dimension minecraft:nope run setblock 4 -60 6 stone",
    "execute if dimension minecraft:overworld",
    "execute if loaded 4 -60 6 run setblock 4 -60 6 stone",
    "execute unless loaded 4 -60 6 run setblock 4 -60 6 stone",
    "execute if loaded 4 -60 6",
    "execute if stopwatch minecraft:nope 0.. run setblock 4 -60 6 stone",
    "execute if dimension minecraft:nope",
    "execute if stopwatch minecraft:nope 0..",
    "execute as @e[type=cow] run setblock 4 -60 6 stone",
    "execute as @e[type=cow] if dimension minecraft:nope run setblock 4 -60 6 stone",
    "execute as @e[type=cow] unless entity @s",
    // if predicate：引用、实体类型、效果、NBT、队伍、闪电、玩家
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{}} run setblock 4 -60 6 stone",
    "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{}}",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}}",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}}",
    "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}}",
    "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}}",
    "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}}",
    "execute if predicate {type:\"location_check\",predicate:{biomes:\"#minecraft:is_nether\"}}",
    "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"survival\"]}}}",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"minecraft:entity_type\":\"minecraft:player\"}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"cow\"}} run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"cow\"}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"#minecraft:undead\"}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{amplifier:1}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{amplifier:{min:2}}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{duration:{min:100},ambient:false,visible:true}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{duration:{max:100}}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{},\"minecraft:haste\":{}}}} run setblock ~ ~ ~ stone",
    "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{}}} run setblock ~ ~ ~ stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}} run setblock 4 -60 6 stone",
    "effect give @s minecraft:speed 30 0 true ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{speed:{visible:false}}}} run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:10.0f}\"}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:{Health:10.0f}}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:10}\"}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:11.0f}\"}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{}\"}} run setblock ~ ~ ~ stone",
    "team add red ;; team join red @s ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}} run setblock 4 -60 6 stone",
    "team add red ;; team join red @s ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"blue\"}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}} run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}} run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"minecraft:type_specific/player\":{}}} run setblock ~ ~ ~ stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"creative\"],level:0,food:{level:{min:20},saturation:{max:5.0}}}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"survival\",\"adventure\"]}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{level:{min:1}}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{food:{level:{max:19}}}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}} run setblock 4 -60 6 stone",
    // entity_scores
    "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:1,max:9}}} run setblock 4 -60 6 stone",
    "scoreboard players set @s result 5 ;; execute if predicate {type:\"minecraft:entity_scores\",entity:\"this\",scores:{result:5}} run setblock 4 -60 6 stone",
    "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:7}} run setblock 4 -60 6 stone",
    "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:6}}} run setblock 4 -60 6 stone",
    "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{missing:5}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{}} run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}} run setblock ~ ~ ~ stone",
    // location_check：流体和群系
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:water\"}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:flowing_water\"}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:[\"minecraft:lava\",\"minecraft:flowing_lava\"]}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"false\"}}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"true\"}}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{level:\"8\"}}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:water\",state:{falling:{min:\"false\",max:\"true\"}}}}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:empty\"}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"false\"}}}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"location_check\",predicate:{biomes:\"minecraft:plains\"}} run setblock 4 -60 6 stone",
    "execute if predicate {type:\"location_check\",predicate:{biomes:\"#minecraft:is_nether\"}} run setblock 4 -60 6 stone",
    "setblock 2 -60 2 stone ;; execute positioned 2 -59 2 if predicate {type:\"location_check\",offsetY:-1,predicate:{block:{blocks:\"minecraft:stone\"}}} run setblock 4 -60 6 stone",
    // if|unless items|slots（字面量槽位范围、实体访问器、链尾数值条件）
    "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone run setblock 4 -60 6 stone",
    "execute unless items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone run setblock 4 -60 6 stone",
    "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand run setblock 4 -60 6 stone",
    "execute unless slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand run setblock 4 -60 6 stone",
    "execute if slots entity @e[type=cow,limit=1,sort=nearest] armor.head run setblock 4 -60 6 stone",
    "execute if items entity @e[type=cow,limit=1,sort=nearest] container.0 minecraft:stone run setblock 4 -60 6 stone",
    "execute if slots entity @e[type=cow] weapon.mainhand run setblock 4 -60 6 stone",
    "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone",
    "execute unless items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone",
    "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand",
    "execute unless slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand",
    "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.* run setblock 4 -60 6 stone",
    "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.* minecraft:stone",
    // execute on（来源实体没有这些关系，全部空分支）
    "execute as @e[type=cow,limit=1,sort=nearest] on vehicle run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on passengers run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on owner run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on attacker run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on target run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on leasher run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on controller run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] on origin run setblock 4 -60 6 stone",
    // positioned over / summon
    "execute positioned over world_surface run setblock 4 -60 6 stone",
    "execute positioned over motion_blocking run setblock 4 -60 6 stone",
    "execute summon minecraft:cow run setblock 4 -60 6 stone",
    "execute summon minecraft:pig at @s run setblock 4 -60 6 stone",
    // location_check 的 structures：超平坦世界没有结构，两边都判 false
    "execute if predicate {type:\"location_check\",predicate:{structures:\"minecraft:stronghold\"}} run setblock 4 -60 6 stone",
    "execute unless predicate {type:\"location_check\",predicate:{structures:\"minecraft:stronghold\"}} run setblock 4 -60 6 stone",
    // 实体子谓词 minecraft:slots（牛的空主手）
    "execute as @e[type=cow,limit=1,sort=nearest] if predicate {type:\"entity_properties\",entity:\"this\",predicate:{slots:{\"weapon.mainhand\":{}}}} run setblock 4 -60 6 stone",
    "execute as @e[type=cow,limit=1,sort=nearest] if predicate {type:\"entity_properties\",entity:\"this\",predicate:{slots:{\"weapon.mainhand\":{items:\"minecraft:stone\"}}}} run setblock 4 -60 6 stone",
];

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
    break_group: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: 8,
            break_self_test: false,
            break_group: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--break-self-test" => options.break_self_test = true,
                "--break-group" => {
                    options.break_group = Some((*args.next().ok_or(USAGE)?).to_owned());
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
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
    // 设了 `EXECUTE_ORACLE_TRACE` 时把官方侧的反馈原文和异常栈原样转出来。
    // With `EXECUTE_ORACLE_TRACE` set, pass through the official side's raw feedback
    // and stack traces.
    if std::env::var_os("EXECUTE_ORACLE_TRACE").is_some() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
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
            Some("header") => {
                if value.get("version").and_then(Json::as_str)
                    != Some(mc_bamboo_data::version::GAME_VERSION)
                {
                    return Err("oracle version does not match the generated data".to_owned());
                }
            }
            Some("case") => {
                let input = value
                    .get("input")
                    .and_then(Json::as_str)
                    .ok_or("case line missing input")?;
                let expected = effects(&value)?;
                let mut ours = mc_bamboo_game::replay_execute(input);
                if options.break_self_test && cases == 0 {
                    ours[0] ^= 1;
                }
                if let Some(group) = &options.break_group
                    && let Some((_, matches, field)) =
                        BREAK_GROUPS.iter().find(|(name, _, _)| name == group)
                    && matches(input)
                {
                    // 故意改坏这一类的一个字段：正常比较会在下面报出差异。
                    ours[*field] = ours[*field].wrapping_add(1);
                }
                cases += 1;
                if ours != expected {
                    mismatches.push(format!("{input}: java {expected:?}, rust {ours:?}"));
                }
            }
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    for scenario in SCENARIOS {
        if !text.contains(&format!("\"input\":{}", quoted(scenario))) {
            return Err(format!("oracle did not emit scenario {scenario}"));
        }
    }
    for mismatch in mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if mismatches.is_empty() {
        println!("execute-oracle: {cases} commands, all side effects and feedback matched");
        Ok(())
    } else {
        Err(format!(
            "execute-oracle: {} mismatch(es) in {cases} commands",
            mismatches.len()
        ))
    }
}

fn effects(value: &Json) -> Result<[i32; 7], String> {
    let mut out = [0; 7];
    for (index, name) in [
        "stone_4_6",
        "stone_cow_a",
        "stone_cow_b",
        "score",
        "ran",
        "feedback",
        "effects",
    ]
    .iter()
    .enumerate()
    {
        out[index] = value
            .get(name)
            .and_then(Json::as_i64)
            .and_then(|number| i32::try_from(number).ok())
            .ok_or_else(|| format!("case line missing {name}"))?;
    }
    Ok(out)
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}
