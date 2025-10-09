use aes::Aes256;
use aes_gcm::{
    Aes128Gcm, KeyInit as _,
    aead::{Aead, Payload},
};
use cbc::{
    Decryptor as CbcDecryptor,
    cipher::{BlockDecryptMut, KeyIvInit},
};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha256;

use crate::error::SaveProviderError;

pub const DEFAULT_KEY: [u8; 32] = [
    0xe8, 0x96, 0x9a, 0xd2, 0xa5, 0x40, 0x25, 0x9b, 0x97, 0x91, 0x90, 0x8b, 0x88, 0xe6, 0xbf, 0x03,
    0x1e, 0x6d, 0x21, 0x95, 0x6e, 0xfa, 0xd6, 0x8a, 0x50, 0xdd, 0x55, 0xd6, 0x7a, 0xb0, 0x92, 0x4b,
];

pub const DEFAULT_IV: [u8; 16] = [
    0x2a, 0x4f, 0xf0, 0x8a, 0xc8, 0x0d, 0x63, 0x07, 0x00, 0x57, 0xc5, 0x95, 0x18, 0xc8, 0x32, 0x53,
];

#[derive(Debug, Clone)]
pub enum CipherSuite {
    Aes256CbcPkcs7 { iv: [u8; 16] },
    Aes128Gcm { nonce: Vec<u8>, tag_len: usize },
}

#[derive(Debug, Clone)]
pub enum KdfSpec {
    None,
    Pbkdf2Sha1 {
        salt: Vec<u8>,
        rounds: u32,
        password: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub enum Integrity {
    None,
    HmacSha1 { key: Vec<u8> },
    HmacSha256 { key: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct DecryptionMeta {
    pub cipher: CipherSuite,
    pub kdf: KdfSpec,
    pub integrity: Integrity,
}

impl Default for DecryptionMeta {
    fn default() -> Self {
        Self {
            cipher: CipherSuite::Aes256CbcPkcs7 { iv: DEFAULT_IV },
            kdf: KdfSpec::None,
            integrity: Integrity::None,
        }
    }
}

pub fn decrypt_zip_entry(
    encrypted_data: &[u8],
    meta: &DecryptionMeta,
) -> Result<Vec<u8>, SaveProviderError> {
    if encrypted_data.is_empty() {
        return Err(SaveProviderError::InvalidHeader);
    }
    let prefix = encrypted_data[0];

    match &meta.cipher {
        CipherSuite::Aes256CbcPkcs7 { iv } => {
            let key_bytes = match &meta.kdf {
                KdfSpec::None => DEFAULT_KEY.to_vec(),
                KdfSpec::Pbkdf2Sha1 { .. } => derive_key(&meta.kdf, 32)?,
            };
            let mut key_arr = [0u8; 32];
            key_arr.copy_from_slice(&key_bytes);
            let ciphertext = &encrypted_data[1..];
            let mut out = Vec::with_capacity(1 + ciphertext.len());
            out.push(prefix);
            let rest = decrypt_aes256_cbc(ciphertext, &key_arr, iv)?;
            out.extend_from_slice(&rest);
            Ok(out)
        }
        CipherSuite::Aes128Gcm { nonce, tag_len } => {
            if encrypted_data.len() <= 1 + *tag_len {
                return Err(SaveProviderError::InvalidHeader);
            }
            if nonce.len() != 12 {
                return Err(SaveProviderError::Unsupported(
                    "AES-GCM nonce 必须为 12 字节".into(),
                ));
            }
            let ct_end = encrypted_data.len() - *tag_len;
            let ct = &encrypted_data[1..ct_end];
            let tag = &encrypted_data[ct_end..];
            let aead = Aes128Gcm::new_from_slice(&DEFAULT_KEY[..16])
                .map_err(|e| SaveProviderError::Decrypt(format!("GCM 初始化失败: {e}")))?;
            let mut buf = ct.to_vec();
            buf.extend_from_slice(tag);
            let pt = aead
                .decrypt(
                    aes_gcm::Nonce::from_slice(nonce),
                    Payload {
                        msg: &buf,
                        aad: &[],
                    },
                )
                .map_err(|_| SaveProviderError::TagVerification)?;
            let mut out = Vec::with_capacity(1 + pt.len());
            out.push(prefix);
            out.extend_from_slice(&pt);
            Ok(out)
        }
    }
}

fn decrypt_aes256_cbc(
    ciphertext: &[u8],
    key: &[u8; 32],
    iv: &[u8; 16],
) -> Result<Vec<u8>, SaveProviderError> {
    if ciphertext.is_empty() {
        return Err(SaveProviderError::Decrypt("空密文".into()));
    }
    use cipher::block_padding::Pkcs7;
    type Aes256CbcDec = CbcDecryptor<Aes256>;
    let dec = Aes256CbcDec::new(key.into(), iv.into());
    let mut buffer = ciphertext.to_vec();
    let decrypted = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|e| SaveProviderError::Decrypt(format!("AES 解密失败: {e:?}")))?;
    Ok(decrypted.to_vec())
}

pub fn verify_integrity(
    data: &[u8],
    integrity: &Integrity,
    provided_tag: Option<&[u8]>,
) -> Result<(), SaveProviderError> {
    match integrity {
        Integrity::None => Ok(()),
        Integrity::HmacSha1 { key } => {
            let tag = provided_tag
                .ok_or_else(|| SaveProviderError::Integrity("HMAC 标签缺失".to_string()))?;
            type H = Hmac<Sha1>;
            let mut mac = <H as Mac>::new_from_slice(key)
                .map_err(|e| SaveProviderError::Integrity(format!("HMAC 初始化失败: {e}")))?;
            mac.update(data);
            mac.verify_slice(tag)
                .map_err(|_| SaveProviderError::Integrity("HMAC-SHA1 验证失败".to_string()))
        }
        Integrity::HmacSha256 { key } => {
            let tag = provided_tag
                .ok_or_else(|| SaveProviderError::Integrity("HMAC 标签缺失".to_string()))?;
            type H = Hmac<Sha256>;
            let mut mac = <H as Mac>::new_from_slice(key)
                .map_err(|e| SaveProviderError::Integrity(format!("HMAC 初始化失败: {e}")))?;
            mac.update(data);
            mac.verify_slice(tag)
                .map_err(|_| SaveProviderError::Integrity("HMAC-SHA256 验证失败".to_string()))
        }
    }
}

pub fn derive_key(kdf: &KdfSpec, desired_len: usize) -> Result<Vec<u8>, SaveProviderError> {
    match kdf {
        KdfSpec::None => Ok(DEFAULT_KEY.to_vec()),
        KdfSpec::Pbkdf2Sha1 {
            salt,
            rounds,
            password,
        } => {
            let mut out = vec![0u8; desired_len];
            pbkdf2_hmac::<Sha1>(password, salt, *rounds, &mut out);
            Ok(out)
        }
    }
}
