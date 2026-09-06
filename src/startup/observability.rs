//! 可观测性引导：全局 tracing 订阅装配与 Sentry 初始化（ADR-0005）。
//!
//! 设计要点：
//! - Sentry 仅在配置了 DSN 时启用；未配置 = 完全禁用，本地/CI 零开销；
//! - `error!` 日志映射为 Sentry 事件、`warn!` 映射为面包屑，info 以下忽略（控量）；
//! - panic 由 SDK 默认集成的全局钩子捕获（含 tokio task 内 panic），无需中间件；
//! - `std::process::exit` 会跳过 guard 的 drop-flush，失败退出必须走 [`fatal_exit`]。

use sentry::ClientInitGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 装配全局 tracing 订阅：stdout fmt 层（`RUST_LOG` 覆盖，默认
/// `phi_backend=info,tower_http=info`）外挂 Sentry 桥接层。
///
/// 必须在任何日志产生前调用一次；Sentry 未启用时桥接层为无害空转。
pub fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "phi_backend=info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(env_filter))
        .with(sentry::integrations::tracing::layer().event_filter(sentry_event_filter))
        .init();
}

/// Sentry 桥接的事件过滤入口（挂到 [`init_tracing`] 的层上）。
fn sentry_event_filter(md: &tracing::Metadata<'_>) -> sentry::integrations::tracing::EventFilter {
    event_filter_for_level(*md.level())
}

/// [`sentry_event_filter`] 的可测内核：`error!` → 事件，`warn!` → 面包屑，其余忽略。
const fn event_filter_for_level(
    level: tracing::Level,
) -> sentry::integrations::tracing::EventFilter {
    use sentry::integrations::tracing::EventFilter;
    match level {
        tracing::Level::ERROR => EventFilter::Event,
        tracing::Level::WARN => EventFilter::Breadcrumb,
        _ => EventFilter::Ignore,
    }
}

/// 初始化 Sentry 客户端并绑定到主 hub；DSN 缺省/空串时返回 `None`（完全禁用）。
///
/// 调用时机约束：必须先于首个 `tokio::spawn`——worker 线程的 hub 在首次访问时
/// 从主 hub 派生，之后才绑定的 client 不会传播到已派生的空 hub。
pub fn init_sentry(dsn: Option<&str>) -> Option<ClientInitGuard> {
    let dsn = dsn.filter(|d| !d.is_empty())?;
    // 0.49 起 ClientOptions 为 #[non_exhaustive]，禁结构体字面量，走 new() + 字段覆写。
    // sentry::init 内部会 apply_defaults：注入默认 transport（ureq）与默认集成（含 panic），
    // 并兜底读取 SENTRY_DSN / SENTRY_RELEASE / SENTRY_ENVIRONMENT / 代理等环境变量
    //（environment 缺省按构建类型：debug=development，release=production）。
    let mut options = sentry::ClientOptions::new();
    options.release = sentry::release_name!();
    // 不采集用户 IP/Cookie/敏感请求头；用户身份本就是 HMAC 哈希，不外发
    options.send_default_pii = false;
    // release-health 会话跟踪：core 默认关闭，显式开启才能统计崩溃率
    options.auto_session_tracking = true;
    Some(sentry::init((dsn, options)))
}

/// 冲刷 Sentry 后退出（`!`）。
///
/// `std::process::exit` 会绕过 guard 的 drop-flush，启动失败路径直接 exit 会让
/// 事件随进程一起丢失；main 中的失败退出统一改走本函数。
pub fn fatal_exit() -> ! {
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(3)));
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{event_filter_for_level, init_sentry};
    use sentry::integrations::tracing::EventFilter;
    use tracing::Level;

    #[test]
    fn level_filter_maps_error_to_event_and_warn_to_breadcrumb() {
        // EventFilter 是 bitflags 位标志（未实现 PartialEq），单标志返回值用 contains 断言
        assert!(event_filter_for_level(Level::ERROR).contains(EventFilter::Event));
        assert!(event_filter_for_level(Level::WARN).contains(EventFilter::Breadcrumb));
        assert!(event_filter_for_level(Level::INFO).contains(EventFilter::Ignore));
        assert!(event_filter_for_level(Level::DEBUG).contains(EventFilter::Ignore));
    }

    /// DSN 缺省/空串一律不初始化（本地与 CI 零开销的前提）。
    #[test]
    fn init_sentry_is_disabled_without_dsn() {
        assert!(init_sentry(None).is_none());
        assert!(init_sentry(Some("")).is_none());
    }

    /// 合法 DSN 经 apply_defaults（sentry::init 的内部路径）后客户端启用。
    #[test]
    fn valid_dsn_with_applied_defaults_produces_enabled_client() {
        // 复现 sentry::init 的内部注入（默认 transport），不触碰全局 hub/panic 钩子；
        // 注意直接 Client::from 不会注入 transport（transport=None 即 no-op 客户端）
        let options = sentry::apply_defaults(sentry::ClientOptions::new());
        // 回环假主机：仅验证装配路径，不产生外发流量
        let client = sentry::Client::from(("http://b70a331d@example.invalid/1", options));
        assert!(client.is_enabled());
    }
}
