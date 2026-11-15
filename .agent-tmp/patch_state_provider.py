"""把 `Noise` 和 `Rotated` 两个 provider 补进以 main 为基底的 state_provider.rs。

Adds the `Noise` and `Rotated` providers on top of main's state_provider.rs.
"""

p = 'crates/mc-bamboo-worldgen/src/feature/state_provider.rs'
s = open(p, encoding='utf-8').read()

# 1) enum
s = s.replace(
    '''    /// `RandomBlockProvider`：从方块集合里等概率取一个，用它的默认状态。
    ///
    /// `RandomBlockProvider`: pick one block uniformly and use its default state.
    RandomBlock(Vec<u32>),
}''',
    '''    /// `RandomBlockProvider`：从方块集合里等概率取一个，用它的默认状态。
    ///
    /// `RandomBlockProvider`: pick one block uniformly and use its default state.
    RandomBlock(Vec<u32>),
    /// `NoiseProvider` / `DualNoiseProvider` / `NoiseThresholdProvider`（按坐标用噪声）。
    ///
    /// The three noise-based providers, which pick a block from a noise sample at `pos`.
    Noise(Box<NoiseStateProvider>),
    /// `RotatedBlockProvider`：先采样内层，再把方向相关属性改掉。
    ///
    /// `RotatedBlockProvider`: sample the inner provider, then overwrite the
    /// direction-dependent properties.
    Rotated {
        source: Box<StateProvider>,
        direction: Option<&'static str>,
    },
}''')

# 2) parse_with branches
s = s.replace(
    '''            "minecraft:random_block" => {
                Self::RandomBlock(parse_block_states(object.get("blocks")?)?)
            }
            _ => return None,''',
    '''            "minecraft:random_block" => {
                Self::RandomBlock(parse_block_states(object.get("blocks")?)?)
            }
            // 三个 noise provider 的 `noise` / `slow_noise` 是内联参数（不是注册表引用），
            // 所以直接按值解析，不需要 `lookup`。
            // The three noise providers carry inline `NormalNoise` parameters, so they
            // parse by value and need no `lookup`.
            "minecraft:noise" | "minecraft:dual_noise" | "minecraft:noise_threshold" => {
                Self::Noise(Box::new(NoiseStateProvider::parse(value)?))
            }
            // `RotatedBlockProvider.CODEC`：`state` + 可选 `direction`。
            // `RotatedBlockProvider.CODEC`: `state` plus an optional `direction`.
            "minecraft:rotated" => Self::Rotated {
                source: Box::new(Self::parse_with(object.get("state")?, lookup)?),
                direction: match object.get("direction") {
                    Some(value) => Some(parse_direction(value.as_str()?)?),
                    None => None,
                },
            },
            _ => return None,''')

# 3) supported
s = s.replace(
    '''            Self::Simple(_)
            | Self::Weighted { .. }
            | Self::RandomizedInt { .. }
            | Self::RandomBlock(_) => true,''',
    '''            Self::Simple(_)
            | Self::Weighted { .. }
            | Self::RandomBlock(_)
            | Self::Noise(_) => true,
            Self::Rotated { source, .. } => source.supported(),
            Self::RandomizedInt { source, .. } => source.supported(),''')

# 4) possible_states
s = s.replace(
    '''            Self::RandomBlock(states) => states.clone(),''',
    '''            Self::RandomBlock(states) => states.clone(),
            Self::Noise(provider) => provider.possible_states(),
            // 方向属性不改变方块类，内层 provider 的状态就够了。
            // The direction properties do not change the block class.
            Self::Rotated { source, .. } => source.possible_states(),''')

# 5) get_state fallthrough
s = s.replace(
    '''            Self::RuleBased { .. } => None,
        }
    }''',
    '''            // `Rotated` 要先定 `direction`（可能消费随机数）再采样，走 `get_optional_state`。
            // `Rotated` resolves `direction` first (possibly consuming random) and then
            // samples, so it goes through `get_optional_state`.
            Self::Rotated { .. } | Self::RuleBased { .. } => None,
        }
    }''')

# 6) get_optional_state
s = s.replace(
    '''            Self::RuleBased { fallback, rules } => {
                for (predicate, provider) in rules {''',
    '''            Self::Noise(provider) => provider.get_state(random, pos),
            // 官方先把 `direction` 定下来（缺省时 `Direction.getRandom(random)` 消费一次
            // `nextInt(6)`），再采样内层 provider，最后依次尝试 `axis` / `facing` /
            // `horizontal_facing` 三个属性（有该属性才改）。
            // The official code resolves `direction` first (consuming one `nextInt(6)` when
            // absent), samples the inner provider, then tries `axis`, `facing` and
            // `horizontal_facing` in that order, setting only properties the state has.
            Self::Rotated { source, direction } => {
                let direction = match direction {
                    Some(direction) => *direction,
                    None => DIRECTIONS[random.next_int(6) as usize],
                };
                let base = source.get_optional_state(world, random, pos)?;
                let state = with_property(base, "axis", axis_of(direction)).unwrap_or(base);
                let state = with_property(state, "facing", direction).unwrap_or(state);
                Some(if is_horizontal(direction) {
                    with_property(state, "horizontal_facing", direction).unwrap_or(state)
                } else {
                    state
                })
            }
            Self::RuleBased { fallback, rules } => {
                for (predicate, provider) in rules {''')

# 7) helpers
s = s.replace(
    '''/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名/`{"id":..}` 数组；结果取默认状态。''',
    '''/// `Direction.values()` 的六个方向，顺序即 ordinal。
///
/// The six `Direction.values()` entries in ordinal order.
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// `Direction.CODEC` 的方向名。
///
/// A name accepted by `Direction.CODEC`.
fn parse_direction(name: &str) -> Option<&'static str> {
    DIRECTIONS.iter().copied().find(|entry| *entry == name)
}

/// `Direction.getAxis().getName()`。
///
/// `Axis.getName()` for `Direction.getAxis()`.
fn axis_of(direction: &str) -> &'static str {
    match direction {
        "up" | "down" => "y",
        "north" | "south" => "z",
        _ => "x",
    }
}

/// `Direction.Axis.isHorizontal()`。
///
/// `Direction.Axis.isHorizontal()`.
fn is_horizontal(direction: &str) -> bool {
    !matches!(direction, "up" | "down")
}

/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名/`{"id":..}` 数组；结果取默认状态。''')

# 8) import
if 'noise_state::NoiseStateProvider' not in s:
    s = s.replace('use super::{', 'use super::{\n    noise_state::NoiseStateProvider,', 1)

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('patched state_provider.rs')
