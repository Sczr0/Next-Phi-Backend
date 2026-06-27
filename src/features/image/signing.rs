use crate::config::{CdnAuthMode, IllustrationSigningConfig, ImageSigningConfig};
use crate::error::AppError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

/// 生成指定长度的随机字符串（大小写字母+数字）
fn random_string(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    (0..len)
        .map(|_| chars[rng.gen_range(0..chars.len())] as char)
        .collect()
}

/// 计算 MD5 哈希，返回 32 位小写十六进制字符串
fn md5_hex(input: &str) -> String {
    use md5::Digest;
    let hash = md5::Md5::digest(input.as_bytes());
    format!("{hash:x}")
}

/// 根据配置生成签名URL
///
/// # 参数
/// - `config`: 签名配置
/// - `path`: 资源路径，必须以 `/` 开头，例如 `/ill/song_id.png`
///
/// # 返回
/// 签名后的路径+查询字符串（不含基地址），例如：
/// - TypeA: `/ill/song_id.png?token=1721028437-Kv4cPTAAP5YTi-0-0fbdca74...`
/// - TypeB: `/202407151533/d1f0b51c.../ill/song_id.png`
/// - TypeC: `/6688749e.../6694d30a/ill/song_id.png`
/// - TypeD: `/ill/song_id.png?token=cadcec4a...&t=1721029907`
pub fn sign_url(config: &IllustrationSigningConfig, path: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    match config.mode {
        CdnAuthMode::TypeA => sign_type_a(config, path, timestamp),
        CdnAuthMode::TypeB => sign_type_b(config, path, timestamp),
        CdnAuthMode::TypeC => sign_type_c(config, path, timestamp),
        CdnAuthMode::TypeD => sign_type_d(config, path, timestamp),
    }
}

/// TypeA 鉴权
///
/// URL: `Path?token=timestamp-rand-uid-md5hash`
/// MD5: `MD5(Path-timestamp-rand-uid-key)`
fn sign_type_a(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let rand = random_string(16);
    let uid = "0";
    let sign_str = format!("{path}-{timestamp}-{rand}-{uid}-{}", config.key);
    let hash = md5_hex(&sign_str);
    format!(
        "{path}?{}={timestamp}-{rand}-{uid}-{hash}",
        config.token_param,
    )
}

/// TypeB 鉴权
///
/// URL: `/timestamp/md5hash/Filename`
/// MD5: `MD5(key + timestamp + Path)`
/// timestamp 格式: `YYYYMMDDHHMM`（UTC+8）
fn sign_type_b(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let ts_formatted = format_timestamp_utc8(timestamp);
    let sign_str = format!("{}{ts_formatted}{path}", config.key);
    let hash = md5_hex(&sign_str);
    format!("/{ts_formatted}/{hash}/{}", path.trim_start_matches('/'))
}

/// TypeC 鉴权
///
/// URL: `/md5hash/timestamp/Filename`
/// MD5: `MD5(key + Path + timestamp)`
/// timestamp 为十六进制 Unix 时间戳（不含 0x 前缀）
fn sign_type_c(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let ts_hex = format!("{timestamp:x}");
    let sign_str = format!("{}{path}{ts_hex}", config.key);
    let hash = md5_hex(&sign_str);
    format!("/{hash}/{ts_hex}/{}", path.trim_start_matches('/'))
}

/// TypeD 鉴权（推荐）
///
/// URL: `Path?token=md5hash&t=timestamp`
/// MD5: `MD5(key + Path + timestamp)`
fn sign_type_d(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let sign_str = format!("{}{path}{timestamp}", config.key);
    let hash = md5_hex(&sign_str);
    format!(
        "{path}?{}={hash}&{}={timestamp}",
        config.token_param, config.timestamp_param,
    )
}

/// 将 Unix 时间戳格式化为 UTC+8 的 `YYYYMMDDHHMM` 字符串（TypeB 专用）
fn format_timestamp_utc8(timestamp: u64) -> String {
    use chrono::{FixedOffset, TimeZone};
    let utc8 = FixedOffset::east_opt(8 * 3600).unwrap();
    let dt = utc8.timestamp_opt(timestamp.cast_signed(), 0).unwrap();
    dt.format("%Y%m%d%H%M").to_string()
}

// ── SVG 内容签名（lilith-sig） ──

/// SVG 签名协议的版本标识。
/// - v3: 新增 hash（明文 SHA-256，客户端可本地校验）+ nonce（UUIDv7 防重放）
const LILITH_SIG_VERSION: &str = "v3";
/// SVG 签名在 XML 注释中的前缀模式。
const LILITH_SIG_PREFIX: &str = "lilith-sig";

/// 生成 UUIDv7（时间排序 UUID，毫秒精度）。
///
/// 布局：
/// - 48 bits: Unix 毫秒时间戳
/// - 4 bits: 版本 0x7
/// - 12 bits: 随机
/// - 2 bits: variant 0b10
/// - 62 bits: 随机
fn uuid_v7() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// 从 SVG 文本中提取的签名信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvgSignature {
    pub hmac: String,
    pub timestamp: u64,
    pub user_hash_prefix: Option<String>,
    pub request_id: Option<String>,
    /// SHA-256（去签名后的 SVG 正文），客户端可本地校验内容完整性
    pub content_hash: String,
    /// UUIDv7，防签名重放
    pub nonce: String,
}

/// 计算 SVG 内容的 SHA-256 摘要用于签名 payload。
fn svg_sha256(svg: &str) -> String {
    use sha2::Digest;
    let hash = Sha256::digest(svg.as_bytes());
    hex::encode(hash)
}

/// 对 SVG 字符串签名。
///
/// 签名 payload = `{timestamp}:{uid}:{rid}:{nonce}:{content_hash}`
/// 使用 HMAC-SHA256 + server key 生成签名。
pub fn sign_svg(
    svg: &str,
    config: &ImageSigningConfig,
    user_hash: Option<&str>,
) -> Option<SvgSignature> {
    let key = config.effective_key()?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let content_hash = svg_sha256(svg);
    let nonce = uuid_v7();
    let user_hash_part = user_hash.map_or("anon", |h| {
        let end = h.char_indices().nth(8).map_or(h.len(), |(i, _)| i);
        &h[..end]
    });
    let request_id = crate::request_id::current_request_id().unwrap_or_default();

    let payload = format!("{timestamp}:{user_hash_part}:{request_id}:{nonce}:{content_hash}");

    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).ok()?;
    mac.update(payload.as_bytes());
    let sig_bytes = mac.finalize().into_bytes();
    let hmac = hex::encode(sig_bytes);

    Some(SvgSignature {
        hmac,
        timestamp,
        user_hash_prefix: Some(user_hash_part.to_string()),
        request_id: if request_id.is_empty() {
            None
        } else {
            Some(request_id)
        },
        content_hash,
        nonce,
    })
}

/// 签名行在 SVG 中的 CSS class，用于识别与剥离。
const SIG_FOOTER_CLASS: &str = "lilith-sig-footer";

/// 构建签名行完整字符串。
fn build_sig_line(sig: &SvgSignature) -> String {
    let uid = sig.user_hash_prefix.as_deref().unwrap_or("anon");
    let rid = sig.request_id.as_deref().unwrap_or("");
    if rid.is_empty() {
        format!(
            "{LILITH_SIG_PREFIX}:{LILITH_SIG_VERSION}:hmac={}:t={}:uid={uid}:hash={}:nonce={}",
            sig.hmac, sig.timestamp, sig.content_hash, sig.nonce
        )
    } else {
        format!(
            "{LILITH_SIG_PREFIX}:{LILITH_SIG_VERSION}:hmac={}:t={}:uid={uid}:rid={rid}:hash={}:nonce={}",
            sig.hmac, sig.timestamp, sig.content_hash, sig.nonce
        )
    }
}

/// 将签名行注入到 SVG 底部（在 `</svg>` 之前作为可见 `<text>` 元素）。
///
/// 放在原有 footer 下方，灰色小字，与 SVG 主体底部对齐。
pub fn inject_sig_footer(svg: &str, sig: &SvgSignature) -> String {
    let line = build_sig_line(sig);
    let canvas_h = parse_viewbox_height(svg)
        .or_else(|| parse_svg_attr(svg, "height"))
        .unwrap_or(600.0);
    // 字号 9，距底边留 12px baseline，避免压线/被裁切。
    let y = (canvas_h - 12.0).max(0.0);
    let text_elem = format!(
        "<text x=\"50%\" y=\"{y}\" class=\"{SIG_FOOTER_CLASS}\" text-anchor=\"middle\" font-size=\"9\" font-family=\"monospace\" fill=\"#999\">{line}</text>"
    );
    if let Some(pos) = svg.rfind("</svg>") {
        let mut out = String::with_capacity(svg.len() + text_elem.len() + 1);
        out.push_str(&svg[..pos]);
        out.push('\n');
        out.push_str(&text_elem);
        out.push_str(&svg[pos..]);
        out
    } else {
        format!("{svg}{text_elem}")
    }
}

/// 从 SVG 中提取 viewBox 高度。
///
/// 正确处理被单/双引号包裹的属性值，例如：
/// `viewBox="0 0 800 1480"` / `viewBox='0 0 800 1480'`
/// 未带引号的情形同样兼容（取到下一个空白/`>` 为止）。
fn parse_viewbox_height(svg: &str) -> Option<f64> {
    let after = svg.split("viewBox=").nth(1)?;
    let val = take_attr_value(after);
    let parts: Vec<&str> = val.splitn(4, |c: char| c.is_whitespace()).collect();
    if parts.len() >= 4 {
        parts[3].parse::<f64>().ok()
    } else {
        None
    }
}

/// 从 `<svg>` 标签提取指定属性（如 `height` / `width`）的数值。
/// 仅在 `<svg ...>` 开闭标签范围内查找，避免误命中后续元素的同名属性。
fn parse_svg_attr(svg: &str, name: &str) -> Option<f64> {
    let tag_end = svg.find('>')?;
    let head = &svg[..tag_end];
    let after = head.split(&format!("{name}=")).nth(1)?;
    take_attr_value(after).parse::<f64>().ok()
}

/// 取出属性值首个 token：支持单/双引号定界或无引号裸值。
fn take_attr_value(s: &str) -> &str {
    let s = s.trim_start();
    match s.as_bytes().first() {
        Some(b'"') => s.split_once('"').map_or("", |(_, rest)| {
            rest.split_once('"').map_or(rest, |(val, _)| val)
        }),
        Some(b'\'') => s.split_once('\'').map_or("", |(_, rest)| {
            rest.split_once('\'').map_or(rest, |(val, _)| val)
        }),
        _ => s
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()
            .unwrap_or(""),
    }
}

/// 从 SVG 中移除签名行，返回清理后的原始 SVG（用于验证时重新计算摘要）。
///
/// 签名行特征：`<text class="lilith-sig-footer" ...>lilith-sig:v3:...</text>`
fn strip_svg_signature(svg: &str) -> String {
    let marker = format!("class=\"{SIG_FOOTER_CLASS}\"");
    let Some(start) = svg.find(&marker) else {
        return svg.to_string();
    };
    // 向前找到 <text 开始
    let tag_start = svg[..start].rfind("<text ").unwrap_or(start);
    // 向后找到 </text>
    let Some(end) = svg[start..].find("</text>") else {
        return svg.to_string();
    };
    let text_end = start + end + 7; // </text>
    // 吞掉前导换行
    let effective_start = if tag_start > 0 && svg.as_bytes()[tag_start - 1] == b'\n' {
        tag_start - 1
    } else {
        tag_start
    };
    let effective_end = if text_end < svg.len() && svg.as_bytes()[text_end] == b'\n' {
        text_end + 1
    } else {
        text_end
    };
    format!("{}{}", &svg[..effective_start], &svg[effective_end..])
}

/// 从 SVG 字符串中提取签名信息。
///
/// 匹配 footer 中的签名行：`lilith-sig:v3:hmac=<hex>:t=<unix_ts>:uid=<prefix>:...`
pub fn extract_svg_signature(svg: &str) -> Option<SvgSignature> {
    let pattern = format!("{LILITH_SIG_PREFIX}:{LILITH_SIG_VERSION}:");
    let start = svg.find(&pattern)?;
    let end = svg[start..].find(['<', '\n'])?;
    let body = &svg[start..start + end];
    // body = "lilith-sig:v3:hmac=xxx:t=12345:uid=abcd1234:hash=...:nonce=..."

    let mut hmac: Option<String> = None;
    let mut timestamp: Option<u64> = None;
    let mut uid: Option<String> = None;
    let mut rid: Option<String> = None;
    let mut content_hash: Option<String> = None;
    let mut nonce: Option<String> = None;

    for part in body.split(':') {
        if let Some(v) = part.strip_prefix("hmac=") {
            hmac = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("t=") {
            timestamp = v.parse::<u64>().ok();
        } else if let Some(v) = part.strip_prefix("uid=") {
            uid = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("rid=") {
            rid = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("hash=") {
            content_hash = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("nonce=") {
            nonce = Some(v.to_string());
        }
    }

    Some(SvgSignature {
        hmac: hmac?,
        timestamp: timestamp?,
        user_hash_prefix: uid,
        request_id: rid,
        content_hash: content_hash.unwrap_or_default(),
        nonce: nonce.unwrap_or_default(),
    })
}

/// 验证 SVG 签名是否有效。
///
/// 1. 从 SVG 中提取签名
/// 2. 移除签名注释后重新计算原始 SVG 的 MD5
/// 3. 用 server key 重新计算 HMAC
/// 4. 对比（时间恒定比较）
/// 5. 可选检查时间窗口
pub fn verify_svg_signature(
    svg: &str,
    config: &ImageSigningConfig,
) -> Result<SvgSignature, AppError> {
    let key = config
        .effective_key()
        .ok_or_else(|| AppError::Validation("服务端未配置签名密钥".into()))?;

    let extracted = extract_svg_signature(svg)
        .ok_or_else(|| AppError::Validation("SVG 中未找到 lilith-sig 签名".into()))?;

    // 重建 payload：重算 SHA-256 并与签名字段交叉校验
    let clean_svg = strip_svg_signature(svg);
    let actual_hash = svg_sha256(&clean_svg);

    // 客户端可独立校验：本地算的 hash 应与签名字段一致
    if actual_hash != extracted.content_hash {
        return Err(AppError::Validation(
            "签名校验失败：内容 SHA-256 与签名中的 hash 不匹配（SVG 被篡改）".into(),
        ));
    }

    let uid = extracted.user_hash_prefix.as_deref().unwrap_or("anon");
    let rid = extracted.request_id.as_deref().unwrap_or("");
    let payload = format!(
        "{}:{uid}:{rid}:{}:{actual_hash}",
        extracted.timestamp, extracted.nonce
    );

    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|e| AppError::Internal(format!("HMAC 初始化失败: {e}")))?;
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    // 恒定时间比较
    if expected.len() != extracted.hmac.len() || {
        let mut acc = 0u8;
        for (a, b) in expected.bytes().zip(extracted.hmac.bytes()) {
            acc |= a ^ b;
        }
        acc != 0
    } {
        return Err(AppError::Validation("签名校验失败：HMAC 不匹配".into()));
    }

    // 时间窗口检查
    if config.ttl_secs > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let age = now.saturating_sub(extracted.timestamp);
        if age > config.ttl_secs {
            return Err(AppError::Validation(format!(
                "签名已过期（签发于 {}s 前，窗口 {}s）",
                age, config.ttl_secs
            )));
        }
    }

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> IllustrationSigningConfig {
        IllustrationSigningConfig {
            enabled: true,
            mode: CdnAuthMode::TypeD,
            key: "DvYmqE81E1F9R791H6lmht".to_string(),
            backup_key: None,
            ttl_secs: 300,
            token_param: "token".to_string(),
            timestamp_param: "t".to_string(),
        }
    }

    #[test]
    fn test_type_d_sign() {
        let config = IllustrationSigningConfig {
            mode: CdnAuthMode::TypeD,
            ..test_config()
        };
        let path = "/foo.jpg";
        let timestamp = 1_721_029_907_u64;
        let sign_str = format!("{}{path}{timestamp}", config.key);
        let expected_hash = md5_hex(&sign_str);
        let result = sign_type_d(&config, path, timestamp);
        assert!(result.starts_with("/foo.jpg?token="));
        assert!(result.contains(&format!("&t={timestamp}")));
        assert!(result.contains(&expected_hash));
    }

    #[test]
    fn test_type_a_sign() {
        let config = IllustrationSigningConfig {
            mode: CdnAuthMode::TypeA,
            ..test_config()
        };
        let path = "/foo.jpg";
        let timestamp = 1_721_028_437_u64;
        let result = sign_type_a(&config, path, timestamp);
        assert!(result.starts_with("/foo.jpg?token="));
        assert!(result.contains(&timestamp.to_string()));
    }

    #[test]
    fn test_type_c_sign() {
        let config = IllustrationSigningConfig {
            mode: CdnAuthMode::TypeC,
            ..test_config()
        };
        let path = "/foo.jpg";
        let timestamp = 1_721_029_907_u64;
        let ts_hex = format!("{timestamp:x}");
        let result = sign_type_c(&config, path, timestamp);
        // TypeC 格式: /{md5hash}/{timestamp_hex}/{filename}
        let parts: Vec<&str> = result.split('/').collect();
        assert_eq!(parts.len(), 4); // "", md5hash, ts_hex, foo.jpg
        assert_eq!(parts[2], ts_hex);
        assert_eq!(parts[3], "foo.jpg");
    }

    #[test]
    fn test_type_b_format_timestamp() {
        // 2024-07-15 15:33:50 UTC+8 => timestamp ~1721028830
        let ts = 1_721_028_830_u64;
        let formatted = format_timestamp_utc8(ts);
        assert_eq!(formatted.len(), 12); // YYYYMMDDHHMM
    }

    // ── SVG 签名测试 ──

    fn test_signing_config() -> ImageSigningConfig {
        ImageSigningConfig {
            enabled: true,
            key: "test-signing-key-32bytes-long!!".to_string(),
            ttl_secs: 0,
            public_verify: false,
        }
    }

    #[test]
    fn test_sign_and_inject_roundtrip() {
        let cfg = test_signing_config();
        let svg = r#"<svg viewBox="0 0 800 600">
  <text x="10" y="20">Hello</text>
</svg>"#;
        let sig = sign_svg(svg, &cfg, Some("abc123456789userhash")).expect("sign");
        let signed = inject_sig_footer(svg, &sig);

        assert!(signed.contains("lilith-sig:v3:"));
        assert!(signed.contains("hmac="));
        assert!(signed.contains(":uid=abc12345"));
        assert!(signed.contains("lilith-sig-footer"));
        assert!(signed.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn test_extract_signature() {
        let svg = r#"<svg viewBox="0 0 800 600">
<text class="lilith-sig-footer">lilith-sig:v3:hmac=abcdef1234567890:t=1721029907:uid=test1234:hash=sha256abc:nonce=01900000-0000-7000-8000-000000000001</text>
</svg>"#;
        let sig = extract_svg_signature(svg).expect("extract");
        assert_eq!(sig.hmac, "abcdef1234567890");
        assert_eq!(sig.timestamp, 1_721_029_907);
        assert_eq!(sig.user_hash_prefix.as_deref(), Some("test1234"));
        assert_eq!(sig.content_hash, "sha256abc");
        assert_eq!(sig.nonce, "01900000-0000-7000-8000-000000000001");
    }

    #[test]
    fn test_verify_svg_signature() {
        let cfg = test_signing_config();
        let svg = r#"<svg viewBox="0 0 800 600">
  <text x="10" y="20">Verify Me</text>
</svg>"#;
        let sig = sign_svg(svg, &cfg, Some("userABCD")).expect("sign");
        let signed = inject_sig_footer(svg, &sig);

        assert_eq!(strip_svg_signature(&signed), svg);
        let verified = verify_svg_signature(&signed, &cfg).expect("verify");
        assert_eq!(verified.hmac, sig.hmac);
        assert_eq!(verified.user_hash_prefix.as_deref(), Some("userABCD"));
    }

    #[test]
    fn test_verify_tampered_svg_fails() {
        let cfg = test_signing_config();
        let svg = r#"<svg viewBox="0 0 800 600"></svg>"#;
        let sig = sign_svg(svg, &cfg, None).expect("sign");
        let signed = inject_sig_footer(svg, &sig);

        let tampered = signed.replacen("<svg", "<svg><rect width='100' height='100'/>", 1);
        assert!(verify_svg_signature(&tampered, &cfg).is_err());
    }

    #[test]
    fn test_extract_none_for_unsigned_svg() {
        let svg = "<svg></svg>";
        assert!(extract_svg_signature(svg).is_none());
    }

    #[test]
    fn test_sig_footer_full_roundtrip() {
        let cfg = test_signing_config();
        let svg = r#"<svg viewBox="0 0 800 600">
  <text x="10" y="20">Best 30</text>
</svg>"#;

        let sig = sign_svg(svg, &cfg, Some("userABCD")).expect("sign");
        let signed = inject_sig_footer(svg, &sig);

        assert!(signed.contains("lilith-sig:v3:hmac="));
        assert!(signed.contains(":nonce="));
        assert_eq!(strip_svg_signature(&signed), svg);

        let verified = verify_svg_signature(&signed, &cfg).expect("verify");
        assert_eq!(verified.hmac, sig.hmac);
        assert!(!verified.nonce.is_empty());
    }

    #[test]
    fn test_sign_no_user_hash() {
        let cfg = test_signing_config();
        let svg = "<svg></svg>";
        let sig = sign_svg(svg, &cfg, None).expect("sign");
        assert_eq!(sig.user_hash_prefix.as_deref(), Some("anon"));
    }

    // ── 回归：签名行必须落在画布底部，而非被 fallback 到 600 附近 ──

    #[test]
    fn test_parse_viewbox_height_double_quoted() {
        // 与实际渲染器（bn_defs::write_svg_open）一致的带引号 viewBox。
        let svg = r#"<svg width="820" height="1480" viewBox="0 0 820 1480" xmlns="http://www.w3.org/2000/svg">"#;
        approx_eq(parse_viewbox_height(svg).unwrap(), 1480.0);
    }

    #[test]
    fn test_parse_viewbox_height_single_quoted() {
        let svg = "<svg viewBox='0 0 820 1480' xmlns='http://www.w3.org/2000/svg'>";
        approx_eq(parse_viewbox_height(svg).unwrap(), 1480.0);
    }

    #[test]
    fn test_parse_svg_attr_height() {
        let svg = r#"<svg width="820" height="1480" viewBox="0 0 820 1480">"#;
        approx_eq(parse_svg_attr(svg, "height").unwrap(), 1480.0);
        approx_eq(parse_svg_attr(svg, "width").unwrap(), 820.0);
    }

    #[test]
    fn test_sig_footer_y_is_at_canvas_bottom() {
        let cfg = test_signing_config();
        // 模拟真实 BestN 渲染器输出：高画布、带引号 viewBox。
        let svg = r#"<svg width="820" height="1480" viewBox="0 0 820 1480" xmlns="http://www.w3.org/2000/svg">
  <text x="10" y="20">Best 30</text>
</svg>"#;

        let sig = sign_svg(svg, &cfg, Some("userABCD")).expect("sign");
        let signed = inject_sig_footer(svg, &sig);

        // 注入元素应位于 y ≈ canvas_h - 12 ≈ 1468，而非 fallback 的 588 左右。
        let y_val = extract_injected_y(&signed).expect("injected text has y attr");
        approx_eq(y_val, 1480.0 - 12.0);
        assert!(
            y_val > 1400.0,
            "footer must be near canvas bottom, got y={y_val}"
        );

        // 往返仍正确。
        assert_eq!(strip_svg_signature(&signed), svg);
        verify_svg_signature(&signed, &cfg).expect("verify");
    }

    fn extract_injected_y(signed: &str) -> Option<f64> {
        let marker = "class=\"lilith-sig-footer\"";
        let pos = signed.find(marker)?;
        let head = &signed[..pos];
        let tag_start = head.rfind("<text ")?;
        let tag = &signed[tag_start..pos];
        let after = tag.split("y=\"").nth(1)?;
        let end = after.find('"')?;
        after[..end].parse::<f64>().ok()
    }

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }
}
