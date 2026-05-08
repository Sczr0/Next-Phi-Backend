use crate::config::{CdnAuthMode, IllustrationSigningConfig};
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
    format!("{:x}", hash)
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
    let sign_str = format!("{}-{}-{}-{}-{}", path, timestamp, rand, uid, config.key);
    let hash = md5_hex(&sign_str);
    format!(
        "{}?{}={}-{}-{}-{}",
        path, config.token_param, timestamp, rand, uid, hash
    )
}

/// TypeB 鉴权
///
/// URL: `/timestamp/md5hash/Filename`
/// MD5: `MD5(key + timestamp + Path)`
/// timestamp 格式: `YYYYMMDDHHMM`（UTC+8）
fn sign_type_b(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let ts_formatted = format_timestamp_utc8(timestamp);
    let sign_str = format!("{}{}{}", config.key, ts_formatted, path);
    let hash = md5_hex(&sign_str);
    format!(
        "/{}/{}/{}",
        ts_formatted,
        hash,
        path.trim_start_matches('/')
    )
}

/// TypeC 鉴权
///
/// URL: `/md5hash/timestamp/Filename`
/// MD5: `MD5(key + Path + timestamp)`
/// timestamp 为十六进制 Unix 时间戳（不含 0x 前缀）
fn sign_type_c(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let ts_hex = format!("{:x}", timestamp);
    let sign_str = format!("{}{}{}", config.key, path, ts_hex);
    let hash = md5_hex(&sign_str);
    format!("/{}/{}/{}", hash, ts_hex, path.trim_start_matches('/'))
}

/// TypeD 鉴权（推荐，最安全）
///
/// URL: `Path?token=md5hash&t=timestamp`
/// MD5: `MD5(key + Path + timestamp)`
fn sign_type_d(config: &IllustrationSigningConfig, path: &str, timestamp: u64) -> String {
    let sign_str = format!("{}{}{}", config.key, path, timestamp);
    let hash = md5_hex(&sign_str);
    format!(
        "{}?{}={}&{}={}",
        path, config.token_param, hash, config.timestamp_param, timestamp
    )
}

/// 将 Unix 时间戳格式化为 UTC+8 的 `YYYYMMDDHHMM` 字符串（TypeB 专用）
fn format_timestamp_utc8(timestamp: u64) -> String {
    use chrono::{FixedOffset, TimeZone};
    let utc8 = FixedOffset::east_opt(8 * 3600).unwrap();
    let dt = utc8.timestamp_opt(timestamp as i64, 0).unwrap();
    dt.format("%Y%m%d%H%M").to_string()
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
        let timestamp = 1721029907u64;
        let sign_str = format!("{}{}{}", config.key, path, timestamp);
        let expected_hash = md5_hex(&sign_str);
        let result = sign_type_d(&config, path, timestamp);
        assert!(result.starts_with("/foo.jpg?token="));
        assert!(result.contains(&format!("&t={}", timestamp)));
        assert!(result.contains(&expected_hash));
    }

    #[test]
    fn test_type_a_sign() {
        let config = IllustrationSigningConfig {
            mode: CdnAuthMode::TypeA,
            ..test_config()
        };
        let path = "/foo.jpg";
        let timestamp = 1721028437u64;
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
        let timestamp = 1721029907u64;
        let ts_hex = format!("{:x}", timestamp);
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
        let ts = 1721028830u64;
        let formatted = format_timestamp_utc8(ts);
        assert_eq!(formatted.len(), 12); // YYYYMMDDHHMM
    }
}
