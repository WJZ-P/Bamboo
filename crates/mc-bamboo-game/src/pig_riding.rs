//! 猪的胡萝卜钓竿加速：把使用物品、载具状态和耐久变化接起来。

#[cfg(test)]
mod tests;

#[cfg(feature = "oracle-samples")]
mod replay;
#[cfg(feature = "oracle-samples")]
pub use replay::*;
