//! 远端 info 文件加载器
//!
//! 从配置的 `resources.info_base_url` 拉取 difficulty.csv / info.csv / nicklist.yaml。
//! 通过 ETag 版本检测避免重复下载；网络不可达时静默 fallback 到本地文件。

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use crate::error::AppError;
use crate::startup::chart_loader::{ChartConstantsMap, parse_chart_constants};
use crate::startup::song_loader::parse_song_catalog;
use crate::features::song::models::SongCatalog;

const INFO_FILES: [&str; 3] = ["difficulty.csv", "info.csv", "nicklist.yaml"];

/// 远端的难度常量和歌曲目录数据
pub struct RemoteInfo {
    pub chart_constants: ChartConstantsMap,
    pub song_catalog: SongCatalog,
}

/// 尝试从远端加载 info 文件。
///
/// 流程：
/// 1. HEAD 每个文件获取 ETag
/// 2. 与本地缓存的 ETag（`.remote-etags.json`）对比
/// 3. 若所有文件 ETag 均一致 → 跳过下载
/// 4. 若有任一文件 ETag 不同 → GET 全部三个文件
/// 5. 解析并返回 `RemoteInfo`
///
/// 网络错误或远端不可达时返回 `Ok(None)`，不阻断启动。
pub async fn try_load_remote_info(
    base_url: &str,
    info_dir: &Path,
) -> Result<Option<RemoteInfo>, AppError> {
    let etag_path = info_dir.join(".remote-etags.json");
    let cached_etags = load_etag_cache(&etag_path);

    let client = match crate::http::client_timeout_30s() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("远端 info 加载跳过：无法创建 HTTP client: {e}");
            return Ok(None);
        }
    };

    // Phase 1: HEAD 请求获取远端 ETag
    let mut remote_etags: HashMap<String, String> = HashMap::new();
    let mut any_changed = false;

    for file in &INFO_FILES {
        let url = format!("{base_url}/{file}");
        match client.head(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Some(etag) = resp
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.trim_matches('"').to_string())
                {
                    if cached_etags.get(*file) != Some(&etag) {
                        any_changed = true;
                    }
                    remote_etags.insert(file.to_string(), etag);
                } else {
                    tracing::warn!("远端 info 文件缺少 ETag: {url}");
                    return Ok(None);
                }
            }
            Ok(resp) => {
                tracing::warn!(
                    "远端 info HEAD 请求返回非成功状态: {url} -> {}",
                    resp.status()
                );
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!("远端 info HEAD 请求失败: {url} -> {e}");
                return Ok(None);
            }
        }
    }

    if !any_changed {
        tracing::info!("远端 info 文件与本地缓存一致，跳过下载");
        return Ok(None);
    }

    // Phase 2: GET 下载全部三个文件
    tracing::info!("远端 info 文件有更新，开始下载...");
    let mut difficulty_bytes = Vec::new();
    let mut info_bytes = Vec::new();
    let mut nicklist_bytes = Vec::new();

    for file in &INFO_FILES {
        let url = format!("{base_url}/{file}");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    tracing::info!(
                        "远端 info 文件下载成功: {file} ({} bytes)",
                        bytes.len()
                    );
                    match *file {
                        "difficulty.csv" => difficulty_bytes = bytes.to_vec(),
                        "info.csv" => info_bytes = bytes.to_vec(),
                        "nicklist.yaml" => nicklist_bytes = bytes.to_vec(),
                        _ => {}
                    }
                }
                Err(e) => {
                    tracing::warn!("远端 info 文件读取失败: {file} -> {e}");
                    return Ok(None);
                }
            },
            Ok(resp) => {
                tracing::warn!(
                    "远端 info GET 请求返回非成功状态: {url} -> {}",
                    resp.status()
                );
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!("远端 info GET 请求失败: {url} -> {e}");
                return Ok(None);
            }
        }
    }

    // Phase 3: 解析
    let chart_constants = parse_chart_constants(Cursor::new(&difficulty_bytes))?;
    let song_catalog = parse_song_catalog(Cursor::new(&info_bytes), Cursor::new(&nicklist_bytes))?;

    // 保存新 ETag 缓存
    save_etag_cache(&etag_path, &remote_etags);

    tracing::info!("远端 info 加载成功，已更新本地 ETag 缓存");
    Ok(Some(RemoteInfo {
        chart_constants,
        song_catalog,
    }))
}

fn load_etag_cache(path: &Path) -> HashMap<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn save_etag_cache(path: &Path, etags: &HashMap<String, String>) {
    if let Ok(json) = serde_json::to_string(etags) {
        if let Err(e) = std::fs::write(path, json) {
            tracing::warn!("保存远端 info ETag 缓存失败: {e}");
        }
    }
}
