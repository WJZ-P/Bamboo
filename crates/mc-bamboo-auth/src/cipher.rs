//! 连接上的 AES 数据流，对应官方 `Crypt.getCipher` 和 `Connection.setEncryptionKey`。

use std::fmt;

use aes::{
    Aes128,
    cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, inout::InOutBuf},
};

/// 客户端送来的 AES 密钥是 16 个字节，官方 `Crypt.generateSecretKey` 生成的也是。
pub const SHARED_SECRET_LEN: usize = 16;

/// 客户端和服务端共用的那把 AES 密钥。
pub type SharedSecret = [u8; SHARED_SECRET_LEN];

/// 出站方向的 AES/CFB8 流。
///
/// CFB8 每次只吐一个字节，所以加密不改变数据的长度，也不需要攒够一个分组。
pub struct EncryptHalf(cfb8::Encryptor<Aes128>);

impl EncryptHalf {
    /// 原地加密要发出去的字节。
    pub fn encrypt(&mut self, buffer: &mut [u8]) {
        let (blocks, tail) = InOutBuf::from(buffer).into_chunks();
        debug_assert!(tail.is_empty(), "CFB8 的分组只有一个字节");
        self.0.encrypt_blocks_inout_mut(blocks);
    }
}

/// 入站方向的 AES/CFB8 流。
pub struct DecryptHalf(cfb8::Decryptor<Aes128>);

impl DecryptHalf {
    /// 原地解密刚收到的字节。
    pub fn decrypt(&mut self, buffer: &mut [u8]) {
        let (blocks, tail) = InOutBuf::from(buffer).into_chunks();
        debug_assert!(tail.is_empty(), "CFB8 的分组只有一个字节");
        self.0.decrypt_blocks_inout_mut(blocks);
    }
}

/// 一条连接上的两个 AES/CFB8 流，收和发各一个。
///
/// 这两个流各自带着自己的状态，所以必须按字节顺序喂给它们：收到的字节一到就解密，
/// 要发的字节在写进 socket 之前加密，中间不能跳过也不能重放。
pub struct Encryption {
    encrypt: EncryptHalf,
    decrypt: DecryptHalf,
}

impl Encryption {
    /// 官方的 `Crypt.getCipher` 把同一份密钥既当密钥又当 IV。
    pub fn new(secret: &SharedSecret) -> Self {
        Self {
            encrypt: EncryptHalf(cfb8::Encryptor::new(secret.into(), secret.into())),
            decrypt: DecryptHalf(cfb8::Decryptor::new(secret.into(), secret.into())),
        }
    }

    /// 原地加密要发出去的字节。
    pub fn encrypt(&mut self, buffer: &mut [u8]) {
        self.encrypt.encrypt(buffer);
    }

    /// 原地解密刚收到的字节。
    pub fn decrypt(&mut self, buffer: &mut [u8]) {
        self.decrypt.decrypt(buffer);
    }

    /// 入站那一半。读 socket 的那一层只碰这个方向。
    pub fn decrypt_half(&mut self) -> &mut DecryptHalf {
        &mut self.decrypt
    }

    /// 拆成两半。Play 阶段读和写在不同的任务里，各自拿走自己那个方向的流。
    pub fn split(self) -> (EncryptHalf, DecryptHalf) {
        (self.encrypt, self.decrypt)
    }
}

/// 不要把密钥打进日志。
impl fmt::Debug for Encryption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Encryption")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: SharedSecret = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];

    /// 官方 JDK 的 `AES/CFB8/NoPadding` 用上面那把密钥（同时当 IV）加密
    /// `Bamboo online mode` 得到的字节，照抄下来当对照。
    const PLAIN: &[u8] = b"Bamboo online mode";
    const CIPHER: [u8; 18] = [
        0x20, 0x3f, 0x3f, 0xa9, 0xc9, 0x69, 0x1e, 0x62, 0x84, 0x7a, 0xf6, 0x03, 0x36, 0xd0, 0x20,
        0xcf, 0x94, 0x69,
    ];

    #[test]
    fn matches_the_jdk_cipher() {
        let mut buffer = PLAIN.to_vec();
        Encryption::new(&SECRET).encrypt(&mut buffer);
        assert_eq!(buffer, CIPHER);

        let mut buffer = CIPHER.to_vec();
        Encryption::new(&SECRET).decrypt(&mut buffer);
        assert_eq!(buffer, PLAIN);
    }

    #[test]
    fn decrypts_what_it_encrypted() {
        let message: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let mut buffer = message.clone();
        Encryption::new(&SECRET).encrypt(&mut buffer);
        assert_ne!(buffer, message);
        Encryption::new(&SECRET).decrypt(&mut buffer);
        assert_eq!(buffer, message);
    }

    /// 流的状态必须跨调用保留：一段一段加密和一次性加密要得到一样的字节。
    #[test]
    fn keeps_its_state_between_calls() {
        let mut piecewise = PLAIN.to_vec();
        let mut encryption = Encryption::new(&SECRET);
        for chunk in piecewise.chunks_mut(5) {
            encryption.encrypt(chunk);
        }
        assert_eq!(piecewise, CIPHER);

        let mut piecewise = CIPHER.to_vec();
        let mut encryption = Encryption::new(&SECRET);
        for chunk in piecewise.chunks_mut(5) {
            encryption.decrypt(chunk);
        }
        assert_eq!(piecewise, PLAIN);
    }

    /// 拆开之后两半各自接着原来的状态走。
    #[test]
    fn the_halves_keep_the_streams_going() {
        let (mut encrypt, mut decrypt) = Encryption::new(&SECRET).split();

        let mut buffer = PLAIN.to_vec();
        encrypt.encrypt(&mut buffer);
        assert_eq!(buffer, CIPHER);

        let mut buffer = CIPHER.to_vec();
        decrypt.decrypt(&mut buffer);
        assert_eq!(buffer, PLAIN);
    }

    #[test]
    fn an_empty_buffer_changes_nothing() {
        let mut encryption = Encryption::new(&SECRET);
        encryption.encrypt(&mut []);
        encryption.decrypt(&mut []);

        let mut buffer = PLAIN.to_vec();
        encryption.encrypt(&mut buffer);
        assert_eq!(buffer, CIPHER);
    }
}
