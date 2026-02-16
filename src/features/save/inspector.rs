//! 存档解密链路诊断工具（库内可复用，供 `src/bin/save_inspect.rs` 调用）
//!
//! 设计目标：
//! - 默认“安全输出”：不打印 stoken、不完整泄露存档内容；
//! - 输出结构化报告（JSON 可序列化），便于扩展字段（例如原始 summary、更多元信息）。

use std::collections::HashMap;
use std::io::Read;

use flate2::read::{GzDecoder, ZlibDecoder};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use zip::ZipArchive;

use crate::config::TapTapMultiConfig;
use crate::error::SaveProviderError;

use super::client;
use super::decryptor::{CipherSuite, DecryptionMeta, KdfSpec, decrypt_zip_entry};
use super::summary_parser::{SummaryParsed, parse_summary_base64};

/// 默认 stoken 环境变量名（避免把 token 写进命令行历史）。
pub const DEFAULT_STOKEN_ENV: &str = "PHI_STOKEN";

/// 诊断输出格式（二进制入口可选 text/json；库内主要产出结构化 report）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

/// 诊断选项（可扩展）。
#[derive(Debug, Clone)]
pub struct InspectOptions {
    /// 是否输出完整 URL（可能包含敏感 query）。
    pub show_full_url: bool,
    /// 是否输出原始 summary（base64 字符串）。
    pub show_summary_raw: bool,
    /// 每个 entry 输出多少字节的解密后明文预览（hex）。0 表示不输出。
    pub preview_plain_bytes: usize,
    /// 允许下载的最大字节数（防止意外的内存峰值）。
    pub max_download_bytes: usize,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            show_full_url: false,
            show_summary_raw: false,
            preview_plain_bytes: 0,
            max_download_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectReport {
    /// 生成时间（UTC RFC3339）。
    pub generated_at: String,
    /// TapTap 版本（cn/global）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taptap_version: Option<String>,

    /// 存档元信息（来源：官方元信息接口）。
    pub meta: SaveMetaReport,
    /// 解密参数（由官方 crypto 元信息推导）。
    pub decrypt_meta: DecryptMetaReport,
    /// 传输与解压判断。
    pub transport: TransportReport,
    /// zip 结构信息（无法解包时为 None，并在 transport.errors 中体现）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip: Option<ZipReport>,
    /// 逐 entry 诊断信息（包含是否存在/解密状态/前缀字节等）。
    pub entries: Vec<EntryReport>,

    /// 额外提示（用于解释“为什么看起来丢了 prefix”等）。
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveMetaReport {
    /// 下载 URL（默认脱敏）。
    pub download_url: String,
    /// 更新时间（LeanCloud updatedAt）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    /// 原始 summary（base64）。默认脱敏；可通过 options 控制是否完整输出。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_b64: Option<String>,
    /// summary 的结构化解析结果（解析失败时为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_parsed: Option<SummaryParsed>,
    /// summary 解析失败原因（仅在失败时存在，便于调试格式变更）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecryptMetaReport {
    pub cipher: CipherReport,
    pub kdf: KdfReport,
    /// 完整性校验：当前链路默认不启用（用于提示“CBC 仅靠 padding”）。
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum CipherReport {
    Aes256CbcPkcs7 { iv_hex: String },
    Aes128Gcm { nonce_hex: String, tag_len: usize },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum KdfReport {
    None,
    Pbkdf2Sha1 {
        salt_hex: String,
        rounds: u32,
        /// 不输出明文 password；仅输出长度与 sha256，便于排查“空密码/变化”。
        password_len: usize,
        password_sha256_hex: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TransportReport {
    pub download_bytes: usize,
    pub decompress: DecompressReport,
    /// 过程中的错误（例如 zip 解析失败）。
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecompressReport {
    pub detected: String,
    pub input_bytes: usize,
    pub output_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZipReport {
    pub file_count: usize,
    /// zip 内部文件名（默认最多 64 个，避免输出过长）。
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryReport {
    pub name: String,
    pub present: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_prefix_u8: Option<u8>,

    pub parser_handling: String,

    pub decrypted: EntryDecryptedReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryDecryptedReport {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub decrypted_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decrypted_prefix_u8: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plain_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plain_sha256_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plain_preview_hex: Option<String>,
}

/// 仅用于诊断：从官方接口拉取存档元信息 + 下载存档 zip + 解密若干 entry，并生成报告。
///
/// 安全性：
/// - 本函数从不打印/返回 stoken；
/// - 默认不返回“完整 URL / 完整 summary / 明文内容”，需要显式开启 options 才会输出。
pub async fn inspect_official_save(
    session_token: &str,
    taptap_config: &TapTapMultiConfig,
    taptap_version: Option<&str>,
    options: InspectOptions,
) -> Result<InspectReport, SaveProviderError> {
    let now = chrono::Utc::now().to_rfc3339();

    let (download_url, decrypt_meta, summary_b64, updated_at) =
        client::fetch_from_official(session_token, taptap_config, taptap_version).await?;

    let summary_report = build_summary_report(&summary_b64, options.show_summary_raw);
    let url_report = if options.show_full_url {
        download_url.clone()
    } else {
        redact_url(&download_url)
    };

    let decrypt_meta_report = build_decrypt_meta_report(&decrypt_meta);

    let download_bytes = download_bytes_limited(&download_url, options.max_download_bytes).await?;
    let mut errors = Vec::new();
    let download_len = download_bytes.len();

    let (zip_bytes, decompress_report) = try_decompress_bytes(download_bytes);

    let (zip_report, entry_reports) = match try_open_zip(&zip_bytes, &decrypt_meta, &options) {
        Ok(ok) => ok,
        Err(e) => {
            errors.push(format!("zip parse failed: {e}"));
            (None, build_missing_entry_reports())
        }
    };

    let mut notes = Vec::new();
    notes.push(
        "提示：decrypt_zip_entry 会保留 entry 第 1 字节 prefix，但 parser.rs 对不同 entry 的 prefix 处理不一致（部分会跳过）。".to_string(),
    );
    notes.push(
        "提示：当前链路未启用 HMAC 等完整性校验；CBC 模式主要依赖 padding 报错来发现密文异常。"
            .to_string(),
    );
    if matches!(decrypt_meta.cipher, CipherSuite::Aes128Gcm { .. }) {
        notes.push("提示：AES-128-GCM 实现基于 Aes128Gcm（固定 16 字节 tag），若 meta.tag_len != 16 可能出现兼容性问题。".to_string());
    }

    Ok(InspectReport {
        generated_at: now,
        taptap_version: taptap_version.map(|s| s.to_string()),
        meta: SaveMetaReport {
            download_url: url_report,
            updated_at,
            summary_b64: summary_report.summary_b64,
            summary_parsed: summary_report.summary_parsed,
            summary_parse_error: summary_report.summary_parse_error,
        },
        decrypt_meta: decrypt_meta_report,
        transport: TransportReport {
            download_bytes: download_len,
            decompress: decompress_report,
            errors,
        },
        zip: zip_report,
        entries: entry_reports,
        notes,
    })
}

struct SummaryReportParts {
    summary_b64: Option<String>,
    summary_parsed: Option<SummaryParsed>,
    summary_parse_error: Option<String>,
}

fn build_summary_report(summary_b64: &Option<String>, show_raw: bool) -> SummaryReportParts {
    let mut out = SummaryReportParts {
        summary_b64: None,
        summary_parsed: None,
        summary_parse_error: None,
    };
    let Some(b64) = summary_b64.as_deref() else {
        return out;
    };

    out.summary_b64 = Some(if show_raw {
        b64.to_string()
    } else {
        redact_b64(b64)
    });
    match parse_summary_base64(b64) {
        Ok(parsed) => out.summary_parsed = Some(parsed),
        Err(e) => out.summary_parse_error = Some(e.to_string()),
    }
    out
}

fn build_decrypt_meta_report(meta: &DecryptionMeta) -> DecryptMetaReport {
    let cipher = match &meta.cipher {
        CipherSuite::Aes256CbcPkcs7 { iv } => CipherReport::Aes256CbcPkcs7 {
            iv_hex: hex::encode(iv),
        },
        CipherSuite::Aes128Gcm { nonce, tag_len } => CipherReport::Aes128Gcm {
            nonce_hex: hex::encode(nonce),
            tag_len: *tag_len,
        },
    };

    let kdf = match &meta.kdf {
        KdfSpec::None => KdfReport::None,
        KdfSpec::Pbkdf2Sha1 {
            salt,
            rounds,
            password,
        } => {
            let mut h = Sha256::new();
            h.update(password);
            let digest = h.finalize();
            KdfReport::Pbkdf2Sha1 {
                salt_hex: hex::encode(salt),
                rounds: *rounds,
                password_len: password.len(),
                password_sha256_hex: hex::encode(digest),
            }
        }
    };

    DecryptMetaReport {
        cipher,
        kdf,
        integrity: "none".to_string(),
    }
}

async fn download_bytes_limited(url: &str, max_bytes: usize) -> Result<Vec<u8>, SaveProviderError> {
    let client = crate::http::client_timeout_90s()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(SaveProviderError::Network(
            resp.error_for_status().unwrap_err().to_string(),
        ));
    }

    // 优先用 Content-Length 进行粗判（可能没有或不可信，但对“异常大响应”有帮助）。
    if let Some(len) = resp.content_length()
        && len as usize > max_bytes
    {
        return Err(SaveProviderError::Io(format!(
            "download too large: content-length={len} exceeds limit={max_bytes}"
        )));
    }

    let bytes = resp.bytes().await?;
    if bytes.len() > max_bytes {
        return Err(SaveProviderError::Io(format!(
            "download too large: bytes={} exceeds limit={max_bytes}",
            bytes.len()
        )));
    }
    Ok(bytes.to_vec())
}

fn try_decompress_bytes(input: Vec<u8>) -> (Vec<u8>, DecompressReport) {
    let input_len = input.len();

    // ZIP 魔数：PK\x03\x04 / PK\x05\x06 / PK\x07\x08
    if input_len >= 4
        && input[0] == b'P'
        && input[1] == b'K'
        && matches!((input[2], input[3]), (3, 4) | (5, 6) | (7, 8))
    {
        return (
            input,
            DecompressReport {
                detected: "zip".to_string(),
                input_bytes: input_len,
                output_bytes: input_len,
            },
        );
    }

    // GZIP 魔数：1F 8B
    if input_len >= 2 && input[0] == 0x1F && input[1] == 0x8B {
        let mut gz = GzDecoder::new(input.as_slice());
        let mut out = Vec::new();
        if gz.read_to_end(&mut out).is_ok() {
            let out_len = out.len();
            let rep = DecompressReport {
                detected: "gzip".to_string(),
                input_bytes: input_len,
                output_bytes: out_len,
            };
            return (out, rep);
        }
        return (
            input,
            DecompressReport {
                detected: "gzip-invalid-fallback-raw".to_string(),
                input_bytes: input_len,
                output_bytes: input_len,
            },
        );
    }

    // 其他：尝试 Zlib，失败则回退 Raw Bytes。
    let mut z = ZlibDecoder::new(input.as_slice());
    let mut out = Vec::new();
    match z.read_to_end(&mut out) {
        Ok(_) => {
            let out_len = out.len();
            (
                out,
                DecompressReport {
                    detected: "zlib".to_string(),
                    input_bytes: input_len,
                    output_bytes: out_len,
                },
            )
        }
        Err(_) => (
            input,
            DecompressReport {
                detected: "raw".to_string(),
                input_bytes: input_len,
                output_bytes: input_len,
            },
        ),
    }
}

fn try_open_zip(
    bytes: &[u8],
    decrypt_meta: &DecryptionMeta,
    options: &InspectOptions,
) -> Result<(Option<ZipReport>, Vec<EntryReport>), SaveProviderError> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;

    let file_count = archive.len();
    let mut names = Vec::new();
    for idx in 0..file_count.min(64) {
        if let Ok(f) = archive.by_index(idx) {
            names.push(f.name().to_string());
        }
    }

    let zip_report = Some(ZipReport { file_count, names });

    let expected = ["gameRecord", "gameKey", "gameProgress", "user", "settings"];
    let parser_handling_map = expected
        .iter()
        .map(|name| (*name, parser_handling_for_entry(name)))
        .collect::<HashMap<_, _>>();

    let mut entries = Vec::new();
    for name in expected {
        let parser_handling = parser_handling_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        match archive.by_name(name) {
            Ok(mut f) => {
                let mut enc = Vec::with_capacity(f.size().min(16 * 1024 * 1024) as usize);
                f.read_to_end(&mut enc)?;
                let encrypted_len = enc.len();
                let encrypted_prefix = enc.first().copied();

                let decrypted = match decrypt_zip_entry(enc, decrypt_meta) {
                    Ok(out) => {
                        let decrypted_prefix = out.first().copied();
                        let plain = out.get(1..).unwrap_or(&[]);
                        let plain_sha256_hex = hex::encode(Sha256::digest(plain));
                        let plain_preview_hex = if options.preview_plain_bytes == 0 {
                            None
                        } else {
                            let n = options.preview_plain_bytes.min(plain.len());
                            Some(hex::encode(&plain[..n]))
                        };
                        EntryDecryptedReport {
                            ok: true,
                            error: None,
                            decrypted_len: Some(out.len()),
                            decrypted_prefix_u8: decrypted_prefix,
                            plain_len: Some(plain.len()),
                            plain_sha256_hex: Some(plain_sha256_hex),
                            plain_preview_hex,
                        }
                    }
                    Err(e) => EntryDecryptedReport {
                        ok: false,
                        error: Some(e.to_string()),
                        decrypted_len: None,
                        decrypted_prefix_u8: None,
                        plain_len: None,
                        plain_sha256_hex: None,
                        plain_preview_hex: None,
                    },
                };

                entries.push(EntryReport {
                    name: name.to_string(),
                    present: true,
                    encrypted_len: Some(encrypted_len),
                    encrypted_prefix_u8: encrypted_prefix,
                    parser_handling,
                    decrypted,
                });
            }
            Err(_) => {
                entries.push(EntryReport {
                    name: name.to_string(),
                    present: false,
                    encrypted_len: None,
                    encrypted_prefix_u8: None,
                    parser_handling,
                    decrypted: EntryDecryptedReport {
                        ok: false,
                        error: Some("missing zip entry".to_string()),
                        decrypted_len: None,
                        decrypted_prefix_u8: None,
                        plain_len: None,
                        plain_sha256_hex: None,
                        plain_preview_hex: None,
                    },
                });
            }
        }
    }

    Ok((zip_report, entries))
}

fn build_missing_entry_reports() -> Vec<EntryReport> {
    let expected = ["gameRecord", "gameKey", "gameProgress", "user", "settings"];
    expected
        .into_iter()
        .map(|name| EntryReport {
            name: name.to_string(),
            present: false,
            encrypted_len: None,
            encrypted_prefix_u8: None,
            parser_handling: parser_handling_for_entry(name),
            decrypted: EntryDecryptedReport {
                ok: false,
                error: Some("zip not available".to_string()),
                decrypted_len: None,
                decrypted_prefix_u8: None,
                plain_len: None,
                plain_sha256_hex: None,
                plain_preview_hex: None,
            },
        })
        .collect()
}

fn parser_handling_for_entry(name: &str) -> String {
    match name {
        "gameRecord" | "user" | "settings" => "解析时跳过第1字节(prefix)".to_string(),
        "gameKey" | "gameProgress" => "解析时把第1字节作为 version".to_string(),
        _ => "unknown".to_string(),
    }
}

fn redact_url(url: &str) -> String {
    // 目标：尽量保留可定位信息（host + path 末段），但去掉 query/fragment。
    // 不引入额外依赖（url crate），用轻量字符串处理即可满足诊断用途。
    let (base, _) = url.split_once('#').unwrap_or((url, ""));
    let (base, _) = base.split_once('?').unwrap_or((base, ""));

    // 提取 host（若是标准 scheme://host/...）。
    let host = base
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("unknown-host");

    // 提取 path 的最后一个 segment。
    let last_seg = base
        .split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or("unknown");

    format!(
        "{}://{}/.../{}",
        base.split("://").next().unwrap_or("http"),
        host,
        last_seg
    )
}

fn redact_b64(b64: &str) -> String {
    // base64 通常较短，但仍可能包含个人信息；默认只输出长度 + 头尾片段。
    if b64.len() <= 24 {
        return format!("(len={}) {}", b64.len(), b64);
    }
    let head = &b64[..12];
    let tail = &b64[b64.len() - 8..];
    format!("(len={}) {}...{}", b64.len(), head, tail)
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose};

    use super::*;

    #[test]
    fn redact_url_removes_query_fragment_and_keeps_host() {
        let url = "https://example.com/path/to/file.zip?token=abc#frag";
        let out = redact_url(url);
        assert!(out.contains("example.com"));
        assert!(!out.contains("token=abc"));
        assert!(!out.contains("#frag"));
        assert!(out.ends_with("/.../file.zip"));
    }

    #[test]
    fn redact_b64_short_keeps_original() {
        let b64 = "abcd";
        let out = redact_b64(b64);
        assert!(out.contains("abcd"));
    }

    #[test]
    fn redact_b64_long_is_truncated() {
        let raw = vec![0u8; 64];
        let b64 = general_purpose::STANDARD.encode(raw);
        let out = redact_b64(&b64);
        assert!(out.contains("len="));
        assert!(out.contains("..."));
    }
}
