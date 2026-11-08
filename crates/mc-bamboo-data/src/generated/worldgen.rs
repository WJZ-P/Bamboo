// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 官方数据包里世界生成的几个注册表：`worldgen/noise`、`worldgen/density_function`、
/// `worldgen/noise_settings`、两个多噪声群系预设和末地的五个群系。
///
/// 前三个是官方 codec 编出来的 JSON，预设是量化好的定点数。解析在 `mc-bamboo-worldgen`。
pub const WORLDGEN: &str = include_str!("worldgen.json");
