//! 官方 26.3 的进度树、显示排版和玩家进度的纯数据结构。
//!
//! 这里没有玩家、世界、岛和文件 IO：全服共享的只读树由 [`AdvancementTree::global`]
//! 提供，单个玩家的完成状态在 [`AdvancementProgress`]，存档 JSON 的编解码在
//! [`progress_file`]。运行时监听、触发器、奖励和网络包由 `mc-bamboo-game` 接进来。
//!
//! The pure data model for vanilla 26.3 advancements: the shared read-only tree,
//! display layout and per-player progress. No world state or IO lives here.

pub mod layout;
pub mod progress_file;

use std::{collections::BTreeMap, sync::OnceLock};

use mc_bamboo_protocol::{item::ItemStack, text::Component};
use serde_json::Value;

/// 显示边框，官方 `AdvancementType` 的注册表顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frame {
    #[default]
    Task,
    Challenge,
    Goal,
}

impl Frame {
    /// 数据包里 `frame` 字段的值；缺省是 `task`。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "task" => Some(Self::Task),
            "challenge" => Some(Self::Challenge),
            "goal" => Some(Self::Goal),
            _ => None,
        }
    }

    /// 官方 `AdvancementType.getSerializedName`。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Challenge => "challenge",
            Self::Goal => "goal",
        }
    }

    /// 公告用的翻译键，官方 `createAnnouncement` 拼出来的串。
    pub const fn announcement_key(self) -> &'static str {
        match self {
            Self::Task => "chat.type.advancement.task",
            Self::Challenge => "chat.type.advancement.challenge",
            Self::Goal => "chat.type.advancement.goal",
        }
    }
}

/// 一个显示节点，官方 `DisplayInfo`。
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    /// 图标的物品注册表 ID。
    pub icon: i32,
    /// 图标上的组件补丁，线上按 `ItemStackTemplate` 写。
    pub icon_components: Vec<(i32, Vec<u8>)>,
    pub title: Component,
    pub description: Component,
    /// 根进度的背景贴图，官方只在根上给。
    pub background: Option<String>,
    pub frame: Frame,
    pub show_toast: bool,
    pub announce_to_chat: bool,
    pub hidden: bool,
}

impl Display {
    /// 官方 `DisplayInfo.serializeToNetwork` 的 flags 位。
    pub const fn flags(&self) -> i32 {
        (self.background.is_some() as i32)
            | ((self.show_toast as i32) << 1)
            | ((self.hidden as i32) << 2)
    }

    /// 写线上格式，顺序照官方：标题、描述、图标、边框、flags、背景。
    pub fn write(&self, out: &mut Vec<u8>) {
        use mc_bamboo_protocol::codec::{write_i32, write_string, write_varint};
        self.title.write(out);
        self.description.write(out);
        // `ItemStackTemplate.STREAM_CODEC`：物品 ID、数量、组件补丁。
        write_varint(out, self.icon);
        write_varint(out, 1);
        write_varint(out, self.icon_components.len() as i32);
        write_varint(out, 0);
        for (kind, value) in &self.icon_components {
            write_varint(out, *kind);
            out.extend_from_slice(value);
        }
        // `AdvancementType.STREAM_CODEC` 是 VarInt 的枚举序号。
        write_varint(
            out,
            match self.frame {
                Frame::Task => 0,
                Frame::Challenge => 1,
                Frame::Goal => 2,
            },
        );
        write_i32(out, self.flags());
        if let Some(background) = &self.background {
            write_string(out, background);
        }
    }

    /// 图标按官方 `ItemStackTemplate` 的写法（供对拍和奖励图标用）。
    pub fn icon_stack(&self) -> ItemStack {
        let mut stack = ItemStack::new(self.icon, 1);
        stack.added = self.icon_components.clone();
        stack
    }
}

/// 一个进度的奖励定义，官方 `AdvancementRewards`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rewards {
    pub experience: i32,
    /// 战利品表的完整标识符。
    pub loot: Vec<String>,
    /// 解锁的配方标识符。
    pub recipes: Vec<String>,
    /// 要执行的函数。Bamboo 没有函数系统，见文档已知差异。
    pub function: Option<String>,
}

/// 一个判据，官方 `Criterion`。条件原文留给 `mc-bamboo-game` 的触发器层判定。
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    /// `minecraft:trigger_type` 注册表里的完整名字。
    pub trigger: String,
    /// 数据包里的 `conditions` 原文。
    pub conditions: Value,
}

/// 一个进度定义。
#[derive(Debug, Clone, PartialEq)]
pub struct Advancement {
    pub parent: Option<String>,
    pub display: Option<Display>,
    pub rewards: Rewards,
    /// 判据按数据包键序保存。
    pub criteria: Vec<(String, Criterion)>,
    /// 与 / 或：外层必须全部满足，内层任意一个满足即可。
    pub requirements: Vec<Vec<String>>,
    pub sends_telemetry_event: bool,
}

impl Advancement {
    /// 官方 `Advancement.isRoot`：没有父进度而且带显示。
    pub const fn is_root(&self) -> bool {
        self.parent.is_none() && self.display.is_some()
    }

    /// 全部判据名（requirements 的集合），顺序按数据包。
    pub fn criterion_names(&self) -> Vec<&str> {
        self.criteria
            .iter()
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// 写 `Advancement.STREAM_CODEC`：可选父、可选显示、requirements、遥测布尔。
    /// 判据和奖励不进网络包，客户端只需要这些。
    pub fn write_network(&self, out: &mut Vec<u8>) {
        use mc_bamboo_protocol::codec::{write_bool, write_string, write_varint};
        write_bool(out, self.parent.is_some());
        if let Some(parent) = &self.parent {
            write_string(out, parent);
        }
        write_bool(out, self.display.is_some());
        if let Some(display) = &self.display {
            display.write(out);
        }
        write_varint(out, self.requirements.len() as i32);
        for group in &self.requirements {
            write_varint(out, group.len() as i32);
            for name in group {
                write_string(out, name);
            }
        }
        write_bool(out, self.sends_telemetry_event);
    }

    /// [`Self::write_network`] 的便捷入口。
    pub fn write_network_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_network(&mut out);
        out
    }
}

/// 树上的一个节点，官方 `AdvancementNode`。
#[derive(Debug, Clone)]
pub struct Node {
    pub id: &'static str,
    pub advancement: Advancement,
    pub parent: Option<usize>,
    /// 子节点的下标，顺序由 [`TreeOrder`] 决定。
    pub children: Vec<usize>,
    /// `TreeNodePosition.run` 算出来的坐标。
    pub x: f32,
    pub y: f32,
}

/// 兄弟节点的顺序。官方用身份哈希，每次启动都可能不同；Bamboo 正式运行按 id 排序。
/// 对拍时可以把当次官方的顺序喂进来验证排版算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeOrder {
    /// 按完整标识符字典序，正式运行用这个。
    ById,
    /// 由调用方给出每个节点的子节点顺序（下标是节点下标）。
    External,
}

/// 全服共享的只读进度树，对应官方 `ServerAdvancementManager` + `AdvancementTree`。
#[derive(Debug, Clone)]
pub struct AdvancementTree {
    pub triggers: Vec<&'static str>,
    pub nodes: Vec<Node>,
    by_id: BTreeMap<&'static str, usize>,
    /// 有显示的根，按 [`TreeOrder`] 排好。
    pub roots: Vec<usize>,
    /// 所有带显示的节点，按 id 排序，发包时按这个顺序取。
    pub displayed: Vec<usize>,
}

/// 数据解析失败的原因。导出数据是官方的，坏数据只报错不 panic。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeError(pub String);

impl std::fmt::Display for TreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TreeError {}

fn bad(message: impl Into<String>) -> TreeError {
    TreeError(message.into())
}

fn field<'a>(object: &'a serde_json::Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).filter(|value| !value.is_null())
}

fn str_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<&'a str, TreeError> {
    field(object, name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{what} 的 {name} 不是字符串")))
}

fn object_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, TreeError> {
    field(object, name)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| bad(format!("{what} 的 {name} 不是对象")))
        })
        .transpose()
}

fn string_list(value: &Value, what: &str) -> Result<Vec<String>, TreeError> {
    let values = match value {
        Value::String(text) => return Ok(vec![text.clone()]),
        Value::Array(values) => values.as_slice(),
        _ => return Err(bad(format!("{what} 不是字符串或字符串数组"))),
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("{what} 里有非字符串")))
        })
        .collect()
}

/// 把 JSON 的图标组件补丁编码成线上字节。
///
/// 官方 `ItemStackTemplate.STREAM_CODEC` 里的 `DataComponentPatch` 用每个组件自己的
/// `Codec` 编码，不能直接把数据包 JSON 当 NBT 写。这里先按组件的存档 codec
///（`mc_bamboo_protocol::components::archive::components::decode_value`）把 JSON 转成
/// 类型化的 `ComponentValue`，再走 `ComponentPatch::write` 写线上格式。
fn icon_components(
    object: &serde_json::Map<String, Value>,
    id: &str,
) -> Result<Vec<(i32, Vec<u8>)>, TreeError> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, PatchEntry, archive::components};
    let mut result = Vec::new();
    for (name, value) in object {
        let kind = match name.as_str() {
            "minecraft:pot_decorations" => kinds::POT_DECORATIONS,
            "minecraft:banner_patterns" => kinds::BANNER_PATTERNS,
            "minecraft:item_name" => kinds::ITEM_NAME,
            "minecraft:rarity" => kinds::RARITY,
            "minecraft:tooltip_display" => kinds::TOOLTIP_DISPLAY,
            other => return Err(bad(format!("进度 {id} 的图标有不支持的组件 {other}"))),
        };
        let tag = component_json_tag(value, name)?;
        let typed: ComponentValue = components::decode_value(kind, &tag, 0)
            .map_err(|error| bad(format!("进度 {id} 的图标组件 {name}: {error}")))?;
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(kind, PatchEntry::Set(typed));
        // `ComponentPatch::write` 前面带 added/removed 两个计数；这里只要这一项的值。
        let mut full = Vec::new();
        patch.write(&mut full);
        let mut cursor = mc_bamboo_protocol::components::cursor::Cursor::new(&full);
        let decoded = mc_bamboo_protocol::components::ComponentPatch::read(&mut cursor)
            .map_err(|error| bad(format!("进度 {id} 的图标组件 {name}: {error}")))?;
        let (added, _) = decoded.to_opaque();
        let bytes = added
            .first()
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| bad(format!("进度 {id} 的图标组件 {name} 没有值")))?;
        result.push((kind, bytes));
    }
    Ok(result)
}

/// 数据包里的组件值转成官方存档 `Codec` 认的 NBT 标签。
fn component_json_tag(value: &Value, name: &str) -> Result<mc_bamboo_nbt::Tag, TreeError> {
    use mc_bamboo_nbt::Tag;
    match value {
        Value::String(text) => Ok(Tag::String(text.clone())),
        Value::Bool(flag) => Ok(Tag::Byte(i8::from(*flag))),
        Value::Number(number) => number
            .as_i64()
            .map(|value| Tag::Int(value as i32))
            .or_else(|| number.as_f64().map(Tag::Double))
            .ok_or_else(|| bad(format!("组件 {name} 的数字不合法"))),
        Value::Array(values) => Ok(Tag::List(
            values
                .iter()
                .map(|value| component_json_tag(value, name))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(entries) => Ok(Tag::Compound(
            entries
                .iter()
                .map(|(key, value)| Ok((key.clone(), component_json_tag(value, name)?)))
                .collect::<Result<Vec<_>, TreeError>>()?,
        )),
        Value::Null => Ok(Tag::Compound(Vec::new())),
    }
}

impl AdvancementTree {
    /// 从生成器导出的原文建树。`child_order` 给 `External` 顺序时用，长度要和定义数一致。
    pub fn parse(
        json: &str,
        order: TreeOrder,
        child_order: Option<&[Vec<String>]>,
    ) -> Result<Self, TreeError> {
        let table: Value = serde_json::from_str(json).map_err(|error| bad(error.to_string()))?;
        let table = table.as_object().ok_or_else(|| bad("进度导出不是对象"))?;
        let mut triggers = Vec::new();
        for value in field(table, "triggers")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("缺少触发器注册表"))?
        {
            let name = value
                .as_str()
                .ok_or_else(|| bad("触发器注册表里有非字符串"))?;
            triggers.push(
                find_static_name(mc_bamboo_data::ADVANCEMENTS_JSON, name)
                    .ok_or_else(|| bad(format!("触发器 {name} 不在内嵌数据里")))?,
            );
        }
        // 定义从静态字符串里切出来，节点可以只存 &'static str。
        let static_json: &'static str = mc_bamboo_data::ADVANCEMENTS_JSON;
        let definitions = field(table, "advancements")
            .and_then(Value::as_object)
            .ok_or_else(|| bad("缺少进度定义"))?;

        let mut ids: Vec<&'static str> = Vec::with_capacity(definitions.len());
        let mut parsed = Vec::with_capacity(definitions.len());
        for (id, value) in definitions {
            let value = value
                .as_object()
                .ok_or_else(|| bad(format!("进度 {id} 不是对象")))?;
            // `static_json` 与 `json` 在正常调用下是同一份；不同时退化为静态表里的同名字串。
            let static_id = find_static_id(static_json, id)
                .ok_or_else(|| bad(format!("进度 {id} 不在内嵌数据里")))?;
            ids.push(static_id);
            parsed.push(parse_advancement(static_id, value)?);
        }
        let by_id: BTreeMap<&'static str, usize> = ids
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, index))
            .collect();

        let mut nodes: Vec<Node> = parsed
            .into_iter()
            .enumerate()
            .map(|(index, advancement)| Node {
                id: ids[index],
                advancement,
                parent: None,
                children: Vec::new(),
                x: 0.0,
                y: 0.0,
            })
            .collect();
        // `AdvancementTree.addAll`：父节点必须先出现，这里按 id 顺序反复插入。
        // 按 id 顺序插入：`addAll` 反复尝试，直到父节点已经登记。
        let mut pending: Vec<usize> = (0..nodes.len()).collect();
        let mut registered = vec![false; nodes.len()];
        loop {
            let mut next = Vec::new();
            let mut inserted = false;
            pending.sort_unstable_by(|&a, &b| {
                let depth_a = parent_depth(&nodes, &by_id, a);
                let depth_b = parent_depth(&nodes, &by_id, b);
                depth_a.cmp(&depth_b).then(nodes[a].id.cmp(nodes[b].id))
            });
            for &index in &pending {
                let parent_id = nodes[index].advancement.parent.clone();
                let parent = match parent_id.as_deref() {
                    Some(id) => match by_id.get(id) {
                        Some(&parent) if registered[parent] => Some(parent),
                        Some(_) => {
                            next.push(index);
                            continue;
                        }
                        None => {
                            // 父引用不存在：官方 addAll 会把它留在未加载表里，这里同样跳过。
                            continue;
                        }
                    },
                    None => None,
                };
                if let Some(parent) = parent {
                    nodes[parent].children.push(index);
                }
                nodes[index].parent = parent;
                registered[index] = true;
                inserted = true;
            }
            if next.is_empty() {
                break;
            }
            if !inserted {
                return Err(bad("进度树里有环"));
            }
            pending = next;
        }
        let mut tree = Self {
            triggers,
            nodes,
            by_id,
            roots: Vec::new(),
            displayed: Vec::new(),
        };
        tree.apply_order(order, child_order)?;
        Ok(tree)
    }

    /// 全进程共用的一份树，正式运行的兄弟顺序按 id 字典序。
    pub fn global() -> &'static Self {
        static GLOBAL: OnceLock<AdvancementTree> = OnceLock::new();
        GLOBAL.get_or_init(|| {
            Self::parse(mc_bamboo_data::ADVANCEMENTS_JSON, TreeOrder::ById, None)
                .expect("内嵌的进度定义必须是合法的")
        })
    }

    /// 按 id 找节点。
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.by_id.get(id).map(|&index| &self.nodes[index])
    }

    /// 按 id 找节点下标。
    pub fn index(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// 某个节点的根（官方 `AdvancementNode.root`）。
    pub fn root_of(&self, mut index: usize) -> usize {
        while let Some(parent) = self.nodes[index].parent {
            index = parent;
        }
        index
    }

    /// 重新排兄弟顺序并算坐标。对拍把官方当次的兄弟顺序传进来验证算法。
    pub fn apply_order(
        &mut self,
        order: TreeOrder,
        child_order: Option<&[Vec<String>]>,
    ) -> Result<(), TreeError> {
        match (order, child_order) {
            (TreeOrder::ById, _) => {
                let ids: Vec<&'static str> = self.nodes.iter().map(|node| node.id).collect();
                for node in &mut self.nodes {
                    node.children.sort_unstable_by_key(|&index| ids[index]);
                }
            }
            (TreeOrder::External, Some(child_order)) => {
                if child_order.len() != self.nodes.len() {
                    return Err(bad("外部兄弟顺序的长度和节点数不一致"));
                }
                for (index, names) in child_order.iter().enumerate() {
                    let mut children = Vec::with_capacity(names.len());
                    for name in names {
                        let child = self
                            .by_id
                            .get(name.as_str())
                            .copied()
                            .ok_or_else(|| bad(format!("外部顺序里有未知进度 {name}")))?;
                        children.push(child);
                    }
                    self.nodes[index].children = children;
                }
            }
            (TreeOrder::External, None) => return Err(bad("外部顺序需要提供 child_order")),
        }
        self.roots = (0..self.nodes.len())
            .filter(|&index| self.nodes[index].advancement.is_root())
            .collect();
        self.roots
            .sort_unstable_by(|&a, &b| self.nodes[a].id.cmp(self.nodes[b].id));
        self.displayed = (0..self.nodes.len())
            .filter(|&index| self.nodes[index].advancement.display.is_some())
            .collect();
        self.displayed
            .sort_unstable_by(|&a, &b| self.nodes[a].id.cmp(self.nodes[b].id));
        // 只排有显示的根，和 `AdvancementTree.repositionNodes` 一致。
        for &root in &self.roots {
            let positions = layout::position(self, root);
            for (index, (x, y)) in positions {
                self.nodes[index].x = x;
                self.nodes[index].y = y;
            }
        }
        Ok(())
    }

    /// 所有节点（含没显示的），按 id 排序。
    pub fn all_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_id.keys().copied()
    }
}

/// 内嵌 JSON 里定义的 id 是静态字符串，切出来给节点存借用。
/// 从内嵌 JSON 的原文里切出一段静态字符串。定义 id 后面跟 `:`，触发器是数组里的字符串。
/// 父链的深度，用来让插入顺序稳定（导出的定义按名字排，父不一定在子前面）。
fn parent_depth(nodes: &[Node], by_id: &BTreeMap<&'static str, usize>, index: usize) -> usize {
    let mut depth = 0;
    let mut current = index;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(parent) = nodes[current].advancement.parent.as_deref() {
        if !seen.insert(current) {
            break;
        }
        let Some(&parent) = by_id.get(parent) else {
            break;
        };
        current = parent;
        depth += 1;
    }
    depth
}

fn find_static_name(static_json: &'static str, name: &str) -> Option<&'static str> {
    if let Some(position) = static_json.find(name) {
        return Some(&static_json[position..position + name.len()]);
    }
    None
}

fn find_static_id(static_json: &'static str, id: &str) -> Option<&'static str> {
    find_static_name(static_json, id)
}

fn parse_advancement(
    id: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<Advancement, TreeError> {
    let parent = field(object, "parent")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("进度 {id} 的 parent 不是字符串")))
        })
        .transpose()?;
    let display = match object_field(object, "display", id)? {
        Some(display) => Some(parse_display(id, display)?),
        None => None,
    };
    let rewards = match object_field(object, "rewards", id)? {
        Some(rewards) => parse_rewards(id, rewards)?,
        None => Rewards::default(),
    };
    let mut criteria = Vec::new();
    if let Some(value) = field(object, "criteria") {
        let map = value
            .as_object()
            .ok_or_else(|| bad(format!("进度 {id} 的 criteria 不是对象")))?;
        for (name, value) in map {
            let criterion = value
                .as_object()
                .ok_or_else(|| bad(format!("进度 {id} 的判据 {name} 不是对象")))?;
            let trigger =
                str_field(criterion, "trigger", &format!("进度 {id} 的判据 {name}"))?.to_owned();
            let conditions = field(criterion, "conditions")
                .cloned()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
            criteria.push((
                name.clone(),
                Criterion {
                    trigger,
                    conditions,
                },
            ));
        }
    }
    let requirements = match field(object, "requirements") {
        Some(value) => {
            let groups = value
                .as_array()
                .ok_or_else(|| bad(format!("进度 {id} 的 requirements 不是数组")))?;
            groups
                .iter()
                .map(|group| string_list(group, &format!("进度 {id} 的 requirements")))
                .collect::<Result<Vec<_>, _>>()?
        }
        // 官方缺省是 allOf(criteria.keySet())。
        None => criteria
            .iter()
            .map(|(name, _)| vec![name.clone()])
            .collect(),
    };
    let sends_telemetry_event = field(object, "sends_telemetry_event")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Advancement {
        parent,
        display,
        rewards,
        criteria,
        requirements,
        sends_telemetry_event,
    })
}

fn parse_display(id: &str, object: &serde_json::Map<String, Value>) -> Result<Display, TreeError> {
    let icon =
        object_field(object, "icon", id)?.ok_or_else(|| bad(format!("进度 {id} 的显示缺 icon")))?;
    let icon_name = str_field(icon, "id", &format!("进度 {id} 的图标"))?;
    let icon_id = mc_bamboo_data::items::ITEMS
        .iter()
        .position(|item| item.name == icon_name)
        .ok_or_else(|| bad(format!("进度 {id} 的图标 {icon_name} 未注册")))?
        as i32;
    let icon_components = match object_field(icon, "components", id)? {
        Some(components) => icon_components(components, id)?,
        None => Vec::new(),
    };
    let title = component_json_tag(
        field(object, "title").ok_or_else(|| bad(format!("进度 {id} 的显示缺 title")))?,
        "title",
    )?;
    let description = component_json_tag(
        field(object, "description")
            .ok_or_else(|| bad(format!("进度 {id} 的显示缺 description")))?,
        "description",
    )?;
    let background = field(object, "background")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("进度 {id} 的 background 不是字符串")))
        })
        .transpose()?;
    let frame = match field(object, "frame") {
        Some(value) => {
            let name = value
                .as_str()
                .ok_or_else(|| bad(format!("进度 {id} 的 frame 不是字符串")))?;
            Frame::from_name(name).ok_or_else(|| bad(format!("进度 {id} 有未知边框 {name}")))?
        }
        None => Frame::Task,
    };
    let bool_field = |name: &str, default: bool| -> Result<bool, TreeError> {
        field(object, name)
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| bad(format!("进度 {id} 的 {name} 不是布尔值")))
            })
            .transpose()
            .map(|value| value.unwrap_or(default))
    };
    Ok(Display {
        icon: icon_id,
        icon_components,
        title: Component::trusted(title),
        description: Component::trusted(description),
        background,
        frame,
        show_toast: bool_field("show_toast", true)?,
        announce_to_chat: bool_field("announce_to_chat", true)?,
        hidden: bool_field("hidden", false)?,
    })
}

fn parse_rewards(id: &str, object: &serde_json::Map<String, Value>) -> Result<Rewards, TreeError> {
    let string_or_list = |name: &str| -> Result<Vec<String>, TreeError> {
        field(object, name)
            .map(|value| string_list(value, &format!("进度 {id} 的奖励 {name}")))
            .transpose()
            .map(Option::unwrap_or_default)
    };
    let experience = field(object, "experience")
        .map(|value| {
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| bad(format!("进度 {id} 的 experience 不是整数")))
        })
        .transpose()?
        .unwrap_or(0);
    let function = field(object, "function")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| bad(format!("进度 {id} 的 function 不是字符串")))
        })
        .transpose()?;
    Ok(Rewards {
        experience,
        loot: string_or_list("loot")?,
        recipes: string_or_list("recipes")?,
        function,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_tree_matches_the_exported_counts() {
        let tree = AdvancementTree::parse(mc_bamboo_data::ADVANCEMENTS_JSON, TreeOrder::ById, None)
            .unwrap();
        assert_eq!(tree.triggers.len(), 58);
        // 6 个根、126 个显示、1866 个节点、4045 个判据。
        assert_eq!(tree.nodes.len(), 1866);
        assert_eq!(tree.roots.len(), 5);
        assert_eq!(tree.displayed.len(), 126);
        let criteria: usize = tree
            .nodes
            .iter()
            .map(|node| node.advancement.criteria.len())
            .sum();
        assert_eq!(criteria, 4045);
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|node| node.advancement.parent.is_none())
                .count(),
            6
        );
        let mut triggers = std::collections::BTreeSet::new();
        for node in &tree.nodes {
            let names: std::collections::BTreeSet<&str> = node
                .advancement
                .requirements
                .iter()
                .flat_map(|group| group.iter().map(String::as_str))
                .collect();
            assert_eq!(names.len(), node.advancement.criteria.len(), "{}", node.id);
            triggers.extend(
                node.advancement
                    .criteria
                    .iter()
                    .map(|(_, criterion)| criterion.trigger.as_str()),
            );
        }
        assert_eq!(triggers.len(), 54);
        // 每个根都排到了坐标；story/root 的 x 是 0。
        let root = tree.index("minecraft:story/root").unwrap();
        assert_eq!(tree.nodes[root].x, 0.0);
        assert!(tree.nodes[root].y.is_finite());
    }

    #[test]
    fn parent_links_and_requirement_strategy_are_preserved() {
        let tree = AdvancementTree::global();
        let child = tree.node("minecraft:story/mine_stone").unwrap();
        assert_eq!(
            child.advancement.parent.as_deref(),
            Some("minecraft:story/root")
        );
        let parent = tree
            .index(child.advancement.parent.as_deref().unwrap())
            .unwrap();
        assert!(
            tree.nodes[parent]
                .children
                .contains(&tree.index("minecraft:story/mine_stone").unwrap())
        );
        let either = tree.node("minecraft:adventure/root").unwrap();
        assert_eq!(either.advancement.requirements.len(), 1);
        assert_eq!(either.advancement.requirements[0].len(), 2);
        assert_eq!(
            tree.root_of(tree.index("minecraft:story/mine_stone").unwrap()),
            tree.index("minecraft:story/root").unwrap()
        );
    }

    #[test]
    fn layout_depends_on_the_sibling_order() {
        let json = mc_bamboo_data::ADVANCEMENTS_JSON;
        let a = AdvancementTree::parse(json, TreeOrder::ById, None).unwrap();
        let mut b = a.clone();
        // 故意打乱第一棵有显示的根下的兄弟顺序（改坏确认）。
        if let Some(&root) = a.roots.first() {
            let reversed: Vec<Vec<String>> = (0..a.nodes.len())
                .map(|index| {
                    let mut names: Vec<String> = a.nodes[index]
                        .children
                        .iter()
                        .map(|&child| a.nodes[child].id.to_owned())
                        .collect();
                    if index == root {
                        names.reverse();
                    }
                    names
                })
                .collect();
            b.apply_order(TreeOrder::External, Some(&reversed)).unwrap();
            let changed = a.displayed.iter().zip(&b.displayed).any(|(&x, &y)| {
                x != y || a.nodes[x].x != b.nodes[y].x || a.nodes[x].y != b.nodes[y].y
            });
            assert!(changed, "打乱兄弟顺序后至少有一个坐标不同");
        }
    }

    #[test]
    fn display_defaults_and_flags_match_vanilla() {
        let tree = AdvancementTree::global();
        let root = tree.node("minecraft:story/root").unwrap();
        let display = root.advancement.display.as_ref().unwrap();
        assert!(!display.show_toast);
        assert!(!display.announce_to_chat);
        assert!(display.background.is_some());
        assert_eq!(display.frame, Frame::Task);
        assert_eq!(display.flags(), 1);
        let hidden = tree.node("minecraft:adventure/arbalistic").unwrap();
        assert_eq!(
            hidden.advancement.display.as_ref().unwrap().frame,
            Frame::Challenge
        );
    }
}
