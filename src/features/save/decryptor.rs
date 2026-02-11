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
    encrypted_data: Vec<u8>,
    meta: &DecryptionMeta,
) -> Result<Vec<u8>, SaveProviderError> {
    decrypt_zip_entry_with_derived_key(encrypted_data, meta, None)
}

pub fn decrypt_zip_entry_with_derived_key(
    mut encrypted_data: Vec<u8>,
    meta: &DecryptionMeta,
    derived_key: Option<&[u8; 32]>,
) -> Result<Vec<u8>, SaveProviderError> {
    if encrypted_data.is_empty() {
        return Err(SaveProviderError::InvalidHeader);
    }
    let prefix = encrypted_data[0];

    match &meta.cipher {
        CipherSuite::Aes256CbcPkcs7 { iv } => {
            // KdfSpec::None 时直接复用默认 key，避免一次 Vec 分配。
            let key_arr = if let Some(pre) = derived_key {
                *pre
            } else {
                let mut key_arr = DEFAULT_KEY;
                if matches!(&meta.kdf, KdfSpec::Pbkdf2Sha1 { .. }) {
                    let key_bytes = derive_key(&meta.kdf, 32)?;
                    key_arr.copy_from_slice(&key_bytes);
                }
                key_arr
            };
            // AES-CBC 需要可变 buffer；这里直接复用 zip entry 的 Vec，避免一次 `to_vec()` 拷贝
            let ciphertext = encrypted_data
                .get_mut(1..)
                .ok_or(SaveProviderError::InvalidHeader)?;
            let decrypted_len = decrypt_aes256_cbc_in_place(ciphertext, &key_arr, iv)?;
            encrypted_data.truncate(1 + decrypted_len);
            Ok(encrypted_data)
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
            let mut buf = Vec::with_capacity(ct.len() + tag.len());
            buf.extend_from_slice(ct);
            buf.extend_from_slice(tag);
            #[allow(deprecated)]
            let nonce_array = aes_gcm::Nonce::from_exact_iter(nonce.iter().copied())
                .ok_or_else(|| SaveProviderError::Decrypt("nonce 长度无效".into()))?;
            let pt = aead
                .decrypt(
                    &nonce_array,
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

fn decrypt_aes256_cbc_in_place(
    ciphertext: &mut [u8],
    key: &[u8; 32],
    iv: &[u8; 16],
) -> Result<usize, SaveProviderError> {
    if ciphertext.is_empty() {
        return Err(SaveProviderError::Decrypt("空密文".into()));
    }
    use cipher::block_padding::Pkcs7;
    type Aes256CbcDec = CbcDecryptor<Aes256>;
    let dec = Aes256CbcDec::new(key.into(), iv.into());
    let decrypted = dec
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| SaveProviderError::Decrypt(format!("AES 解密失败: {e:?}")))?;
    // `decrypted` 指向 ciphertext 的前缀区域，长度即为去除 PKCS#7 padding 后的明文长度
    Ok(decrypted.len())
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
