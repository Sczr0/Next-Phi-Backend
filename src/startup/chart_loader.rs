use std::{collections::HashMap, fs, path::Path};

use crate::error::AppError;

/// 单曲各难度定数
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
pub struct ChartConstants {
    pub ez: Option<f32>,
    pub hd: Option<f32>,
    #[serde(rename = "in")]
    pub in_level: Option<f32>,
    pub at: Option<f32>,
}

/// 歌曲ID -> 定数映射
pub type ChartConstantsMap = HashMap<String, ChartConstants>;

/// 从 difficulty.csv 加载曲目定数映射
/// 要求 CSV 头包含列：`id`, `EZ`, `HD`, `IN`, `AT`
pub fn load_chart_constants(file_path: &Path) -> Result<ChartConstantsMap, AppError> {
    if !file_path.exists() {
        return Err(AppError::Internal(format!(
            "未找到难度常量文件: {file_path:?}"
        )));
    }

    // 先尝试打开文件以区分 IO 问题
    fs::File::open(file_path)
        .map_err(|e| AppError::Internal(format!("打开 difficulty.csv 失败: {e}")))?;

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(file_path)
        .map_err(|e| AppError::Internal(format!("读取 CSV 失败: {e}")))?;

    let headers = rdr
        .headers()
        .map_err(|e| AppError::Internal(format!("读取 CSV 表头失败: {e}")))?
        .clone();

    // 表头索引（不区分大小写）
    let idx_of = |name: &str| -> Result<usize, AppError> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
            .ok_or_else(|| AppError::Internal(format!("CSV 缺少必需列: {name}")))
    };

    let id_idx = idx_of("id")?;
    let ez_idx = idx_of("EZ")?;
    let hd_idx = idx_of("HD")?;
    let in_idx = idx_of("IN")?;
    let at_idx = idx_of("AT")?;

    let mut map: ChartConstantsMap = HashMap::new();

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

    for result in rdr.records() {
        let record = result.map_err(|e| AppError::Internal(format!("读取 CSV 记录失败: {e}")))?;

        let id = record.get(id_idx).unwrap_or("").trim().to_string();
        if id.is_empty() {
            continue;
        }

        let ez = parse_opt_f32(record.get(ez_idx).unwrap_or(""))?;
        let hd = parse_opt_f32(record.get(hd_idx).unwrap_or(""))?;
        let in_level = parse_opt_f32(record.get(in_idx).unwrap_or(""))?;
        let at = parse_opt_f32(record.get(at_idx).unwrap_or(""))?;

        map.insert(
            id,
            ChartConstants {
                ez,
                hd,
                in_level,
                at,
            },
        );
    }

    Ok(map)
}
