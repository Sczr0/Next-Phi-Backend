use axum::{Router, routing::post};

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
    Router::new()
        .route("/image/bn", post(render_bn))
        .route("/image/song", post(render_song))
        .route("/image/bn/user", post(render_bn_user))
}
