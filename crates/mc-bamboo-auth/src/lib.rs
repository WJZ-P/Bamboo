//! 正版验证和协议加密。
//!
//! 对应官方的 `net.minecraft.util.Crypt`、`ServerLoginPacketListenerImpl` 里向
//! session server 提问的那一段，以及 authlib 的
//! `com.mojang.authlib.yggdrasil.YggdrasilMinecraftSessionService`。
//!
//! 这个 crate 只管密码学和 HTTP：它不认识数据包，也不认识世界线程。登录流程本身在
//! `mc-bamboo-server` 的连接任务里。

mod cipher;
mod digest;
mod http;
mod keypair;
mod session;

pub use cipher::{DecryptHalf, EncryptHalf, Encryption, SHARED_SECRET_LEN, SharedSecret};
pub use digest::{minecraft_hex_digest, server_id_hash};
pub use keypair::{
    CHALLENGE_LEN, Challenge, CryptError, ServerKeyPair, encrypt_to_server, random_challenge,
};
pub use session::{AuthError, Profile, SESSION_SERVER_URL, has_joined};
