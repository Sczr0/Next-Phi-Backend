//! 提取器包装：让 `Query`/`Json`/`Path` 提取失败统一返回 422 problem+json。
//!
//! axum 0.7 的提取器失败时默认直接 `rejection.into_response()`（text/plain 的 400/415），
//! 与项目"参数校验失败 → 422 VALIDATION_FAILED（application/problem+json）"的惯例不一致。
//! 这里提供与 `parse_json_with_bearer_state` 相同模式的包装提取器，把 rejection 转为
//! [`AppError::Validation`]。

use std::ops::{Deref, DerefMut};

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Path, Query, Request},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// 校验版 `Query` 提取器：失败时返回 422 VALIDATION_FAILED。
pub struct ValidatedQuery<T>(pub T);

impl<T> Deref for ValidatedQuery<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ValidatedQuery<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// axum 0.8 的 FromRequestParts 已是原生 async fn in trait，不再需要 async_trait 宏。
impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let q = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|err| AppError::Validation(format!("查询参数无效: {err}")))?;
        Ok(Self(q.0))
    }
}

/// 校验版 `Path` 提取器：失败时返回 422 VALIDATION_FAILED。
pub struct ValidatedPath<T>(pub T);

impl<T> Deref for ValidatedPath<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ValidatedPath<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// axum 0.8 的 FromRequestParts 已是原生 async fn in trait，不再需要 async_trait 宏。
impl<S, T> FromRequestParts<S> for ValidatedPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let p = Path::<T>::from_request_parts(parts, state)
            .await
            .map_err(|err| AppError::Validation(format!("路径参数无效: {err}")))?;
        Ok(Self(p.0))
    }
}

/// 校验版 `Json` 提取器：失败时返回 422 VALIDATION_FAILED。
pub struct ValidatedJson<T>(pub T);

impl<T> Deref for ValidatedJson<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ValidatedJson<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// axum 0.8 的 FromRequest 已是原生 async fn in trait，不再需要 async_trait 宏。
impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let j = Json::<T>::from_request(req, state)
            .await
            .map_err(|err| AppError::Validation(format!("请求体 JSON 无效: {err}")))?;
        Ok(Self(j.0))
    }
}
