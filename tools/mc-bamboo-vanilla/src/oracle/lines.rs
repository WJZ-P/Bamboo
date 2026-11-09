//! `BlockOracle.java` 写到 stdout 的行，字段的含义以它顶部的注释为准。

use std::collections::BTreeMap;

use mc_bamboo_core::{BlockPos, Direction, Hand};
use serde::{Deserialize, Deserializer, de::Error};

/// 一行输出。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Line {
    Header(Header),
    Case(Box<Case>),
    Skip(Skip),
    End(End),
    /// 生存模式挖掉一块，比较掉落和 `level.random` 的种子。
    ///
    /// One survival mining check. Compares the drops and the `level.random` seed.
    Mine(Mine),
}

/// 生存挖掘的一题。30 万道创造模式题不走这里。
///
/// One survival-mining case. The 300k creative cases do not use this line.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mine {
    pub block: String,
    pub state: u32,
    pub tool: String,
    pub seed: i64,
    pub drops: Vec<MineDrop>,
    pub random_seed: i64,
}

#[derive(Debug, Deserialize)]
pub struct MineDrop {
    pub item: String,
    pub count: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub version: String,
    pub protocol: i32,
    pub seed: i64,
    pub max_chained_updates: u32,
    pub game_time: i64,
    pub min_y: i32,
    pub max_y: i32,
    /// 这批题覆盖的游戏规则。空表示官方默认值。
    ///
    /// Game rules overridden for this batch. Empty means the official defaults.
    #[serde(default)]
    pub rules: BTreeMap<String, String>,
    /// 假世界的天气：`clear`、`rain` 或 `thunder`。旧的题面没有这一项时是晴天。
    ///
    /// The fake level's weather. A batch without this field is clear weather.
    #[serde(default = "default_weather")]
    pub weather: String,
}

fn default_weather() -> String {
    "clear".to_owned()
}

#[derive(Debug, Deserialize)]
pub struct End {
    pub cases: u64,
    pub skipped: u64,
}

/// 官方给不出唯一答案的题。
#[derive(Debug, Deserialize)]
pub struct Skip {
    pub kind: Kind,
    pub block: String,
    pub reason: String,
}

/// 一道题和官方的答案。
#[derive(Debug, Deserialize)]
pub struct Case {
    pub id: u64,
    /// 官方假世界的随机源的种子，重放时用同一个种子。
    pub seed: i64,
    pub block: String,
    pub scene: Vec<Cell>,
    /// 场景里容器方块的内容。列表里没有的容器是空的。
    #[serde(default)]
    pub contents: Vec<ContainerStack>,
    pub action: Action,
    pub outcome: Answer,
}

/// 题目的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Place,
    Use,
    Destroy,
    Tick,
    /// 跑一格的流体刻，再往后跑几刻。
    Flow,
    /// `use_item` 包：物品自己从眼睛看出去找目标。
    Item,
    /// 一格的随机刻。
    Random,
    /// 一个区块的随机刻，官方的 `tickChunk`。
    Chunk,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Place => "place",
            Self::Use => "use",
            Self::Destroy => "destroy",
            Self::Tick => "tick",
            Self::Flow => "flow",
            Self::Item => "item",
            Self::Random => "random",
            Self::Chunk => "chunk",
        }
    }
}

/// 一格的坐标和状态 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Cell(pub i32, pub i32, pub i32, pub u32);

impl Cell {
    pub const fn pos(self) -> BlockPos {
        BlockPos::new(self.0, self.1, self.2)
    }

    pub const fn state(self) -> u32 {
        self.3
    }
}

/// `levelEvent(2001)`：坐标、状态 ID、是否跳过发起的玩家。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Destroyed(pub i32, pub i32, pub i32, pub u32, pub bool);

/// 一次 `levelEvent`：类型、坐标、数据、是否跳过发起的玩家。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct LevelPlay(pub i32, pub i32, pub i32, pub i32, pub i32, pub bool);

/// 一次 `playSeededSound`：音效 ID、坐标、来源、音量、音调、是否跳过发起的玩家。
/// 种子不比较，只影响客户端挑哪个变体。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SoundPlay(
    pub i32,
    pub Double,
    pub Double,
    pub Double,
    pub u8,
    pub Float,
    pub Float,
    pub bool,
);
/// 容器里一个非空的格子：坐标、槽位、物品 ID、数量。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ContainerStack(pub i32, pub i32, pub i32, pub u32, pub String, pub i32);

impl ContainerStack {
    pub const fn pos(&self) -> BlockPos {
        BlockPos::new(self.0, self.1, self.2)
    }

    pub const fn slot(&self) -> u32 {
        self.3
    }

    pub fn item(&self) -> &str {
        &self.4
    }

    pub const fn count(&self) -> i32 {
        self.5
    }
}

/// 一个计划刻：坐标、`"block"` 或 `"fluid"`、注册表 ID、延迟、优先级。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tick(
    pub i32,
    pub i32,
    pub i32,
    pub String,
    pub String,
    pub i32,
    pub i32,
);

impl Tick {
    /// 官方写 -1 表示这个延迟是随机的，只比较其余部分。
    pub const RANDOM_DELAY: i32 = -1;

    pub fn is_fluid(&self) -> bool {
        self.3 == "fluid"
    }

    pub const fn pos(&self) -> BlockPos {
        BlockPos::new(self.0, self.1, self.2)
    }

    pub const fn delay(&self) -> i32 {
        self.5
    }

    pub const fn priority(&self) -> i32 {
        self.6
    }

    pub fn id(&self) -> &str {
        &self.4
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub kind: Kind,
    pub main: String,
    pub off: String,
    #[serde(deserialize_with = "hand")]
    pub hand: Hand,
    /// 这只手上物品的官方类名。
    pub item_class: String,
    pub item_use_on: bool,
    pub pos: [i32; 3],
    #[serde(default, deserialize_with = "face")]
    pub face: Option<Direction>,
    pub loc: Option<[Double; 3]>,
    pub inside: Option<bool>,
    /// 只有 flow 题有：第一个流体刻之后再跑多少刻。
    #[serde(default)]
    pub steps: u32,
    /// 只有 chunk 题有：`tickChunk` 的 `tickSpeed`。
    #[serde(default)]
    pub speed: i32,
    pub actor: [Double; 3],
    pub yaw: Float,
    pub pitch: Float,
    pub sneak: bool,
    #[serde(rename = "box")]
    pub actor_box: [Double; 6],
    pub others: Vec<[Double; 6]>,
    /// 动作做完之后再走几个游戏刻。
    pub settle: u32,
}

impl Action {
    pub const fn target(&self) -> BlockPos {
        BlockPos::new(self.pos[0], self.pos[1], self.pos[2])
    }

    /// 这次动作用的那只手上的物品。
    pub fn used_item(&self) -> &str {
        match self.hand {
            Hand::Main => &self.main,
            Hand::Off => &self.off,
        }
    }
}

/// 官方的答案。`spawned` 和 `rng` 只是记录，不读。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub result: String,
    pub changes: Vec<Cell>,
    pub destroyed: Vec<Destroyed>,
    /// 全部 `levelEvent`，包括 2001。
    #[serde(default)]
    pub level_events: Vec<LevelPlay>,
    /// 全部 `playSeededSound`。
    #[serde(default)]
    pub sounds: Vec<SoundPlay>,
    /// 结束时所有已建模容器的非空格子，按坐标和槽位排序。
    #[serde(default)]
    pub contents: Vec<ContainerStack>,
    pub ticks: Vec<Tick>,
    /// `settle` 里真的运行过 `tick` 的方块的注册表 ID。
    pub ran: Vec<String>,
    /// 改了容器内容，旁边又有比较器读得到。Bamboo 不保留容器内容，这题比不了。
    pub analog_content: bool,
    pub item_reached: bool,
    /// 物品栏里新出现的物品，只有 item 题有内容。
    #[serde(default)]
    pub gained: Vec<String>,
    /// 官方 `level.random` 跑完这题之后的内部种子。旧的题目行没有这个字段。
    ///
    /// The internal seed of official `level.random` after the case. Older lines omit it.
    #[serde(default)]
    pub random_seed: Option<i64>,
}

/// 写成 IEEE 754 位的 `double`：十进制不保证往返，这些数在边界上差一位结果就变。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Double(pub f64);

impl<'de> Deserialize<'de> for Double {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bits = u64::from_str_radix(&text, 16).map_err(D::Error::custom)?;
        Ok(Self(f64::from_bits(bits)))
    }
}

/// 写成 IEEE 754 位的 `float`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Float(pub f32);

impl<'de> Deserialize<'de> for Float {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bits = u32::from_str_radix(&text, 16).map_err(D::Error::custom)?;
        Ok(Self(f32::from_bits(bits)))
    }
}

fn hand<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Hand, D::Error> {
    match String::deserialize(deserializer)?.as_str() {
        "main" => Ok(Hand::Main),
        "off" => Ok(Hand::Off),
        other => Err(D::Error::custom(format!("unknown hand {other}"))),
    }
}

fn face<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Direction>, D::Error> {
    let name = String::deserialize(deserializer)?;
    Direction::ALL
        .into_iter()
        .find(|direction| direction.name() == name)
        .map(Some)
        .ok_or_else(|| D::Error::custom(format!("unknown face {name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASE: &str = r#"{"type":"case","id":3,"seed":34,"block":"minecraft:stone","class":"Block",
        "scene":[[1,64,2,1]],
        "contents":[[1,64,2,0,"minecraft:stone",1]],
        "action":{"kind":"place","main":"minecraft:stone","off":"minecraft:air","hand":"off",
            "itemClass":"BlockItem","itemUseOn":false,"pos":[1,64,2],"face":"up",
            "loc":["3ff8000000000000","4050400000000000","4004000000000000"],"inside":false,
            "actor":["0000000000000000","4050000000000000","0000000000000000"],
            "yaw":"c2340001","pitch":"00000000","sneak":true,
            "box":["bfd3333333333333","4050000000000000","bfd3333333333333",
                   "3fd3333333333333","4051000000000000","3fd3333333333333"],"others":[],"settle":3},
        "outcome":{"result":"success","changes":[[1,65,2,1]],"destroyed":[[0,1,2,3,true]],
            "levelEvents":[[1505,1,2,3,15,false],[2001,0,1,2,3,true]],
            "sounds":[[157,"3ff8000000000000","4050400000000000","4004000000000000",4,
                       "3f800000","3f000000",true]],
            "contents":[[1,64,2,0,"minecraft:stone",2]],
            "ticks":[[1,2,3,"fluid","minecraft:water",5,0]],"ran":["minecraft:repeater"],
            "analogContent":true,"itemReached":true,"spawned":[],"rng":false}}"#;

    #[test]
    fn a_case_line_parses_with_exact_floats() {
        let Line::Case(case) = serde_json::from_str(CASE).expect("这一行符合格式") else {
            panic!("这是 case 行");
        };
        assert_eq!(case.id, 3);
        assert_eq!(case.scene, [Cell(1, 64, 2, 1)]);
        assert_eq!(
            case.contents,
            [ContainerStack(1, 64, 2, 0, "minecraft:stone".to_owned(), 1)]
        );
        assert_eq!(case.action.kind, Kind::Place);
        assert_eq!(case.action.hand, Hand::Off);
        assert_eq!(case.action.used_item(), "minecraft:air");
        assert_eq!(case.action.face, Some(Direction::Up));
        assert_eq!(case.action.loc.expect("place 有点击点")[0].0, 1.5);
        // -45.000004 是 -45 往外一位的 float，走十进制很容易丢掉。
        assert_eq!(case.action.yaw.0.to_bits(), 0xc234_0001);
        assert_eq!(case.action.settle, 3);
        assert!(case.outcome.analog_content);
        assert!(case.outcome.item_reached);
        assert!(case.outcome.ticks[0].is_fluid());
        assert_eq!(case.outcome.ran, ["minecraft:repeater"]);
        assert_eq!(case.outcome.destroyed, [Destroyed(0, 1, 2, 3, true)]);
        assert_eq!(
            case.outcome.level_events,
            [
                LevelPlay(1505, 1, 2, 3, 15, false),
                LevelPlay(2001, 0, 1, 2, 3, true)
            ]
        );
        let sound = case.outcome.sounds[0];
        assert_eq!((sound.0, sound.4), (157, 4));
        assert_eq!((sound.1.0, sound.2.0, sound.3.0), (1.5, 65.0, 2.5));
        assert_eq!((sound.5.0, sound.6.0), (1.0, 0.5));
        assert!(sound.7);
    }

    #[test]
    fn a_destroy_case_has_no_hit() {
        let line = CASE
            .replace(r#""kind":"place""#, r#""kind":"destroy""#)
            .replace(r#""face":"up","#, "")
            .replace(
                r#""loc":["3ff8000000000000","4050400000000000","4004000000000000"],"inside":false,"#,
                "",
            );
        let line = line.replace(r#""contents":[[1,64,2,0,"minecraft:stone",1]],"#, "");
        let Line::Case(case) = serde_json::from_str(&line).expect("这一行符合格式") else {
            panic!("这是 case 行");
        };
        assert_eq!(case.action.kind, Kind::Destroy);
        assert_eq!(case.action.face, None);
        assert_eq!(case.action.loc, None);
        // 没有 contents 的旧格式行也读得进来。
        assert!(case.contents.is_empty());
    }

    #[test]
    fn flow_and_item_cases_carry_their_extra_fields() {
        let flow = CASE
            .replace(r#""kind":"place""#, r#""kind":"flow""#)
            .replace(r#""inside":false,"#, r#""inside":false,"steps":40,"#);
        let Line::Case(case) = serde_json::from_str(&flow).expect("这一行符合格式") else {
            panic!("这是 case 行");
        };
        assert_eq!(case.action.kind, Kind::Flow);
        assert_eq!(case.action.steps, 40);
        assert!(case.outcome.gained.is_empty());

        let item = CASE
            .replace(r#""kind":"place""#, r#""kind":"item""#)
            .replace(
                r#""itemReached":true"#,
                r#""itemReached":false,"gained":["minecraft:water_bucket"]"#,
            );
        let Line::Case(case) = serde_json::from_str(&item).expect("这一行符合格式") else {
            panic!("这是 case 行");
        };
        assert_eq!(case.action.kind, Kind::Item);
        assert_eq!(case.action.steps, 0);
        assert_eq!(case.outcome.gained, ["minecraft:water_bucket"]);
    }

    #[test]
    fn floats_with_bad_digits_are_rejected() {
        assert!(serde_json::from_str::<Double>(r#""not hex""#).is_err());
        assert!(serde_json::from_str::<Float>(r#""zz""#).is_err());
    }

    #[test]
    fn the_other_lines_parse() {
        let header = r#"{"type":"header","version":"26.3","protocol":777,"seed":1,"cases":10,
            "maxChainedUpdates":1000000,"gameTime":100000,"minY":-64,"maxY":319}"#;
        let Line::Header(header) = serde_json::from_str(header).expect("这一行符合格式")
        else {
            panic!("这是 header 行");
        };
        assert_eq!((header.min_y, header.max_y), (-64, 319));

        let skip = r#"{"type":"skip","id":7,"kind":"place","block":"minecraft:kelp",
            "class":"KelpBlock","reason":"random:KelpBlock.getStateForPlacement"}"#;
        assert!(matches!(serde_json::from_str(skip), Ok(Line::Skip(_))));

        let end = r#"{"type":"end","cases":1,"skipped":2}"#;
        let Line::End(end) = serde_json::from_str(end).expect("这一行符合格式") else {
            panic!("这是 end 行");
        };
        assert_eq!((end.cases, end.skipped), (1, 2));
    }
}
