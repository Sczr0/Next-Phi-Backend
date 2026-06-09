use crate::{
    config::AppConfig,
    error::AppError,
    save_contract::{self, SaveSource},
    startup::chart_loader::ChartConstantsMap,
};

pub(super) async fn fetch_image_save_meta(
    source: SaveSource,
    taptap_version: Option<&str>,
) -> Result<(save_contract::SaveMeta, String), AppError> {
    let meta = save_contract::fetch_save_meta(source, &AppConfig::global().taptap, taptap_version)
        .await
        .map_err(|e| AppError::Internal(format!("获取存档元信息失败: {e}")))?;
    let updated_for_cache = save_updated_cache_version(meta.updated_at.as_deref());

    Ok((meta, updated_for_cache))
}

pub(super) async fn decrypt_image_save_from_meta(
    meta: save_contract::SaveMeta,
    chart_constants: std::sync::Arc<ChartConstantsMap>,
) -> Result<save_contract::ParsedSave, AppError> {
    save_contract::get_decrypted_save_from_meta(meta, chart_constants)
        .await
        .map_err(|e| AppError::Internal(format!("获取存档失败: {e}")))
}

pub(super) fn save_updated_cache_version(updated_at: Option<&str>) -> String {
    updated_at.unwrap_or("none").to_string()
}

pub(super) fn to_save_source(
    req: &crate::auth_contract::UnifiedSaveRequest,
) -> Result<SaveSource, AppError> {
    match (&req.session_token, &req.external_credentials) {
        (Some(token), None) => Ok(SaveSource::official(token.clone())),
        (None, Some(creds)) => Ok(SaveSource::external(creds.clone())),
        (Some(_), Some(_)) => Err(AppError::SaveHandlerError(
            "不能同时提供 sessionToken 和 externalCredentials".into(),
        )),
        (None, None) => Err(AppError::SaveHandlerError(
            "必须提供 sessionToken 或 externalCredentials 中的一项".into(),
        )),
    }
}
