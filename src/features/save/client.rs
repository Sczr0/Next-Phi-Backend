use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::decryptor::{CipherSuite, DEFAULT_IV, DecryptionMeta, KdfSpec};

use crate::error::SaveProviderError;

const USER_AGENT: &str = "LeanCloud-CSharp-SDK/1.0.3";

fn clamp_pbkdf2_rounds(rounds: u32) -> u32 {
    let cfg = &crate::config::AppConfig::global().save;
    let min = cfg.pbkdf2_rounds_min;
    let max = cfg.pbkdf2_rounds_max.max(min);
    rounds.clamp(min, max)
}

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
    #[must_use]
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

/// GET /users/me 的响应（只需要 objectId 用于构造 where 过滤）
#[derive(Debug, Deserialize)]
struct UserMeResponse {
    #[serde(rename = "objectId")]
    object_id: String,
}

#[derive(Debug, Deserialize)]
struct SaveInfoResult {
    #[serde(rename = "objectId")]
    object_id: String,
    summary: String,
    /// 可能缺失（phi-plugin 中 `if (!item?.gameFile) continue`），需要过滤
    #[serde(rename = "gameFile", default)]
    game_file: Option<GameFile>,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    /// LeanCloud 日期对象 `{"__type":"Date","iso":"..."}`，用于取最新存档
    #[serde(rename = "modifiedAt", default)]
    modified_at: Option<LeancloudDate>,
    #[serde(default)]
    user: Option<SaveUser>,
    #[serde(default)]
    crypto: Option<SaveCryptoMeta>,
}

#[derive(Debug, Deserialize)]
struct SaveUser {
    #[serde(rename = "objectId")]
    object_id: String,
}

#[derive(Debug, Deserialize)]
struct GameFile {
    #[serde(rename = "objectId")]
    _object_id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct SaveCryptoMeta {
    #[serde(default)]
    crypto: Option<CryptoSpec>,
    #[serde(default)]
    _etag: Option<String>,
    #[serde(default)]
    _length: Option<u64>,
    #[serde(default)]
    _compressed: Option<bool>,
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
    _key_hex: Option<String>,
    #[serde(default)]
    _tag_hex: Option<String>,
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

/// 第一步：GET /users/me 获取当前玩家 objectId（对齐 phi-plugin 的 getPlayerInfo）
async fn fetch_user_object_id(
    client: &reqwest::Client,
    tap_config: &crate::config::TapTapConfig,
    session_token: &str,
) -> Result<String, SaveProviderError> {
    let url = format!("{}/users/me", tap_config.leancloud_base_url);
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
            "获取玩家信息失败: {}",
            response.status()
        )));
    }
    let me: UserMeResponse = response.json().await?;
    Ok(me.object_id)
}

/// RFC 3986 unreserved 之外的所有字节做百分号编码（用于 LeanCloud where 查询参数）
fn percent_encode_query(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// 排序 key：modifiedAt.iso → 毫秒时间戳；缺失/解析失败为 None（排最后）
fn modified_at_sort_key(r: &SaveInfoResult) -> Option<i64> {
    r.modified_at
        .as_ref()
        .and_then(|d| d.iso.as_deref())
        .and_then(|iso| chrono::DateTime::parse_from_rfc3339(iso).ok())
        .map(|dt| dt.timestamp_millis())
}

pub async fn fetch_from_official(
    session_token: &str,
    config: &crate::config::TapTapMultiConfig,
    version: Option<&str>,
) -> Result<(String, DecryptionMeta, Option<String>, Option<String>), SaveProviderError> {
    let t_total = Instant::now();
    let client = crate::http::client_timeout_30s()?;

    let tap_config = config.resolve(version);

    // 对齐 phi-plugin 的两步获取流程：
    // 1) GET /users/me 拿当前玩家 objectId
    // 2) GET /gamesaves/?skip=0&limit=100&where={user Pointer}&include=cover,gameFile
    //    按 modifiedAt.iso 倒序取最新一份
    let user_object_id = fetch_user_object_id(client, tap_config, session_token).await?;

    // 国服: 2025-06-08 TapTap 将 _GameSave 端点迁移至 /gamesaves/
    let is_cn = tap_config
        .leancloud_base_url
        .contains("rak3ffdi.cloud.tds1.tapapis.cn");
    let path = if is_cn {
        "/gamesaves/"
    } else {
        "/classes/_GameSave"
    };
    // where 按当前用户过滤 + limit=100，与 phi-plugin 的 saveArray 一致
    let where_json = serde_json::json!({
        "user": {
            "__type": "Pointer",
            "className": "_User",
            "objectId": user_object_id,
        }
    });
    let url = format!(
        "{}{}?skip=0&limit=100&where={}&include=cover,gameFile",
        tap_config.leancloud_base_url,
        path,
        percent_encode_query(&where_json.to_string())
    );

    let t_http = Instant::now();
    let response = client
        .get(&url)
        .header("X-LC-Id", &tap_config.leancloud_app_id)
        .header("X-LC-Key", &tap_config.leancloud_app_key)
        .header("X-LC-Session", session_token)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    let http_ms = t_http.elapsed().as_millis();

    if !response.status().is_success() {
        tracing::info!(
            target: "phi_backend::save::performance",
            phase = "fetch_from_official",
            provider = "official",
            status = "error",
            status_code = response.status().as_u16(),
            dur_ms = http_ms,
            total_dur_ms = t_total.elapsed().as_millis(),
            "save provider performance"
        );
        return Err(SaveProviderError::Auth(format!(
            "API 请求失败: {}",
            response.status()
        )));
    }

    let save_info: SaveInfoResponse = response.json().await?;
    // 对齐 phi-plugin：过滤掉没有 gameFile 的条目
    let mut candidates: Vec<SaveInfoResult> = save_info
        .results
        .into_iter()
        .filter(|r| r.game_file.is_some())
        .filter(|r| {
            // 0608 事件残留的异常存档（where 已按当前用户过滤，正常不会触发，保留兜底）
            if is_cn
                && r.user
                    .as_ref()
                    .is_some_and(|u| u.object_id == "6a265effd774134774ac90d6")
            {
                tracing::warn!(
                    target: "phi_backend::save::client",
                    save_object_id = %r.object_id,
                    "跳过异常存档 (bad user objectId)"
                );
                return false;
            }
            true
        })
        .collect();

    // 对齐 phi-plugin：按 modifiedAt.iso 倒序取最新一份；缺失时间戳的排最后
    candidates.sort_by_key(|r| std::cmp::Reverse(modified_at_sort_key(r)));

    let result = candidates
        .into_iter()
        .next()
        .ok_or_else(|| SaveProviderError::Metadata("未找到存档".to_string()))?;

    let game_file = result
        .game_file
        .as_ref()
        .ok_or_else(|| SaveProviderError::Metadata("未找到存档".to_string()))?;
    let download_url = if game_file.url.starts_with("http") {
        game_file.url.clone()
    } else {
        format!("https://{}", game_file.url)
    };
    let summary_b64 = Some(result.summary);
    let updated_at = Some(result.updated_at);

    let mut meta = DecryptionMeta::default();
    if let Some(meta_root) = result.crypto
        && let Some(crypto) = meta_root.crypto
    {
        if let Some(mode) = crypto.mode {
            match mode.as_str() {
                "aes-256-cbc" | "AES-256-CBC" => {
                    if let Some(iv_hex) = crypto.iv_hex
                        && let Ok(iv) = hex::decode(iv_hex)
                        && iv.len() == 16
                    {
                        let mut iv_arr = [0u8; 16];
                        iv_arr.copy_from_slice(&iv);
                        meta.cipher = CipherSuite::Aes256CbcPkcs7 { iv: iv_arr };
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

        if let Some(kdf) = crypto.kdf
            && let Some(kind) = kdf.kind
            && kind.eq_ignore_ascii_case("pbkdf2-sha1")
        {
            let salt = kdf
                .salt_hex
                .and_then(|h| hex::decode(h).ok())
                .unwrap_or_default();
            let raw_rounds = kdf.rounds.unwrap_or(1000);
            let rounds = clamp_pbkdf2_rounds(raw_rounds);
            if rounds != raw_rounds {
                tracing::warn!(
                    target: "phi_backend::save::client",
                    raw_rounds,
                    rounds,
                    "pbkdf2 rounds 超出配置范围，已自动收敛"
                );
            }
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

    tracing::info!(
        target: "phi_backend::save::performance",
        phase = "fetch_from_official",
        provider = "official",
        status = "ok",
        status_code = 200_u16,
        dur_ms = http_ms,
        total_dur_ms = t_total.elapsed().as_millis(),
        "save provider performance"
    );
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
    let t_total = Instant::now();
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

    let client = crate::http::client_timeout_30s()?;

    let t_http = Instant::now();
    let response = client
        .post("https://phib19.top:8080/get/cloud/saves")
        .json(&request_body)
        .send()
        .await?;
    let http_ms = t_http.elapsed().as_millis();

    if !response.status().is_success() {
        tracing::info!(
            target: "phi_backend::save::performance",
            phase = "fetch_from_external",
            provider = "external",
            status = "error",
            status_code = response.status().as_u16(),
            dur_ms = http_ms,
            total_dur_ms = t_total.elapsed().as_millis(),
            "save provider performance"
        );
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
        if updated_at.is_none()
            && let Some(md) = info.modified_at.and_then(|d| d.iso)
        {
            updated_at = Some(md);
        }
        if updated_at.is_none() {
            updated_at = info.game_file.and_then(|g| g.updated_at);
        }
    }
    if updated_at.is_none() {
        updated_at = api_response.data.summary.and_then(|s| s.updated_at);
    }
    tracing::info!(
        target: "phi_backend::save::performance",
        phase = "fetch_from_external",
        provider = "external",
        status = "ok",
        status_code = 200_u16,
        dur_ms = http_ms,
        total_dur_ms = t_total.elapsed().as_millis(),
        "save provider performance"
    );
    Ok((api_response.data.save_url, updated_at))
}

#[cfg(test)]
mod tests {
    use super::{
        LeancloudDate, SaveInfoResult, clamp_pbkdf2_rounds, modified_at_sort_key,
        percent_encode_query,
    };

    fn ensure_config_initialized() {
        let _ = crate::config::AppConfig::init_global();
    }

    #[test]
    fn percent_encode_query_encodes_json_specials() {
        let json = r##"{"user":{"__type":"Pointer","className":"_User","objectId":"abc123"}}"##;
        let encoded = percent_encode_query(json);
        assert_eq!(
            encoded,
            "%7B%22user%22%3A%7B%22__type%22%3A%22Pointer%22%2C%22className%22%3A%22_User%22%2C%22objectId%22%3A%22abc123%22%7D%7D"
        );
        // objectId 这类字母数字保持不变
        assert!(encoded.contains("abc123"));
    }

    fn sample_result(iso: Option<&str>) -> SaveInfoResult {
        SaveInfoResult {
            object_id: "id".to_string(),
            summary: String::new(),
            game_file: None,
            updated_at: String::new(),
            modified_at: iso.map(|iso| LeancloudDate {
                _type: Some("Date".to_string()),
                iso: Some(iso.to_string()),
            }),
            user: None,
            crypto: None,
        }
    }

    #[test]
    fn modified_at_sort_key_parses_rfc3339() {
        let newer = sample_result(Some("2025-08-01T00:00:00.000Z"));
        let older = sample_result(Some("2025-01-01T00:00:00.000Z"));
        let missing = sample_result(None);
        let bad = sample_result(Some("not-a-date"));

        // 最新的排最前（Reverse + Option: None 排最后）
        let mut v = vec![older, missing, newer, bad];
        v.sort_by_key(|r| std::cmp::Reverse(modified_at_sort_key(r)));
        assert_eq!(
            v[0].modified_at.as_ref().unwrap().iso.as_deref(),
            Some("2025-08-01T00:00:00.000Z")
        );
        assert_eq!(
            v[1].modified_at.as_ref().unwrap().iso.as_deref(),
            Some("2025-01-01T00:00:00.000Z")
        );
        // 缺失/无效时间戳排最后
        assert!(
            v[2].modified_at.is_none()
                || v[2].modified_at.as_ref().unwrap().iso.as_deref() == Some("not-a-date")
        );
        assert!(
            v[3].modified_at.is_none()
                || v[3].modified_at.as_ref().unwrap().iso.as_deref() == Some("not-a-date")
        );
    }

    #[test]
    fn clamp_pbkdf2_rounds_applies_config_bounds() {
        ensure_config_initialized();
        // 依赖当前默认配置边界：1000..=100000
        let low = clamp_pbkdf2_rounds(1);
        let ok = clamp_pbkdf2_rounds(5000);
        let high = clamp_pbkdf2_rounds(1_000_000);
        assert!(low >= 1000);
        assert_eq!(ok, 5000);
        assert!(high <= 100_000);
    }
}
