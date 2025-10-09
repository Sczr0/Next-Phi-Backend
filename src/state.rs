use std::sync::Arc;

use crate::features::auth::{client::TapTapClient, qrcode_service::QrCodeService};
use crate::features::song::models::SongCatalog;
use crate::startup::chart_loader::ChartConstantsMap;
use crate::features::stats::StatsHandle;

/// 聚合的应用共享状态
#[derive(Clone)]
pub struct AppState {
    pub chart_constants: Arc<ChartConstantsMap>,
    pub song_catalog: Arc<SongCatalog>,
    pub taptap_client: Arc<TapTapClient>,
    pub qrcode_service: Arc<QrCodeService>,
    pub stats: Option<StatsHandle>,
    pub stats_storage: Option<Arc<crate::features::stats::storage::StatsStorage>>,
}
