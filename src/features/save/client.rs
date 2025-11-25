use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};

use super::decryptor::{CipherSuite, DEFAULT_IV, DecryptionMeta, KdfSpec};

use crate::error::SaveProviderError;

const USER_AGENT: &str = "LeanCloud-CSharp-SDK/1.0.3";

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalApiCredentials {
    /// 外部平台标识，如 "TapTap"/"Bilibili"（与 platformId 配对）
    #[schema(example = "TapTap")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// 外部平台用户唯一标识（与 platform 配对）
    #[schema(example = "user_123456")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<String>,
    /// 外部平台会话令牌（某些平台以此直连）
    #[schema(example = "ext-session-abcdef")] 
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessiontoken: Option<String>,
    /// 外部 API 的用户 ID（直连方式之一）
    #[schema(example = "1008611")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_user_id: Option<String>,
    /// 外部 API 的访问令牌（如需）
    #[schema(example = "token-xyz")] 
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
}

impl ExternalApiCredentials {
    pub fn is_valid(&self) -> bool {
        let has_platform_auth = self.platform.is_some() && self.platform_id.is_some();
        let has_session_auth = self.sessiontoken.is_some();
        let has_api_auth = self.api_user_id.is_some();
        has_platform_auth || has_session_auth || has_api_auth
    }
}

#[derive(Debug, Deserialize)]
struct SaveInfoResponse {
    results: Vec<SaveInfoResult>,
}

#[derive(Debug, Deserialize)]
struct SaveInfoResult {
    #[serde(rename = "objectId")]
    object_id: String,
    summary: String,
    #[serde(rename = "gameFile")]
    game_file: GameFile,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(default)]
    crypto: Option<SaveCryptoMeta>,
}

#[derive(Debug, Deserialize)]
struct GameFile {
    #[serde(rename = "objectId")]
    object_id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct SaveCryptoMeta {
    #[serde(default)]
    crypto: Option<CryptoSpec>,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    length: Option<u64>,
    #[serde(default)]
    compressed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CryptoSpec {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    iv_hex: Option<String>,
    #[serde(default)]
    nonce_hex: Option<String>,
    #[serde(default)]
    key_hex: Option<String>,
    #[serde(default)]
    tag_hex: Option<String>,
    #[serde(default)]
    tag_len: Option<usize>,
    #[serde(default)]
    kdf: Option<KdfFields>,
}

#[derive(Debug, Deserialize)]
struct KdfFields {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    salt_hex: Option<String>,
    #[serde(default)]
    rounds: Option<u32>,
    #[serde(default)]
    password_b64: Option<String>,
}

pub async fn fetch_from_official(
    session_token: &str,
    config: &crate::config::TapTapMultiConfig,
    version: Option<&str>,
) -> Result<(String, DecryptionMeta, Option<String>, Option<String>), SaveProviderError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    
    // 根据版本选择配置
    let tap_config = match version {
        Some("global") => &config.global,
        Some("cn") | None => &config.cn,
        _ => &config.cn,
    };

    let url = format!("{}/classes/_GameSave?limit=1", tap_config.leancloud_base_url);

    let response = client
        .get(&url)
        .header("X-LC-Id", &tap_config.leancloud_app_id)
        .header("X-LC-Key", &tap_config.leancloud_app_key)
        .header("X-LC-Session", session_token)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(SaveProviderError::Auth(format!(
            "API 请求失败: {}",
            response.status()
        )));
    }

    let save_info: SaveInfoResponse = response.json().await?;
    let result = save_info
        .results
        .into_iter()
        .next()
        .ok_or_else(|| SaveProviderError::Metadata("未找到存档".to_string()))?;

    let download_url = if result.game_file.url.starts_with("http") {
        result.game_file.url
    } else {
        format!("https://{}", result.game_file.url)
    };
    let summary_b64 = Some(result.summary);
    let updated_at = Some(result.updated_at);

    let mut meta = DecryptionMeta::default();
    if let Some(meta_root) = result.crypto {
        if let Some(crypto) = meta_root.crypto {
            if let Some(mode) = crypto.mode {
                match mode.as_str() {
                    "aes-256-cbc" | "AES-256-CBC" => {
                        if let Some(iv_hex) = crypto.iv_hex {
                            if let Ok(iv) = hex::decode(iv_hex) {
                                if iv.len() == 16 {
                                    let mut iv_arr = [0u8; 16];
                                    iv_arr.copy_from_slice(&iv);
                                    meta.cipher = CipherSuite::Aes256CbcPkcs7 { iv: iv_arr };
                                }
                            }
                        }
                    }
                    "aes-128-gcm" | "AES-128-GCM" => {
                        let nonce = if let Some(nh) = crypto.nonce_hex {
                            hex::decode(nh).unwrap_or_default()
                        } else if let Some(ivh) = crypto.iv_hex {
                            hex::decode(ivh).unwrap_or_default()
                        } else {
                            vec![]
                        };
                        let tag_len = crypto.tag_len.unwrap_or(16);
                        meta.cipher = CipherSuite::Aes128Gcm { nonce, tag_len };
                    }
                    _ => {}
                }
            }

            if let Some(kdf) = crypto.kdf {
                if let Some(kind) = kdf.kind {
                    if kind.eq_ignore_ascii_case("pbkdf2-sha1") {
                        let salt = kdf
                            .salt_hex
                            .and_then(|h| hex::decode(h).ok())
                            .unwrap_or_default();
                        let rounds = kdf.rounds.unwrap_or(1000);
                        let password = if let Some(b) = kdf.password_b64 {
                            general_purpose::STANDARD.decode(b).unwrap_or_default()
                        } else {
                            vec![]
                        };
                        meta.kdf = KdfSpec::Pbkdf2Sha1 {
                            salt,
                            rounds,
                            password,
                        };
                    }
                }
            }
        }
    }

    if let DecryptionMeta {
        cipher: CipherSuite::Aes256CbcPkcs7 { .. },
        ..
    } = &meta
    {
        // ok
    } else if matches!(meta.cipher, CipherSuite::Aes128Gcm { .. }) {
        // ok
    } else {
        meta.cipher = CipherSuite::Aes256CbcPkcs7 { iv: DEFAULT_IV };
    }

    Ok((download_url, meta, summary_b64, updated_at))
}

#[derive(Debug, Serialize)]
struct ExternalApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sessiontoken: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalApiResponse {
    data: ExternalApiData,
}

#[derive(Debug, Deserialize)]
struct ExternalApiData {
    #[serde(rename = "saveUrl")]
    save_url: String,
    #[serde(rename = "saveInfo")]
    save_info: Option<ExternalSaveInfo>,
    #[serde(default)]
    summary: Option<ExternalSummary>,
}

#[derive(Debug, Deserialize)]
struct ExternalSaveInfo {
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    #[serde(rename = "modifiedAt")]
    modified_at: Option<LeancloudDate>,
    #[serde(rename = "gameFile")]
    game_file: Option<ExternalGameFile>,
}

#[derive(Debug, Deserialize)]
struct LeancloudDate {
    #[serde(rename = "__type")]
    _type: Option<String>,
    iso: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalGameFile {
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalSummary {
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
}

pub async fn fetch_from_external(
    credentials: &ExternalApiCredentials,
) -> Result<(String, Option<String>), SaveProviderError> {
    if !credentials.is_valid() {
        return Err(SaveProviderError::InvalidCredentials(
            "必须提供以下凭证之一：platform + platform_id / sessiontoken / api_user_id".to_string(),
        ));
    }

    let request_body = ExternalApiRequest {
        platform: credentials.platform.clone(),
        platform_id: credentials.platform_id.clone(),
        sessiontoken: credentials.sessiontoken.clone(),
        api_user_id: credentials.api_user_id.clone(),
        api_token: credentials.api_token.clone(),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client
        .post("https://phib19.top:8080/get/cloud/saves")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(SaveProviderError::InvalidResponse(format!(
            "外部 API 请求失败: {}",
            response.status()
        )));
    }

    let api_response: ExternalApiResponse = response.json().await?;
    let mut updated_at: Option<String> = None;
    if let Some(info) = api_response.data.save_info {
        if updated_at.is_none() {
            updated_at = info.updated_at;
        }
        if updated_at.is_none() {
            if let Some(md) = info.modified_at.and_then(|d| d.iso) {
                updated_at = Some(md);
            }
        }
        if updated_at.is_none() {
            updated_at = info.game_file.and_then(|g| g.updated_at);
        }
    }
    if updated_at.is_none() {
        updated_at = api_response.data.summary.and_then(|s| s.updated_at);
    }
    Ok((api_response.data.save_url, updated_at))
}
