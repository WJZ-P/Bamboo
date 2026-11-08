//! 服务端的 RSA 密钥对，对应官方 `MinecraftServer.initializeKeyPair` 和
//! `Crypt.generateKeyPair` / `Crypt.decryptByteToSecretKey`。
//!
//! # 已知安全问题：RUSTSEC-2023-0071
//!
//! 这里用的 `rsa` crate（0.9）有一个计时侧信道（Marvin 攻击）：解密的耗时和私钥有关，
//! 能反复计时的攻击者理论上可以把私钥还原出来。上游至今没有修好的版本，所以换版本解决不了。
//!
//! 缓解方式是 [`ServerKeyPair::decrypt`] 里的 `decrypt_blinded`：每次解密先用随机数把密文
//! 盲化，解密耗时就和真实的密文无关了。这是上游推荐的做法，但 advisory 本身仍然挂着。
//!
//! 残余风险在这里有限：这把钥匙每次启动重新生成（官方也是），只用来接一次 AES 密钥交换，
//! 泄露它最多让攻击者解开同一次启动期间的登录握手，拿不到 Mojang 的任何凭据。真要彻底躲开，
//! 得换一个解密实现或者自己做 PKCS#1 解包，那是另一件事。

use rand_core::{OsRng, RngCore};
use rsa::{
    Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePublicKey, EncodePublicKey},
};

use crate::cipher::{SHARED_SECRET_LEN, SharedSecret};

/// 官方 `Crypt.generateKeyPair` 用 1024 位 RSA。这把钥匙只用来交换一次 AES 密钥，
/// 每次启动都重新生成，所以它短得有点旧派也无所谓。
const KEY_BITS: usize = 1024;

/// 官方的 challenge 是一个 int，`Ints.toByteArray` 之后正好 4 个字节。
pub const CHALLENGE_LEN: usize = 4;

/// 服务端发给客户端、再要求原样加密送回来的那几个字节。
pub type Challenge = [u8; CHALLENGE_LEN];

#[derive(Debug, thiserror::Error)]
pub enum CryptError {
    #[error("the public key is not a valid RSA key: {0}")]
    PublicKey(rsa::pkcs8::spki::Error),
    #[error("cannot encrypt with the public key: {0}")]
    Encrypt(rsa::Error),
    #[error("the server key cannot decrypt the data")]
    Decrypt,
    #[error("expected a shared secret of {SHARED_SECRET_LEN} bytes, got {0}")]
    SecretLength(usize),
}

/// 服务端这次启动用的密钥对。
pub struct ServerKeyPair {
    private_key: RsaPrivateKey,
    /// 公钥的 X.509 SubjectPublicKeyInfo 编码，也就是 Java
    /// `PublicKey.getEncoded()` 给出的那串字节。它要原样进 hello 包，
    /// 还要参与 serverId 哈希，所以生成一次就存着。
    public_key_der: Vec<u8>,
}

impl ServerKeyPair {
    /// 生成一对新钥匙。官方在这里失败是直接抛 `IllegalStateException` 把服务端拖垮的，
    /// 这里同样不给调用方留退路：没有钥匙就没法开正版验证。
    pub fn generate() -> Self {
        let private_key = RsaPrivateKey::new(&mut OsRng, KEY_BITS)
            .expect("generating an RSA key pair should not fail");
        let public_key_der = RsaPublicKey::from(&private_key)
            .to_public_key_der()
            .expect("encoding an RSA public key should not fail")
            .as_bytes()
            .to_vec();
        Self {
            private_key,
            public_key_der,
        }
    }

    pub fn public_key_der(&self) -> &[u8] {
        &self.public_key_der
    }

    /// 用私钥解开客户端送来的数据。Java 的 `Cipher.getInstance("RSA")` 默认是
    /// PKCS#1 v1.5 填充，客户端的 `Crypt.encryptUsingKey` 走的就是这条路。
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptError> {
        // 必须用带盲化的解密，免得解密耗时泄露私钥。模块注释里写了 RUSTSEC-2023-0071 的来龙去脉。
        self.private_key
            .decrypt_blinded(&mut OsRng, Pkcs1v15Encrypt, ciphertext)
            .map_err(|_| CryptError::Decrypt)
    }

    /// 解开客户端送来的 AES 密钥，对应官方 `Crypt.decryptByteToSecretKey`。
    ///
    /// 官方那边长度不对会在之后用这把密钥的时候才炸，这里提前挡掉。
    pub fn decrypt_shared_secret(&self, ciphertext: &[u8]) -> Result<SharedSecret, CryptError> {
        let secret = self.decrypt(ciphertext)?;
        SharedSecret::try_from(secret.as_slice())
            .map_err(|_| CryptError::SecretLength(secret.len()))
    }
}

/// 不要把私钥打进日志。
impl std::fmt::Debug for ServerKeyPair {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ServerKeyPair")
    }
}

/// 客户端那一侧：用服务端公布的公钥加密一小段数据。
///
/// 真正的客户端才需要这个函数，服务端自己用不上它。留在这里是为了让测试和探针
/// 能扮演客户端，对应官方 `Crypt.encryptUsingKey`。
pub fn encrypt_to_server(public_key_der: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptError> {
    RsaPublicKey::from_public_key_der(public_key_der)
        .map_err(CryptError::PublicKey)?
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, plaintext)
        .map_err(CryptError::Encrypt)
}

/// 新的 challenge。官方用的是普通的 `RandomSource`，这里用系统随机数，更难猜。
pub fn random_challenge() -> Challenge {
    let mut challenge = Challenge::default();
    OsRng.fill_bytes(&mut challenge);
    challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_what_a_client_encrypted() {
        let keypair = ServerKeyPair::generate();
        let secret: SharedSecret = [3; SHARED_SECRET_LEN];
        let encrypted = encrypt_to_server(keypair.public_key_der(), &secret).expect("encrypt");
        assert_ne!(encrypted, secret);
        assert_eq!(
            keypair.decrypt_shared_secret(&encrypted).expect("decrypt"),
            secret
        );
    }

    /// 公钥是 X.509 SubjectPublicKeyInfo，1024 位的 RSA 公钥一共 162 个字节，
    /// 和 Java `PublicKey.getEncoded()` 的长度一致。
    #[test]
    fn publishes_an_x509_public_key() {
        let der = ServerKeyPair::generate().public_key_der().to_vec();
        assert_eq!(der.len(), 162);
        assert_eq!(der[0], 0x30, "SubjectPublicKeyInfo 是一个 DER SEQUENCE");
        RsaPublicKey::from_public_key_der(&der).expect("公钥应该能被解析回来");
    }

    #[test]
    fn rejects_data_encrypted_for_someone_else() {
        let keypair = ServerKeyPair::generate();
        let other = ServerKeyPair::generate();
        let encrypted =
            encrypt_to_server(other.public_key_der(), &[1; SHARED_SECRET_LEN]).expect("encrypt");
        assert!(matches!(
            keypair.decrypt(&encrypted),
            Err(CryptError::Decrypt)
        ));
    }

    #[test]
    fn rejects_malformed_ciphertexts() {
        let keypair = ServerKeyPair::generate();
        assert!(matches!(keypair.decrypt(&[]), Err(CryptError::Decrypt)));
        assert!(matches!(keypair.decrypt(&[0; 7]), Err(CryptError::Decrypt)));
        assert!(matches!(
            keypair.decrypt(&[0xff; 128]),
            Err(CryptError::Decrypt)
        ));
        assert!(matches!(
            keypair.decrypt(&[0; 4096]),
            Err(CryptError::Decrypt)
        ));
    }

    /// 长度不对的密钥要被认出来，不能当成 16 个字节用。
    #[test]
    fn rejects_a_shared_secret_of_the_wrong_length() {
        let keypair = ServerKeyPair::generate();
        let encrypted = encrypt_to_server(keypair.public_key_der(), &[5; 8]).expect("encrypt");
        assert!(matches!(
            keypair.decrypt_shared_secret(&encrypted),
            Err(CryptError::SecretLength(8))
        ));
    }

    #[test]
    fn rejects_a_public_key_that_is_not_a_key() {
        assert!(matches!(
            encrypt_to_server(b"not a key", &[0; SHARED_SECRET_LEN]),
            Err(CryptError::PublicKey(_))
        ));
    }

    #[test]
    fn challenges_are_not_all_the_same() {
        assert_ne!(random_challenge(), random_challenge());
    }
}
