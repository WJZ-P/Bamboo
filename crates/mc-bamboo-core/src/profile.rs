//! 账号档案上的属性，对应官方 authlib 的 `com.mojang.authlib.properties.Property`。
//!
//! 验证层算出它，协议层把它写进包里，世界线程又要把它存着发给后来的玩家，
//! 所以这个类型放在它们下面。

/// 档案上的一项属性。皮肤和披风的地址都在名为 `textures` 的那一项里。
///
/// `signature` 是 Mojang 用自己的私钥对 `value` 的签名。客户端只有拿到签名才会显示
/// 别人的披风，所以这一项要原样转发，不能自己拼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}
