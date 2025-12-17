use std::collections::HashMap;
use std::io::{Cursor, Read};

use axum::body::Bytes;
use flate2::read::{GzDecoder, ZlibDecoder};

use super::client::{self, ExternalApiCredentials};
use super::decryptor::{DecryptionMeta, decrypt_zip_entry};
use super::models::DifficultyRecord;
use super::record_parser;
use super::summary_parser::{SummaryParsed, parse_summary_base64};
use crate::error::SaveProviderError;
use crate::startup::chart_loader::ChartConstantsMap;

/// 存档元信息（用于缓存前移：先拿 updatedAt 再决定是否需要下载/解密）
#[derive(Debug, Clone)]
pub struct SaveMeta {
    pub download_url: String,
    pub decrypt_meta: DecryptionMeta,
    pub summary_b64: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParsedSave {
    pub game_record: HashMap<String, Vec<DifficultyRecord>>,
    pub game_progress: serde_json::Value,
    pub user: serde_json::Value,
    pub settings: serde_json::Value,
    pub game_key: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none", rename = "summaryParsed")]
    pub summary_parsed: Option<SummaryParsed>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SaveSource {
    Official { session_token: String },
    ExternalApi { credentials: ExternalApiCredentials },
}

impl SaveSource {
    pub fn official(session_token: impl Into<String>) -> Self {
        Self::Official {
            session_token: session_token.into(),
        }
    }
    pub fn external(credentials: ExternalApiCredentials) -> Self {
        Self::ExternalApi { credentials }
    }
}

/// 仅获取存档元信息（download_url / 解密参数 / updatedAt / summary），不下载存档本体。
///
/// 用途：
/// - 生成缓存 Key（updatedAt 作为版本号）
/// - 缓存命中时，避免走“下载+解密+解析”全流程
pub async fn fetch_save_meta(
    source: SaveSource,
    taptap_config: &crate::config::TapTapMultiConfig,
    version: Option<&str>,
) -> Result<SaveMeta, SaveProviderError> {
    let (download_url, decrypt_meta, summary_b64, updated_at) = match source {
        SaveSource::Official { session_token } => {
            client::fetch_from_official(&session_token, taptap_config, version).await?
        }
        SaveSource::ExternalApi { credentials } => {
            if let Some(s) = credentials.sessiontoken.clone() {
                // 外部凭证若包含 sessiontoken，则直接走官方元信息接口（能拿到 updatedAt/summary/crypto）
                client::fetch_from_official(&s, taptap_config, version).await?
            } else {
                let (url, ext_updated_at) = client::fetch_from_external(&credentials).await?;
                (url, DecryptionMeta::default(), None, ext_updated_at)
            }
        }
    };

    Ok(SaveMeta {
        download_url,
        decrypt_meta,
        summary_b64,
        updated_at,
    })
}

pub async fn get_decrypted_save(
    source: SaveSource,
    chart_constants: &ChartConstantsMap,
    taptap_config: &crate::config::TapTapMultiConfig,
    version: Option<&str>,
) -> Result<ParsedSave, SaveProviderError> {
    let meta = fetch_save_meta(source, taptap_config, version).await?;
    get_decrypted_save_from_meta(meta, chart_constants).await
}

/// 使用已获取的元信息下载/解密/解析存档（用于缓存前移后的 miss 路径，避免重复请求元信息接口）。
pub async fn get_decrypted_save_from_meta(
    meta: SaveMeta,
    chart_constants: &ChartConstantsMap,
) -> Result<ParsedSave, SaveProviderError> {
    let SaveMeta {
        download_url,
        decrypt_meta,
        summary_b64,
        updated_at,
    } = meta;

    let encrypted_bytes = download_encrypted_save(&download_url).await?;

    // 注意：解压/解密/解析属于 CPU/内存密集型同步任务，为避免阻塞 Tokio worker，这里 offload 到 blocking 线程池。
    let join = tokio::task::spawn_blocking(move || {
        let zip_bytes = try_decompress(encrypted_bytes)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))?;

        let mut decrypted_entries: HashMap<String, Vec<u8>> = HashMap::new();
        let expected = ["gameRecord", "gameKey", "gameProgress", "user", "settings"];
        for name in &expected {
            if let Ok(mut f) = archive.by_name(name) {
                // zip entry 通常携带 size 信息，预分配可以显著减少扩容次数。
                let mut enc = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut enc)?;
                let plain = decrypt_zip_entry(&enc, &decrypt_meta)?;
                decrypted_entries.insert((*name).to_string(), plain);
            }
        }

        super::parser::parse_save_to_json(&decrypted_entries)
    })
    .await;
    let json_value = match join {
        Ok(res) => res?,
        Err(e) => {
            // 为保持“异常情况下”的原有行为：如果 blocking 任务发生 panic，则继续向上传播 panic。
            let e_str = e.to_string();
            if let Ok(panic) = e.try_into_panic() {
                std::panic::resume_unwind(panic);
            }
            return Err(SaveProviderError::Io(format!(
                "spawn_blocking cancelled: {e_str}"
            )));
        }
    };

    // 解析结构化的 gameRecord 与其余部分
    let mut root = match json_value {
        serde_json::Value::Object(map) => map,
        _ => {
            return Err(SaveProviderError::Json(
                "root save json is not an object".to_string(),
            ));
        }
    };

    let game_record_val = root
        .remove("gameRecord")
        .ok_or_else(|| SaveProviderError::MissingField("gameRecord".to_string()))?;

    let game_record = record_parser::parse_game_record(&game_record_val, chart_constants)
        .map_err(|e| SaveProviderError::Json(format!("parse gameRecord failed: {e}")))?;

    let summary_parsed = summary_b64
        .as_deref()
        .and_then(|b64| parse_summary_base64(b64).ok());

    Ok(ParsedSave {
        game_record,
        game_progress: root
            .remove("gameProgress")
            .unwrap_or(serde_json::Value::Null),
        user: root.remove("user").unwrap_or(serde_json::Value::Null),
        settings: root.remove("settings").unwrap_or(serde_json::Value::Null),
        game_key: root.remove("gameKey").unwrap_or(serde_json::Value::Null),
        summary_parsed,
        updated_at,
    })
}

async fn download_encrypted_save(url: &str) -> Result<Bytes, SaveProviderError> {
    let client = crate::http::client_timeout_90s()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(SaveProviderError::Network(
            response.error_for_status().unwrap_err().to_string(),
        ));
    }
    Ok(response.bytes().await?)
}

fn try_decompress(bytes: Bytes) -> Result<Bytes, SaveProviderError> {
    if bytes.len() >= 2 && bytes[0] == 0x1F && bytes[1] == 0x8B {
        let mut gz = GzDecoder::new(bytes.as_ref());
        let mut out = Vec::new();
        gz.read_to_end(&mut out)?;
        return Ok(Bytes::from(out));
    }

    let mut z = ZlibDecoder::new(bytes.as_ref());
    let mut out = Vec::new();
    match z.read_to_end(&mut out) {
        Ok(_) => Ok(Bytes::from(out)),
        Err(_) => Ok(bytes),
    }
}
