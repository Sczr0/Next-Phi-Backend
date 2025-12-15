use axum::body::Bytes;
use moka::future::Cache;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::features::auth::{client::TapTapClient, qrcode_service::QrCodeService};
use crate::features::song::models::SongCatalog;
use crate::features::stats::StatsHandle;
use crate::startup::chart_loader::ChartConstantsMap;

/// 聚合的应用共享状态
#[derive(Clone)]
pub struct AppState {
    pub chart_constants: Arc<ChartConstantsMap>,
    pub song_catalog: Arc<SongCatalog>,
    pub taptap_client: Arc<TapTapClient>,
    pub qrcode_service: Arc<QrCodeService>,
    pub stats: Option<StatsHandle>,
    pub stats_storage: Option<Arc<crate::features::stats::storage::StatsStorage>>,
    /// 控制并发渲染的信号量（限制 CPU 密集型任务数量）
    pub render_semaphore: Arc<Semaphore>,
    /// BN 图片缓存（按图片字节大小加权）
    pub bn_image_cache: Cache<String, Bytes>,
    /// 单曲图片缓存（按图片字节大小加权）
    pub song_image_cache: Cache<String, Bytes>,
}
