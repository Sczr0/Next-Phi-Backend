use std::{collections::HashMap, fs::File, path::Path, sync::Arc};

use crate::{
    error::AppError,
    features::song::models::{SongCatalog, SongInfo, normalize_song_search_text},
    startup::chart_loader::ChartConstants,
};

fn resolve_song_for_nick_key(catalog: &SongCatalog, key: &str) -> Option<Arc<SongInfo>> {
    let normalized_key = normalize_song_search_text(key);
    if normalized_key.is_empty() {
        return None;
    }

    let mut matched: Option<Arc<SongInfo>> = None;
    for song in catalog.by_id.values() {
        let normalized_name = normalize_song_search_text(&song.name);
        if normalized_name.is_empty() {
            continue;
        }

        let matched_by_name = normalized_key == normalized_name
            || normalized_key.contains(&normalized_name)
            || normalized_name.contains(&normalized_key);
        if !matched_by_name {
            continue;
        }

        match &matched {
            None => matched = Some(Arc::clone(song)),
            Some(existing) if existing.id == song.id => {}
            Some(_) => return None,
        }
    }

    matched
}

fn push_song_nickname(catalog: &mut SongCatalog, nickname: &str, song: &Arc<SongInfo>) {
    let nickname = nickname.trim();
    if nickname.is_empty() {
        return;
    }

    let entry = catalog.by_nickname.entry(nickname.to_string()).or_default();
    if !entry.iter().any(|existing| existing.id == song.id) {
        entry.push(Arc::clone(song));
    }
}

/// 从 info 目录加载 `info.csv` 与 `nicklist.yaml`，构建内存索引
pub fn load_song_catalog(info_path: &Path) -> Result<SongCatalog, AppError> {
    if !info_path.exists() {
        return Err(AppError::Internal(format!(
            "info 目录不存在: {}",
            info_path.display()
        )));
    }

    let info_csv = info_path.join("info.csv");
    let nicklist_yaml = info_path.join("nicklist.yaml");

    if !info_csv.exists() {
        return Err(AppError::Internal(format!(
            "未找到歌曲信息文件 info.csv: {}",
            info_csv.display()
        )));
    }
    if !nicklist_yaml.exists() {
        return Err(AppError::Internal(format!(
            "未找到别名文件 nicklist.yaml: {}",
            nicklist_yaml.display()
        )));
    }

    let mut catalog = SongCatalog::default();

    // 1) 读取 info.csv
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&info_csv)
        .map_err(|e| AppError::Internal(format!("读取 info.csv 失败: {e}")))?;

    let headers = rdr
        .headers()
        .map_err(|e| AppError::Internal(format!("读取 info.csv 表头失败: {e}")))?
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
                .map_err(|e| AppError::Internal(format!("解析浮点数失败 '{s}': {e}")))
        }
    };

    for rec in rdr.records() {
        let record = rec.map_err(|e| AppError::Internal(format!("读取 info.csv 记录失败: {e}")))?;

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
            .or_default()
            .push(Arc::clone(&info));
    }

    // 2) 读取 nicklist.yaml
    let nick_file = File::open(&nicklist_yaml)
        .map_err(|e| AppError::Internal(format!("打开 nicklist.yaml 失败: {e}")))?;

    let nick_map: HashMap<String, Vec<String>> = serde_yaml::from_reader(nick_file)
        .map_err(|e| AppError::Internal(format!("解析 nicklist.yaml 失败: {e}")))?;

    let mut fallback_matched_keys: Vec<(String, String)> = Vec::new();
    let mut unresolved_keys: Vec<(String, Vec<String>)> = Vec::new();

    for (song_id, nick_vec) in nick_map {
        let resolved_song = catalog
            .by_id
            .get(&song_id)
            .cloned()
            .or_else(|| resolve_song_for_nick_key(&catalog, &song_id));

        match resolved_song {
            Some(info_arc) => {
                let matched_by_fallback = info_arc.id != song_id;
                if matched_by_fallback {
                    push_song_nickname(&mut catalog, &song_id, &info_arc);
                    fallback_matched_keys.push((song_id.clone(), info_arc.id.clone()));
                }

                for nickname in nick_vec {
                    push_song_nickname(&mut catalog, &nickname, &info_arc);
                }
            }
            None => {
                unresolved_keys.push((song_id, nick_vec));
            }
        }
    }

    if !fallback_matched_keys.is_empty() {
        let preview = fallback_matched_keys
            .iter()
            .take(5)
            .map(|(key, id)| format!("{key} -> {id}"))
            .collect::<Vec<_>>()
            .join(", ");
        tracing::info!(
            "nicklist.yaml 中有 {} 个键通过曲名归一化回退匹配成功，例如: {}",
            fallback_matched_keys.len(),
            preview
        );
    }
    for (key, aliases) in &unresolved_keys {
        let alias_preview = aliases
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        tracing::warn!(
            "nicklist.yaml 中的键未匹配任何歌曲ID或官方曲名: {} (aliases={}, sample=[{}])",
            key,
            aliases.len(),
            alias_preview
        );
    }
    if !unresolved_keys.is_empty() {
        tracing::warn!(
            "nicklist.yaml 中仍有 {} 个键无法匹配歌曲ID或官方曲名",
            unresolved_keys.len()
        );
    }

    // 构建搜索缓存，避免运行期每次搜索重复分配与遍历预处理。
    catalog.rebuild_search_cache();

    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::load_song_catalog;

    fn new_temp_info_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phi-song-loader-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn load_song_catalog_maps_unmatched_nick_key_by_normalized_song_name() {
        let dir = new_temp_info_dir();
        let info_csv = "\
id,song,composer,illustrator,EZ,HD,IN,AT
Burn.NceS,Burn,NceS,i,1.0,2.0,3.0,
Wintercube.CtymaxfeatNceS,Winter ↑cube↓,Ctymax feat. NceS,i,1.0,2.0,3.0,
";
        let nicklist_yaml = "\
Burn(Haocore Mix):
  - BurnSP
Winter♂cube:
  - 冬立方
Anomaly:
  - 异常
";

        fs::write(dir.join("info.csv"), info_csv).expect("write info.csv");
        fs::write(dir.join("nicklist.yaml"), nicklist_yaml).expect("write nicklist.yaml");

        let catalog = load_song_catalog(&dir).expect("load catalog");

        let burn = catalog.search("Burn(Haocore Mix)");
        assert_eq!(burn.len(), 1);
        assert_eq!(burn[0].id, "Burn.NceS");

        let burn_alias = catalog.search("BurnSP");
        assert_eq!(burn_alias.len(), 1);
        assert_eq!(burn_alias[0].id, "Burn.NceS");

        let winter = catalog.search("Winter♂cube");
        assert_eq!(winter.len(), 1);
        assert_eq!(winter[0].id, "Wintercube.CtymaxfeatNceS");

        let unresolved = catalog.search("Anomaly");
        assert!(unresolved.is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}
