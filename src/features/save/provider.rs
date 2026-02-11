use std::collections::HashMap;
use std::io::{Cursor, Read};

use axum::body::Bytes;
use flate2::read::{GzDecoder, ZlibDecoder};
use futures_util::StreamExt;

use super::client::{self, ExternalApiCredentials};
use super::decryptor::{DecryptionMeta, decrypt_zip_entry_with_derived_key, derive_key};
use super::models::DifficultyRecord;
use super::record_parser;
use super::summary_parser::{SummaryParsed, parse_summary_base64};
use crate::error::SaveProviderError;
use crate::startup::chart_loader::ChartConstantsMap;

/// zip entry 的 size 可能来自元数据/上游返回值，直接用于 Vec::with_capacity 可能导致单次预分配过大。
/// 这里做一个守护性 cap：只影响预分配策略，不影响最终读取/解密/解析结果（不改变对外行为）。
const ZIP_ENTRY_PREALLOC_CAP: usize = 16 * 1024 * 1024;

/// try_decompress 的输出 Vec 做守护性预分配，减少常见场景下的 reallocate；
/// 仅影响内存分配策略，不改变“是否解压/如何回退”的语义（不改变对外行为）。
const DECOMPRESS_PREALLOC_CAP: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
struct SaveLimits {
    max_download_bytes: usize,
    max_decompress_bytes: usize,
    max_zip_entry_bytes: usize,
    max_zip_entries: usize,
}

impl SaveLimits {
    fn from_global() -> Result<Self, SaveProviderError> {
        let cfg = &crate::config::AppConfig::global().save;
        Ok(Self {
            max_download_bytes: usize::try_from(cfg.max_download_bytes)
                .map_err(|_| SaveProviderError::Io("max_download_bytes 超出平台 usize".into()))?,
            max_decompress_bytes: usize::try_from(cfg.max_decompress_bytes)
                .map_err(|_| SaveProviderError::Io("max_decompress_bytes 超出平台 usize".into()))?,
            max_zip_entry_bytes: usize::try_from(cfg.max_zip_entry_bytes)
                .map_err(|_| SaveProviderError::Io("max_zip_entry_bytes 超出平台 usize".into()))?,
            max_zip_entries: usize::try_from(cfg.max_zip_entries)
                .map_err(|_| SaveProviderError::Io("max_zip_entries 超出平台 usize".into()))?,
        })
    }
}

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
    let limits = SaveLimits::from_global()?;
    let SaveMeta {
        download_url,
        decrypt_meta,
        summary_b64,
        updated_at,
    } = meta;

    let encrypted_bytes = download_encrypted_save(&download_url, limits.max_download_bytes).await?;

    // 注意：解压/解密/解析属于 CPU/内存密集型同步任务，为避免阻塞 Tokio worker，这里 offload 到 blocking 线程池。
    let join = tokio::task::spawn_blocking(move || {
        let zip_bytes = try_decompress(encrypted_bytes, limits.max_decompress_bytes)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))?;

        // P1：PBKDF2 key 在单份 save 内只派生一次并复用，避免按 entry 重复派生。
        let derived_key_arr = if matches!(
            &decrypt_meta.kdf,
            super::decryptor::KdfSpec::Pbkdf2Sha1 { .. }
        ) {
            let key_bytes = derive_key(&decrypt_meta.kdf, 32)?;
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key_bytes);
            Some(arr)
        } else {
            None
        };

        let expected = ["gameRecord", "gameKey", "gameProgress", "user", "settings"];
        let mut decrypted_entries: HashMap<String, Vec<u8>> =
            HashMap::with_capacity(expected.len());
        let mut entry_count = 0usize;
        for name in &expected {
            if let Ok(mut f) = archive.by_name(name) {
                entry_count = entry_count.saturating_add(1);
                if entry_count > limits.max_zip_entries {
                    return Err(SaveProviderError::Io(format!(
                        "zip entry 数量超限: count={} limit={}",
                        entry_count, limits.max_zip_entries
                    )));
                }
                // zip entry 通常携带 size 信息，预分配可以显著减少扩容次数。
                let size = usize::try_from(f.size()).unwrap_or(usize::MAX);
                let mut enc = Vec::with_capacity(size.min(ZIP_ENTRY_PREALLOC_CAP));
                read_to_end_limited(
                    &mut f,
                    &mut enc,
                    limits.max_zip_entry_bytes,
                    "zip entry 读取超限",
                )?;
                let plain = decrypt_zip_entry_with_derived_key(
                    enc,
                    &decrypt_meta,
                    derived_key_arr.as_ref(),
                )?;
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

async fn download_encrypted_save(url: &str, max_bytes: usize) -> Result<Bytes, SaveProviderError> {
    let client = crate::http::client_timeout_90s()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(SaveProviderError::Network(
            response.error_for_status().unwrap_err().to_string(),
        ));
    }

    // 先按 Content-Length 快速拒绝（缺失或不可信时仍由流式累计兜底）
    if let Some(content_len) = response.content_length()
        && content_len as usize > max_bytes
    {
        return Err(SaveProviderError::Io(format!(
            "download too large: content-length={content_len} exceeds limit={max_bytes}"
        )));
    }

    let mut stream = response.bytes_stream();
    let mut out = Vec::with_capacity(max_bytes.min(DECOMPRESS_PREALLOC_CAP));
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        total = total.saturating_add(chunk.len());
        if total > max_bytes {
            return Err(SaveProviderError::Io(format!(
                "download too large: bytes={} exceeds limit={max_bytes}",
                total
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(out))
}

fn try_decompress(bytes: Bytes, max_decompress_bytes: usize) -> Result<Bytes, SaveProviderError> {
    // 快速识别：ZIP（PK..）/GZIP（1F 8B），避免不必要的解压尝试。
    // 决策：保留“回退机制”。即：识别失败或解压失败时，不报错，直接按 Raw Bytes 处理（保持现有行为）。

    let raw = bytes.as_ref();

    // ZIP 魔数：PK\x03\x04 / PK\x05\x06 / PK\x07\x08
    if raw.len() >= 4
        && raw[0] == b'P'
        && raw[1] == b'K'
        && matches!((raw[2], raw[3]), (3, 4) | (5, 6) | (7, 8))
    {
        return Ok(bytes);
    }

    // GZIP 魔数：1F 8B
    if raw.len() >= 2 && raw[0] == 0x1F && raw[1] == 0x8B {
        let mut gz = GzDecoder::new(raw);
        let mut out = Vec::with_capacity(raw.len().min(DECOMPRESS_PREALLOC_CAP));
        if read_to_end_limited(&mut gz, &mut out, max_decompress_bytes, "gzip 解压输出超限").is_ok()
        {
            return Ok(Bytes::from(out));
        }
        return Ok(bytes);
    }

    // 其他：尝试 Zlib，失败则回退 Raw Bytes。
    let mut z = ZlibDecoder::new(raw);
    let mut out = Vec::with_capacity(raw.len().min(DECOMPRESS_PREALLOC_CAP));
    match read_to_end_limited(&mut z, &mut out, max_decompress_bytes, "zlib 解压输出超限") {
        Ok(_) => Ok(Bytes::from(out)),
        Err(_) => Ok(bytes),
    }
}

fn read_to_end_limited<R: Read>(
    reader: &mut R,
    out: &mut Vec<u8>,
    limit: usize,
    err_label: &str,
) -> Result<(), SaveProviderError> {
    let mut buf = [0u8; 8192];
    let mut total = out.len();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > limit {
            return Err(SaveProviderError::Io(format!(
                "{err_label}: bytes={} exceeds limit={limit}",
                total
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::{GzEncoder, ZlibEncoder};
    use std::io::Write;

    #[test]
    fn try_decompress_zip_magic_returns_raw() {
        let raw = Bytes::from_static(b"PK\x03\x04this-is-zip");
        let out = try_decompress(raw.clone(), usize::MAX).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn try_decompress_gzip_success_returns_decompressed() {
        let payload = b"hello-gzip";
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(payload).unwrap();
        let gz = Bytes::from(enc.finish().unwrap());
        assert_ne!(gz.as_ref(), payload);

        let out = try_decompress(gz, usize::MAX).unwrap();
        assert_eq!(out.as_ref(), payload);
    }

    #[test]
    fn try_decompress_gzip_failure_falls_back_to_raw() {
        // 伪造 gzip 魔数但内容非法，必须回退为原 bytes（不报错）。
        let raw = Bytes::from_static(b"\x1f\x8b\x00\x00\x00\x00\x00");
        let out = try_decompress(raw.clone(), usize::MAX).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn try_decompress_zlib_success_returns_decompressed() {
        let payload = b"hello-zlib";
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(payload).unwrap();
        let z = Bytes::from(enc.finish().unwrap());
        assert_ne!(z.as_ref(), payload);

        let out = try_decompress(z, usize::MAX).unwrap();
        assert_eq!(out.as_ref(), payload);
    }

    #[test]
    fn try_decompress_unknown_header_falls_back_to_raw() {
        let raw = Bytes::from_static(b"not-gzip-not-zip-not-zlib");
        let out = try_decompress(raw.clone(), usize::MAX).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn try_decompress_gzip_over_limit_falls_back_to_raw() {
        let payload = vec![b'a'; 64];
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload).unwrap();
        let gz = Bytes::from(enc.finish().unwrap());

        let out = try_decompress(gz.clone(), 16).unwrap();
        assert_eq!(out, gz);
    }

    #[test]
    fn read_to_end_limited_returns_error_when_exceeds_limit() {
        let data = vec![1u8; 32];
        let mut cursor = Cursor::new(data);
        let mut out = Vec::new();
        let err = read_to_end_limited(&mut cursor, &mut out, 8, "test 限流").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("test 限流"));
        assert!(msg.contains("exceeds limit=8"));
    }
}
