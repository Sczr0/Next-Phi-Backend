use crate::auth_contract::UnifiedSaveRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "song": "Tempestissimo",
  "difficulty": "AT",
  "acc": 99.43,
  "rks": 15.12
}))]
pub struct ChartTextItem {
    /// 歌曲名称
    pub song: String,
    /// 难度（EZ/HD/IN/AT）
    pub difficulty: String,
    /// ACC 百分比（如 99.43）
    pub acc: f64,
    /// 该谱面的 RKS 值
    pub rks: f64,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "best27Sum": 390.12,
  "apTop3Sum": 49.20
}))]
pub struct RksCompositionText {
    /// Best27 的 RKS 总和
    pub best27_sum: f64,
    /// AP Top3 的 RKS 总和
    pub ap_top3_sum: f64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "rank": 1,
  "alias": "Alice",
  "user": "ab12****",
  "score": 14.73,
  "updatedAt": "2025-09-20T04:10:44Z",
  "bestTop3": [{"song":"Tempestissimo","difficulty":"AT","acc":99.43,"rks":15.12}],
  "apTop3": [{"song":"AP Song","difficulty":"IN","acc":100.0,"rks":13.45}]
}))]
pub struct LeaderboardTopItem {
    /// 名次（竞争排名）
    pub rank: i64,
    /// 公开别名（如有）
    pub alias: Option<String>,
    /// 去敏化用户标识（hash 前缀）
    pub user: String,
    /// 总 RKS
    pub score: f64,
    /// 最近更新时间（UTC RFC3339）
    pub updated_at: String,
    /// （可选）BestTop3 列表（当用户允许展示时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_top3: Option<Vec<ChartTextItem>>,
    /// （可选）AP Top3 列表（当用户允许展示时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_top3: Option<Vec<ChartTextItem>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "items": [
    {
      "rank": 1,
      "alias": "Alice",
      "user": "ab12****",
      "score": 14.73,
      "updatedAt": "2025-09-20T04:10:44Z"
    }
  ],
  "total": 12345,
  "nextAfterScore": 14.73,
  "nextAfterUpdated": "2025-09-20T04:10:44Z",
  "nextAfterUser": "abcd****"
}))]
pub struct LeaderboardTopResponse {
    pub items: Vec<LeaderboardTopItem>,
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after_updated: Option<String>,
    /// 下一页游标：去敏化用户标识（与 `items[].user` 同规则）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after_user: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "rank": 42,
  "score": 13.21,
  "total": 10000,
  "percentile": 99.58
}))]
pub struct MeResponse {
    pub rank: i64,
    pub score: f64,
    pub total: i64,
    pub percentile: f64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "auth": {"sessionToken": "r:abcdefg.hijklmn"},
  "alias": "Alice"
}))]
pub struct AliasRequest {
    pub auth: UnifiedSaveRequest,
    pub alias: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "auth": {"sessionToken": "r:abcdefg.hijklmn"},
  "isPublic": true,
  "showRksComposition": true,
  "showBestTop3": true,
  "showApTop3": true
}))]
pub struct ProfileUpdateRequest {
    pub auth: UnifiedSaveRequest,
    pub is_public: Option<bool>,
    pub show_rks_composition: Option<bool>,
    pub show_best_top3: Option<bool>,
    pub show_ap_top3: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
  "alias": "Alice",
  "score": 14.73,
  "updatedAt": "2025-09-20T04:10:44Z",
  "rksComposition": {"best27Sum": 390.12, "apTop3Sum": 49.20},
  "bestTop3": [{"song":"Tempestissimo","difficulty":"AT","acc":99.43,"rks":15.12}],
  "apTop3": [{"song":"AP Song","difficulty":"IN","acc":100.0,"rks":13.45}]
}))]
pub struct PublicProfileResponse {
    pub alias: String,
    pub score: f64,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rks_composition: Option<RksCompositionText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_top3: Option<Vec<ChartTextItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_top3: Option<Vec<ChartTextItem>>,
}
