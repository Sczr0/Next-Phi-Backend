use std::{collections::HashMap, fs::File, path::Path, sync::Arc};

use crate::{
    error::AppError,
    features::song::models::{SongCatalog, SongInfo},
    startup::chart_loader::ChartConstants,
};

/// 从 info 目录加载 `info.csv` 与 `nicklist.yaml`，构建内存索引
pub fn load_song_catalog(info_path: &Path) -> Result<SongCatalog, AppError> {
    if !info_path.exists() {
        return Err(AppError::Internal(format!(
            "info 目录不存在: {:?}",
            info_path
        )));
    }

    let info_csv = info_path.join("info.csv");
    let nicklist_yaml = info_path.join("nicklist.yaml");

    if !info_csv.exists() {
        return Err(AppError::Internal(format!(
            "未找到歌曲信息文件 info.csv: {:?}",
            info_csv
        )));
    }
    if !nicklist_yaml.exists() {
        return Err(AppError::Internal(format!(
            "未找到别名文件 nicklist.yaml: {:?}",
            nicklist_yaml
        )));
    }

    let mut catalog = SongCatalog::default();

    // 1) 读取 info.csv
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&info_csv)
        .map_err(|e| AppError::Internal(format!("读取 info.csv 失败: {}", e)))?;

    let headers = rdr
        .headers()
        .map_err(|e| AppError::Internal(format!("读取 info.csv 表头失败: {}", e)))?
        .clone();

    // 支持不同大小写/命名（song 或 name）
    let idx_of = |name: &str| -> Option<usize> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };

    let id_idx =
        idx_of("id").ok_or_else(|| AppError::Internal("info.csv 缺少必需列: id".into()))?;
    let name_idx = idx_of("song")
        .or_else(|| idx_of("name"))
        .ok_or_else(|| AppError::Internal("info.csv 缺少必需列: song/name".into()))?;
    let composer_idx = idx_of("composer")
        .ok_or_else(|| AppError::Internal("info.csv 缺少必需列: composer".into()))?;
    let illustrator_idx = idx_of("illustrator")
        .ok_or_else(|| AppError::Internal("info.csv 缺少必需列: illustrator".into()))?;
    let ez_idx =
        idx_of("EZ").ok_or_else(|| AppError::Internal("info.csv 缺少必需列: EZ".into()))?;
    let hd_idx =
        idx_of("HD").ok_or_else(|| AppError::Internal("info.csv 缺少必需列: HD".into()))?;
    let in_idx =
        idx_of("IN").ok_or_else(|| AppError::Internal("info.csv 缺少必需列: IN".into()))?;
    let at_idx =
        idx_of("AT").ok_or_else(|| AppError::Internal("info.csv 缺少必需列: AT".into()))?;

    let parse_opt_f32 = |s: &str| -> Result<Option<f32>, AppError> {
        let s = s.trim();
        if s.is_empty() {
            Ok(None)
        } else {
            s.parse::<f32>()
                .map(Some)
                .map_err(|e| AppError::Internal(format!("解析浮点数失败 '{}': {}", s, e)))
        }
    };

    for rec in rdr.records() {
        let record =
            rec.map_err(|e| AppError::Internal(format!("读取 info.csv 记录失败: {}", e)))?;

        let id = record.get(id_idx).unwrap_or("").trim().to_string();
        if id.is_empty() {
            continue;
        }
        let name = record.get(name_idx).unwrap_or("").trim().to_string();
        let composer = record.get(composer_idx).unwrap_or("").trim().to_string();
        let illustrator = record.get(illustrator_idx).unwrap_or("").trim().to_string();

        let ez = parse_opt_f32(record.get(ez_idx).unwrap_or(""))?;
        let hd = parse_opt_f32(record.get(hd_idx).unwrap_or(""))?;
        let in_level = parse_opt_f32(record.get(in_idx).unwrap_or(""))?;
        let at = parse_opt_f32(record.get(at_idx).unwrap_or(""))?;

        let info = Arc::new(SongInfo {
            id: id.clone(),
            name: name.clone(),
            composer,
            illustrator,
            chart_constants: ChartConstants {
                ez,
                hd,
                in_level,
                at,
            },
        });

        catalog.by_id.insert(id, Arc::clone(&info));
        catalog
            .by_name
            .entry(name)
            .or_insert_with(Vec::new)
            .push(Arc::clone(&info));
    }

    // 2) 读取 nicklist.yaml
    let nick_file = File::open(&nicklist_yaml)
        .map_err(|e| AppError::Internal(format!("打开 nicklist.yaml 失败: {}", e)))?;

    let nick_map: HashMap<String, Vec<String>> = serde_yaml::from_reader(nick_file)
        .map_err(|e| AppError::Internal(format!("解析 nicklist.yaml 失败: {}", e)))?;

    for (song_id, nick_vec) in nick_map.into_iter() {
        match catalog.by_id.get(&song_id) {
            Some(info_arc) => {
                for nickname in nick_vec {
                    let nickname = nickname.trim();
                    if nickname.is_empty() {
                        continue;
                    }
                    catalog
                        .by_nickname
                        .entry(nickname.to_string())
                        .or_insert_with(Vec::new)
                        .push(Arc::clone(info_arc));
                }
            }
            None => {
                tracing::warn!("nicklist.yaml 中的键未匹配任何歌曲ID: {}", song_id);
            }
        }
    }

    Ok(catalog)
}
