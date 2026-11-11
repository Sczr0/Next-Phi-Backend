//! 委托授权的加密与随机值工具。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn new_token(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

pub fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

fn decode_key(key_hex: &str) -> Result<[u8; 32], AppError> {
    let bytes = hex::decode(key_hex.trim())
        .map_err(|_| AppError::Internal("delegation credential_key 必须是 hex".into()))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AppError::Internal("delegation credential_key 必须为 32 字节".into()))?;
    Ok(key)
}

pub fn seal_credential(
    key_hex: &str,
    delegation_id: &str,
    plaintext: &str,
) -> Result<String, AppError> {
    let key = decode_key(key_hex)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| AppError::Internal("delegation credential_key 无效".into()))?;
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext.as_bytes(),
                aad: delegation_id.as_bytes(),
            },
        )
        .map_err(|_| AppError::Internal("delegation 凭证加密失败".into()))?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(URL_SAFE_NO_PAD.encode(out))
}

pub fn open_credential(
    key_hex: &str,
    delegation_id: &str,
    sealed: &str,
) -> Result<String, AppError> {
    let key = decode_key(key_hex)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| AppError::Internal("delegation credential_key 无效".into()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(sealed)
        .map_err(|_| AppError::Auth("delegation 凭证损坏".into()))?;
    if bytes.len() < 13 {
        return Err(AppError::Auth("delegation 凭证损坏".into()));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: delegation_id.as_bytes(),
            },
        )
        .map_err(|_| AppError::Auth("delegation 凭证无法解密".into()))?;
    String::from_utf8(plaintext).map_err(|_| AppError::Auth("delegation 凭证不是 UTF-8".into()))
}

pub fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    if verifier.len() < 43 || verifier.len() > 128 {
        return false;
    }
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = URL_SAFE_NO_PAD.encode(hasher.finalize());
    constant_time_eq(digest.as_bytes(), challenge.as_bytes())
}

pub fn scopes_contain(granted: &str, required: &str) -> bool {
    granted.split_whitespace().any(|scope| scope == required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_round_trip_and_aad_binding() {
        let key = "11".repeat(32);
        let sealed = seal_credential(&key, "delegation-1", "r:session").expect("seal");
        assert_eq!(
            open_credential(&key, "delegation-1", &sealed).expect("open"),
            "r:session"
        );
        assert!(open_credential(&key, "delegation-2", &sealed).is_err());
    }

    #[test]
    fn pkce_matches_s256() {
        let verifier = "a".repeat(43);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert!(verify_pkce(&verifier, &challenge));
        assert!(!verify_pkce(&verifier, "wrong"));
    }
}
