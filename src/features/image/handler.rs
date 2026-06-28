use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[cfg(test)]
use crate::save_contract::{Difficulty, DifficultyRecord};

pub(crate) mod bn;
mod bn_compute;
mod context;
mod display;
mod nickname;
mod output;
mod runtime;
mod save_flow;
mod score;
pub(crate) mod song;
mod song_compute;
pub(crate) mod user_bn;
mod user_bn_compute;

pub use bn::render_bn;
pub use output::ImageQueryOpts;
pub use song::render_song;
pub use user_bn::render_bn_user;

#[cfg(test)]
use context::{
    derive_image_user_identity, ensure_image_user_not_banned, image_cache_enabled,
    image_footer_text,
};
#[cfg(test)]
use display::{format_data_string, parse_challenge_rank, parse_update_time_or_now};
#[cfg(test)]
use output::{ImageOutputCacheSpec, content_type_from_fmt_code, format_code};
#[cfg(test)]
use save_flow::save_updated_cache_version;
#[cfg(test)]
use score::{
    build_engine_records_from_game_record, build_engine_records_from_render_records,
    calculate_ap_top_3_avg, calculate_best_27_avg, calculate_push_acc_map, collect_ap_top_3_scores,
    difficulty_from_canonical_label, difficulty_index, find_song_engine_record_indices,
    index_song_difficulty_records, is_user_score_full_combo, parse_user_score_difficulty,
    sort_engine_records_by_rks_desc, sort_render_records_by_rks_desc, user_score_difficulty_error,
};

fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests;

pub fn create_image_router() -> Router<AppState> {
    let mut router = Router::new()
        .route("/image/bn", post(render_bn))
        .route("/image/song", post(render_song))
        .route("/image/bn/user", post(render_bn_user));

    // 签名验证端点（仅在配置 public_verify=true 时可用；
    // 也可以在路由层始终注册，handler 内部根据配置决定是否响应）
    if crate::config::AppConfig::global()
        .image
        .signing
        .public_verify
    {
        router = router.route("/verify", post(verify_image));
    }

    // 始终注册 GET 版本（方便浏览器直接访问）
    router = router.route("/verify", get(verify_image_get));

    // Ed25519 公钥查询（始终可用，便于客户端获取验签公钥）
    router = router.route("/verify/public-key", get(get_public_key));

    router
}

// ── 验证端点 ──

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    /// SVG 字符串
    pub svg: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_hash_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    // ── v4-beta 字段 ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merkle_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ed_sig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 服务端 Ed25519 公钥（v4-beta），客户端可据此自行验签。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

#[utoipa::path(
    post,
    path = "/verify",
    summary = "验证图片签名",
    description = "验证 SVG 中的 lilith-sig 签名，确保图片由本服务器合法生成。",
    request_body = VerifyRequest,
    responses(
        (status = 200, description = "验证结果", body = VerifyResponse),
        (status = 400, description = "请求参数错误", body = crate::error::ProblemDetails),
        (status = 500, description = "服务器内部错误", body = crate::error::ProblemDetails)
    ),
    tag = "Image"
)]
pub async fn verify_image(
    State(_state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, AppError> {
    let cfg = &crate::config::AppConfig::global().image.signing;
    if !cfg.is_usable() {
        return Ok(Json(VerifyResponse {
            valid: false,
            signed_at: None,
            user_hash_prefix: None,
            request_id: None,
            content_hash: None,
            nonce: None,
            merkle_root: None,
            score_count: None,
            ed_sig: None,
            version: None,
            public_key: None,
            error: Some("服务端未配置签名密钥".into()),
        }));
    }

    match crate::features::image::signing::verify_svg_signature(&req.svg, cfg) {
        Ok(sig) => {
            let signed_at = chrono::DateTime::from_timestamp(sig.timestamp.cast_signed(), 0)
                .map(|dt| dt.to_rfc3339());
            let version = if sig.ed_sig.is_some() {
                Some("v4-beta".to_string())
            } else if !sig.hmac.is_empty() {
                Some("v3".to_string())
            } else {
                None
            };
            Ok(Json(VerifyResponse {
                valid: true,
                signed_at,
                user_hash_prefix: sig.user_hash_prefix,
                request_id: sig.request_id,
                content_hash: Some(sig.content_hash),
                nonce: Some(sig.nonce),
                merkle_root: sig.merkle_root,
                score_count: sig.score_count,
                ed_sig: sig.ed_sig,
                version,
                public_key: cfg.effective_ed25519_public_key(),
                error: None,
            }))
        }
        Err(e) => Ok(Json(VerifyResponse {
            valid: false,
            signed_at: None,
            user_hash_prefix: None,
            request_id: None,
            content_hash: None,
            nonce: None,
            merkle_root: None,
            score_count: None,
            ed_sig: None,
            version: None,
            public_key: None,
            error: Some(e.to_string()),
        })),
    }
}

/// GET 版本的验证端点（便于浏览器直接访问）。
/// Query 参数：?svg=<url_encoded_svg>
#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub svg: Option<String>,
}

#[utoipa::path(
    get,
    path = "/verify",
    summary = "验证图片签名（GET）",
    description = "通过 Query 参数 `svg` 传递 SVG 内容进行验证。",
    params(
        ("svg" = Option<String>, Query, description = "待验证的 SVG 字符串"),
    ),
    responses(
        (status = 200, description = "验证结果", body = VerifyResponse),
    ),
    tag = "Image"
)]
pub async fn verify_image_get(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<VerifyQuery>,
) -> Result<Json<VerifyResponse>, AppError> {
    let svg = q.svg.as_deref().unwrap_or("");
    if svg.is_empty() {
        return Ok(Json(VerifyResponse {
            valid: false,
            signed_at: None,
            user_hash_prefix: None,
            request_id: None,
            content_hash: None,
            nonce: None,
            merkle_root: None,
            score_count: None,
            ed_sig: None,
            version: None,
            public_key: None,
            error: Some("缺少 svg 参数".into()),
        }));
    }
    verify_image(
        State(state),
        Json(VerifyRequest {
            svg: svg.to_string(),
        }),
    )
    .await
}

// ── Ed25519 公钥查询 ──

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyResponse {
    /// 签名协议版本（"v4-beta" 或 null）
    pub version: Option<String>,
    /// Ed25519 公钥（64 hex），未启用 v4 时为 null
    pub public_key: Option<String>,
}

#[utoipa::path(
    get,
    path = "/verify/public-key",
    summary = "获取 Ed25519 公钥",
    description = "返回服务端 v4-beta 签名所用的 Ed25519 公钥。客户端拿到后即可脱离服务端独立验签。",
    responses(
        (status = 200, description = "公钥信息", body = PublicKeyResponse),
    ),
    tag = "Image"
)]
pub async fn get_public_key() -> Json<PublicKeyResponse> {
    let cfg = &crate::config::AppConfig::global().image.signing;
    let (version, pk) = if let Some(pk) = cfg.effective_ed25519_public_key() {
        (Some("v4-beta".to_string()), Some(pk))
    } else if cfg.effective_key().is_some() {
        (Some("v3".to_string()), None)
    } else {
        (None, None)
    };
    Json(PublicKeyResponse {
        version,
        public_key: pk,
    })
}
