use once_cell::sync::OnceCell;
use reqwest::Client;
use std::time::Duration;

/// 全局复用的 HTTP Client（统一连接池/Keep-Alive），避免每次请求重复创建。
///
/// 说明：
/// - 保持与旧实现一致：不同调用点原本使用不同的 timeout，这里按 timeout 维度拆分 client。
/// - `Client` 本身是线程安全的，适合全局复用。
static CLIENT_DEFAULT: OnceCell<Client> = OnceCell::new();
static CLIENT_TIMEOUT_30S: OnceCell<Client> = OnceCell::new();
static CLIENT_TIMEOUT_90S: OnceCell<Client> = OnceCell::new();

/// 默认配置的 HTTP Client（不额外设置 timeout），用于“尽力而为”的辅助请求。
pub fn client_default() -> Result<&'static Client, reqwest::Error> {
    CLIENT_DEFAULT.get_or_try_init(|| Client::builder().build())
}

/// timeout=30s 的 HTTP Client（用于元信息/短请求），与旧实现一致。
pub fn client_timeout_30s() -> Result<&'static Client, reqwest::Error> {
    CLIENT_TIMEOUT_30S
        .get_or_try_init(|| Client::builder().timeout(Duration::from_secs(30)).build())
}

/// timeout=90s 的 HTTP Client（用于下载存档等大请求），与旧实现一致。
pub fn client_timeout_90s() -> Result<&'static Client, reqwest::Error> {
    CLIENT_TIMEOUT_90S
        .get_or_try_init(|| Client::builder().timeout(Duration::from_secs(90)).build())
}
