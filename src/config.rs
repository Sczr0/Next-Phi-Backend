use config::{Config as ConfigBuilder, ConfigError, Environment, File};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// 全局配置单例
static CONFIG: OnceCell<AppConfig> = OnceCell::new();

/// 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 监听地址
    pub host: String,
    /// 监听端口
    pub port: u16,
}

/// 资源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesConfig {
    /// 资源基础路径
    pub base_path: String,
    /// 曲绘仓库 URL
    pub illustration_repo: String,
    /// 曲绘文件夹名
    pub illustration_folder: String,
    /// 曲绘外部资源基地址（HTTP），用于不依赖 Git/本地仓库时按需回源（例如 https://somnia.xtower.site）
    #[serde(default)]
    pub illustration_external_base_url: Option<String>,
    /// 启动时是否自动同步曲绘仓库（默认关闭，避免启动强依赖外部网络/Git）
    #[serde(default = "ResourcesConfig::default_illustration_repo_auto_sync")]
    pub illustration_repo_auto_sync: bool,
    /// info 数据目录（包含 difficulty.csv）
    pub info_path: String,
}

impl ResourcesConfig {
    fn default_illustration_repo_auto_sync() -> bool {
        false
    }
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// 日志级别
    pub level: String,
    /// 日志格式
    pub format: String,
}

/// API 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// API 路由前缀
    pub prefix: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "SessionConfig::default_enabled")]
    pub enabled: bool,
    #[serde(default = "SessionConfig::default_jwt_issuer")]
    pub jwt_issuer: String,
    #[serde(default = "SessionConfig::default_jwt_audience")]
    pub jwt_audience: String,
    #[serde(default = "SessionConfig::default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "SessionConfig::default_access_ttl_secs")]
    pub access_ttl_secs: u64,
    #[serde(default = "SessionConfig::default_revoke_all_grace_secs")]
    pub revoke_all_grace_secs: u64,
    #[serde(default = "SessionConfig::default_revoke_ttl_secs")]
    pub revoke_ttl_secs: u64,
    #[serde(default = "SessionConfig::default_exchange_shared_secret")]
    pub exchange_shared_secret: String,
}
impl SessionConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_jwt_issuer() -> String {
        "phi-backend".to_string()
    }
    fn default_jwt_audience() -> String {
        "phi-clients".to_string()
    }
    fn default_jwt_secret() -> String {
        std::env::var("APP_SESSION_JWT_SECRET").unwrap_or_default()
    }
    fn default_access_ttl_secs() -> u64 {
        900
    }
    fn default_revoke_all_grace_secs() -> u64 {
        10
    }
    fn default_revoke_ttl_secs() -> u64 {
        864_000
    }
    fn default_exchange_shared_secret() -> String {
        std::env::var("APP_SESSION_EXCHANGE_SHARED_SECRET").unwrap_or_default()
    }
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            jwt_issuer: Self::default_jwt_issuer(),
            jwt_audience: Self::default_jwt_audience(),
            jwt_secret: Self::default_jwt_secret(),
            access_ttl_secs: Self::default_access_ttl_secs(),
            revoke_all_grace_secs: Self::default_revoke_all_grace_secs(),
            revoke_ttl_secs: Self::default_revoke_ttl_secs(),
            exchange_shared_secret: Self::default_exchange_shared_secret(),
        }
    }
}
/// 开放平台 GitHub OAuth 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPlatformGithubConfig {
    /// GitHub OAuth client_id
    #[serde(default = "OpenPlatformGithubConfig::default_client_id")]
    pub client_id: String,
    /// GitHub OAuth client_secret
    #[serde(default = "OpenPlatformGithubConfig::default_client_secret")]
    pub client_secret: String,
    /// GitHub OAuth 回调地址
    #[serde(default = "OpenPlatformGithubConfig::default_redirect_uri")]
    pub redirect_uri: String,
    /// OAuth scope
    #[serde(default = "OpenPlatformGithubConfig::default_scope")]
    pub scope: String,
    /// GitHub OAuth 授权地址
    #[serde(default = "OpenPlatformGithubConfig::default_authorize_url")]
    pub authorize_url: String,
    /// GitHub OAuth token 交换地址
    #[serde(default = "OpenPlatformGithubConfig::default_token_url")]
    pub token_url: String,
    /// GitHub API 基地址
    #[serde(default = "OpenPlatformGithubConfig::default_api_base_url")]
    pub api_base_url: String,
    /// 登录成功后跳转地址（控制台）
    #[serde(default = "OpenPlatformGithubConfig::default_post_login_redirect")]
    pub post_login_redirect: String,
    /// OAuth state 有效期（秒）
    #[serde(default = "OpenPlatformGithubConfig::default_state_ttl_secs")]
    pub state_ttl_secs: u64,
    /// OAuth HTTP 超时（秒）
    #[serde(default = "OpenPlatformGithubConfig::default_http_timeout_secs")]
    pub http_timeout_secs: u64,
    /// OAuth HTTP 重试次数（仅网络波动时）
    #[serde(default = "OpenPlatformGithubConfig::default_http_retry_count")]
    pub http_retry_count: u32,
}

impl OpenPlatformGithubConfig {
    fn default_client_id() -> String {
        std::env::var("APP_OPEN_PLATFORM_GITHUB_CLIENT_ID").unwrap_or_default()
    }
    fn default_client_secret() -> String {
        std::env::var("APP_OPEN_PLATFORM_GITHUB_CLIENT_SECRET").unwrap_or_default()
    }
    fn default_redirect_uri() -> String {
        std::env::var("APP_OPEN_PLATFORM_GITHUB_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3939/api/v2/auth/github/callback".into())
    }
    fn default_scope() -> String {
        "read:user user:email".to_string()
    }
    fn default_authorize_url() -> String {
        "https://github.com/login/oauth/authorize".to_string()
    }
    fn default_token_url() -> String {
        "https://github.com/login/oauth/access_token".to_string()
    }
    fn default_api_base_url() -> String {
        "https://api.github.com".to_string()
    }
    fn default_post_login_redirect() -> String {
        "/open-platform".to_string()
    }
    fn default_state_ttl_secs() -> u64 {
        600
    }
    fn default_http_timeout_secs() -> u64 {
        10
    }
    fn default_http_retry_count() -> u32 {
        2
    }
}

impl Default for OpenPlatformGithubConfig {
    fn default() -> Self {
        Self {
            client_id: Self::default_client_id(),
            client_secret: Self::default_client_secret(),
            redirect_uri: Self::default_redirect_uri(),
            scope: Self::default_scope(),
            authorize_url: Self::default_authorize_url(),
            token_url: Self::default_token_url(),
            api_base_url: Self::default_api_base_url(),
            post_login_redirect: Self::default_post_login_redirect(),
            state_ttl_secs: Self::default_state_ttl_secs(),
            http_timeout_secs: Self::default_http_timeout_secs(),
            http_retry_count: Self::default_http_retry_count(),
        }
    }
}

/// 开放平台开发者会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPlatformSessionConfig {
    /// 开发者会话 JWT 发行方
    #[serde(default = "OpenPlatformSessionConfig::default_jwt_issuer")]
    pub jwt_issuer: String,
    /// 开发者会话 JWT 受众
    #[serde(default = "OpenPlatformSessionConfig::default_jwt_audience")]
    pub jwt_audience: String,
    /// 开发者会话 JWT 密钥
    #[serde(default = "OpenPlatformSessionConfig::default_jwt_secret")]
    pub jwt_secret: String,
    /// 会话有效期（秒）
    #[serde(default = "OpenPlatformSessionConfig::default_ttl_secs")]
    pub ttl_secs: u64,
    /// 会话 cookie 名称
    #[serde(default = "OpenPlatformSessionConfig::default_cookie_name")]
    pub cookie_name: String,
    /// 会话 cookie 是否强制 Secure
    #[serde(default = "OpenPlatformSessionConfig::default_cookie_secure")]
    pub cookie_secure: bool,
}

impl OpenPlatformSessionConfig {
    fn default_jwt_issuer() -> String {
        "phi-open-platform".to_string()
    }
    fn default_jwt_audience() -> String {
        "phi-open-platform-console".to_string()
    }
    fn default_jwt_secret() -> String {
        std::env::var("APP_OPEN_PLATFORM_SESSION_JWT_SECRET").unwrap_or_default()
    }
    fn default_ttl_secs() -> u64 {
        86_400
    }
    fn default_cookie_name() -> String {
        "op_session".to_string()
    }
    fn default_cookie_secure() -> bool {
        false
    }
}

impl Default for OpenPlatformSessionConfig {
    fn default() -> Self {
        Self {
            jwt_issuer: Self::default_jwt_issuer(),
            jwt_audience: Self::default_jwt_audience(),
            jwt_secret: Self::default_jwt_secret(),
            ttl_secs: Self::default_ttl_secs(),
            cookie_name: Self::default_cookie_name(),
            cookie_secure: Self::default_cookie_secure(),
        }
    }
}

/// 开放平台 API Key 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPlatformApiKeyConfig {
    /// live key 前缀
    #[serde(default = "OpenPlatformApiKeyConfig::default_live_prefix")]
    pub live_prefix: String,
    /// test key 前缀
    #[serde(default = "OpenPlatformApiKeyConfig::default_test_prefix")]
    pub test_prefix: String,
    /// key hash 使用的服务端密钥（建议通过环境变量注入）
    #[serde(default = "OpenPlatformApiKeyConfig::default_hash_secret")]
    pub hash_secret: String,
    /// key 随机体字节数（越大越难猜）
    #[serde(default = "OpenPlatformApiKeyConfig::default_random_bytes")]
    pub random_bytes: usize,
    /// 轮换默认过渡窗口（秒），0 表示立即失效旧 key
    #[serde(default = "OpenPlatformApiKeyConfig::default_rotate_grace_secs")]
    pub rotate_grace_secs: u64,
    /// 默认 scopes
    #[serde(default = "OpenPlatformApiKeyConfig::default_scopes")]
    pub default_scopes: Vec<String>,
    /// 每分钟限流阈值（按 key_id + client_ip）
    #[serde(default = "OpenPlatformApiKeyConfig::default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
}

impl OpenPlatformApiKeyConfig {
    fn default_live_prefix() -> String {
        "pgr_live_".to_string()
    }
    fn default_test_prefix() -> String {
        "pgr_test_".to_string()
    }
    fn default_hash_secret() -> String {
        std::env::var("APP_OPEN_PLATFORM_API_KEY_HASH_SECRET").unwrap_or_default()
    }
    fn default_random_bytes() -> usize {
        24
    }
    fn default_rotate_grace_secs() -> u64 {
        86_400
    }
    fn default_scopes() -> Vec<String> {
        vec!["public.read".to_string()]
    }
    fn default_rate_limit_per_minute() -> u32 {
        120
    }
}

impl Default for OpenPlatformApiKeyConfig {
    fn default() -> Self {
        Self {
            live_prefix: Self::default_live_prefix(),
            test_prefix: Self::default_test_prefix(),
            hash_secret: Self::default_hash_secret(),
            random_bytes: Self::default_random_bytes(),
            rotate_grace_secs: Self::default_rotate_grace_secs(),
            default_scopes: Self::default_scopes(),
            rate_limit_per_minute: Self::default_rate_limit_per_minute(),
        }
    }
}

/// 开放平台配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPlatformConfig {
    /// 是否启用开放平台
    #[serde(default = "OpenPlatformConfig::default_enabled")]
    pub enabled: bool,
    /// 开放平台 SQLite 路径
    #[serde(default = "OpenPlatformConfig::default_sqlite_path")]
    pub sqlite_path: String,
    /// 开放平台 SQLite 是否启用 WAL
    #[serde(default = "OpenPlatformConfig::default_sqlite_wal")]
    pub sqlite_wal: bool,
    /// GitHub OAuth 配置
    #[serde(default)]
    pub github: OpenPlatformGithubConfig,
    /// 开发者会话配置
    #[serde(default)]
    pub session: OpenPlatformSessionConfig,
    /// API Key 配置
    #[serde(default)]
    pub api_key: OpenPlatformApiKeyConfig,
}

impl OpenPlatformConfig {
    fn default_enabled() -> bool {
        false
    }
    fn default_sqlite_path() -> String {
        "./resources/open_platform.db".to_string()
    }
    fn default_sqlite_wal() -> bool {
        true
    }
}

impl Default for OpenPlatformConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            sqlite_path: Self::default_sqlite_path(),
            sqlite_wal: Self::default_sqlite_wal(),
            github: OpenPlatformGithubConfig::default(),
            session: OpenPlatformSessionConfig::default(),
            api_key: OpenPlatformApiKeyConfig::default(),
        }
    }
}
/// CORS 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// 是否启用 CORS
    #[serde(default = "CorsConfig::default_enabled")]
    pub enabled: bool,
    /// 允许的 Origin 列表（支持 "*" 表示任意）
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// 允许的方法列表（支持 "*" 表示任意）
    #[serde(default)]
    pub allowed_methods: Vec<String>,
    /// 允许的请求头列表（支持 "*" 表示任意）
    #[serde(default)]
    pub allowed_headers: Vec<String>,
    /// 暴露的响应头列表（支持 "*" 表示任意）
    #[serde(default)]
    pub expose_headers: Vec<String>,
    /// 是否允许携带凭证（Cookie/Authorization）
    #[serde(default = "CorsConfig::default_allow_credentials")]
    pub allow_credentials: bool,
    /// 预检缓存时间（秒）
    #[serde(default)]
    pub max_age_secs: Option<u64>,
}

impl CorsConfig {
    fn default_enabled() -> bool {
        false
    }

    fn default_allow_credentials() -> bool {
        false
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            allowed_origins: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_headers: Vec::new(),
            expose_headers: Vec::new(),
            allow_credentials: Self::default_allow_credentials(),
            max_age_secs: None,
        }
    }
}

/// TapTap 版本枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TapTapVersion {
    /// 大陆版
    #[default]
    CN,
    /// 国际版
    Global,
}

/// TapTap API 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapTapConfig {
    /// 设备码请求端点
    pub device_code_endpoint: String,
    /// Token交换端点
    pub token_endpoint: String,
    /// 用户基本信息端点
    pub user_info_endpoint: String,
    /// LeanCloud API Base URL
    pub leancloud_base_url: String,
    /// LeanCloud App ID
    pub leancloud_app_id: String,
    /// LeanCloud App Key
    pub leancloud_app_key: String,
}

/// 多版本 TapTap 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapTapMultiConfig {
    /// 大陆版配置
    pub cn: TapTapConfig,
    /// 国际版配置
    pub global: TapTapConfig,
    /// 默认版本
    pub default_version: TapTapVersion,
}

/// 品牌/展示配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrandingConfig {
    /// 右下角自定义文字（留空则不显示）
    #[serde(default)]
    pub footer_text: String,
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub resources: ResourcesConfig,
    pub logging: LoggingConfig,
    pub api: ApiConfig,
    /// CORS 配置
    #[serde(default)]
    pub cors: CorsConfig,
    /// TapTap API 配置（多版本）
    #[serde(default)]
    pub taptap: TapTapMultiConfig,
    /// 统计配置
    #[serde(default)]
    pub stats: StatsConfig,
    /// 会话令牌配置
    #[serde(default)]
    pub session: SessionConfig,
    /// 开放平台配置
    #[serde(default)]
    pub open_platform: OpenPlatformConfig,
    /// 品牌/展示配置
    #[serde(default)]
    pub branding: BrandingConfig,
    /// 水印配置
    #[serde(default)]
    pub watermark: WatermarkConfig,
    /// 图片渲染配置
    #[serde(default)]
    pub image: ImageRenderConfig,
    /// /save 接口防护配置（下载/解压/entry 上限）
    #[serde(default)]
    pub save: SaveLimitsConfig,
    /// 优雅退出配置
    #[serde(default)]
    pub shutdown: ShutdownConfig,
    /// 排行榜配置（纯文字）
    #[serde(default)]
    pub leaderboard: LeaderboardConfig,
}

impl AppConfig {
    /// 从配置文件加载配置，支持环境变量覆盖
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::get_config_path();

        tracing::info!("正在从 {:?} 加载配置文件", config_path);

        let builder = ConfigBuilder::builder()
            // 加载配置文件
            .add_source(File::with_name(config_path.to_str().unwrap()))
            // 支持环境变量覆盖，例如：APP_API_PREFIX
            .add_source(
                Environment::with_prefix("APP")
                    .separator("_")
                    .try_parsing(true),
            )
            .build()?;

        let config: Self = builder.try_deserialize()?;

        // 调试：打印 user_hash_salt 配置状态
        tracing::debug!(
            "配置加载完成: user_hash_salt = {:?}",
            config
                .stats
                .user_hash_salt
                .as_deref()
                .map(|s| format!("{}...", &s[..s.len().min(4)]))
        );

        Ok(config)
    }

    /// 获取全局配置单例
    pub fn global() -> &'static AppConfig {
        CONFIG.get().expect("配置未初始化，请先调用 init_global()")
    }

    /// 初始化全局配置
    pub fn init_global() -> Result<(), ConfigError> {
        let config = Self::load()?;
        CONFIG
            .set(config)
            .map_err(|_| ConfigError::Message("配置已经被初始化".to_string()))?;
        Ok(())
    }

    /// 获取配置文件路径
    fn get_config_path() -> PathBuf {
        PathBuf::from("config.toml")
    }

    /// 获取服务器监听地址
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// 获取资源文件夹路径
    pub fn resources_path(&self) -> PathBuf {
        PathBuf::from(&self.resources.base_path)
    }

    /// 获取曲绘文件夹完整路径
    pub fn illustration_path(&self) -> PathBuf {
        self.resources_path()
            .join(&self.resources.illustration_folder)
    }

    /// 获取 info 数据目录
    pub fn info_path(&self) -> PathBuf {
        PathBuf::from(&self.resources.info_path)
    }
}

impl Default for TapTapConfig {
    fn default() -> Self {
        Self {
            device_code_endpoint: "https://www.taptap.com/oauth2/v1/device/code".to_string(),
            token_endpoint: "https://www.taptap.cn/oauth2/v1/token".to_string(),
            user_info_endpoint: "https://open.tapapis.cn/account/basic-info/v1".to_string(),
            leancloud_base_url: "https://rak3ffdi.cloud.tds1.tapapis.cn/1.1".to_string(),
            leancloud_app_id: "rAK3FfdieFob2Nn8Am".to_string(),
            leancloud_app_key: "Qr9AEqtuoSVS3zeD6iVbM4ZC0AtkJcQ89tywVyi0".to_string(),
        }
    }
}

impl Default for TapTapMultiConfig {
    fn default() -> Self {
        Self {
            cn: TapTapConfig::default(),
            global: TapTapConfig {
                device_code_endpoint: "https://www.taptap.io/oauth2/v1/device/code".to_string(),
                token_endpoint: "https://www.taptap.io/oauth2/v1/token".to_string(),
                user_info_endpoint: "https://open.tapapis.io/account/basic-info/v1".to_string(),
                leancloud_base_url: "https://rak3ffdi.cloud.tds1.tapapis.io/1.1".to_string(),
                leancloud_app_id: "rAK3FfdieFob2Nn8Am".to_string(),
                leancloud_app_key: "Qr9AEqtuoSVS3zeD6iVbM4ZC0AtkJcQ89tywVyi0".to_string(),
            },
            default_version: TapTapVersion::default(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3939,
            },
            resources: ResourcesConfig {
                base_path: "./resources".to_string(),
                illustration_repo: "https://github.com/Catrong/phi-plugin-ill".to_string(),
                illustration_folder: "phi-plugin-ill".to_string(),
                illustration_external_base_url: None,
                illustration_repo_auto_sync: ResourcesConfig::default_illustration_repo_auto_sync(),
                info_path: "./info".to_string(),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "full".to_string(),
            },
            api: ApiConfig {
                prefix: "/api/v2".to_string(),
            },
            cors: CorsConfig::default(),
            taptap: TapTapMultiConfig::default(),
            stats: StatsConfig::default(),
            session: SessionConfig::default(),
            open_platform: OpenPlatformConfig::default(),
            branding: BrandingConfig::default(),
            watermark: WatermarkConfig::default(),
            image: ImageRenderConfig::default(),
            save: SaveLimitsConfig::default(),
            shutdown: ShutdownConfig::default(),
            leaderboard: LeaderboardConfig::default(),
        }
    }
}

/// 图片渲染配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRenderConfig {
    /// 是否优先速度渲染（OptimizeSpeed），提升栅格化性能，可能略降画质
    #[serde(default)]
    pub optimize_speed: bool,
    /// 是否启用图片缓存（BN/单曲）
    #[serde(default = "ImageRenderConfig::default_cache_enabled")]
    pub cache_enabled: bool,
    /// 缓存最大容量（字节），按图片字节大小加权
    #[serde(default = "ImageRenderConfig::default_cache_max_bytes")]
    pub cache_max_bytes: u64,
    /// 缓存 TTL（秒）
    #[serde(default = "ImageRenderConfig::default_cache_ttl")]
    pub cache_ttl_secs: u64,
    /// 缓存 TTI（秒）
    #[serde(default = "ImageRenderConfig::default_cache_tti")]
    pub cache_tti_secs: u64,
    /// 并发渲染许可数（0=自动，取 CPU 核心数）
    #[serde(default)]
    pub max_parallel: u32,
    /// 用户自报成绩 BN：scores 条数硬上限（0=不限制，不建议）
    #[serde(default = "ImageRenderConfig::default_max_user_scores")]
    pub max_user_scores: u32,
}

impl ImageRenderConfig {
    fn default_cache_enabled() -> bool {
        true
    }
    fn default_cache_max_bytes() -> u64 {
        100 * 1024 * 1024
    }
    fn default_cache_ttl() -> u64 {
        60
    }
    fn default_cache_tti() -> u64 {
        30
    }
    fn default_max_user_scores() -> u32 {
        500
    }
}

impl Default for ImageRenderConfig {
    fn default() -> Self {
        Self {
            optimize_speed: false,
            cache_enabled: Self::default_cache_enabled(),
            cache_max_bytes: Self::default_cache_max_bytes(),
            cache_ttl_secs: Self::default_cache_ttl(),
            cache_tti_secs: Self::default_cache_tti(),
            max_parallel: 0,
            max_user_scores: Self::default_max_user_scores(),
        }
    }
}

/// /save 接口资源上限配置（用于 P0 级防护）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveLimitsConfig {
    /// 下载存档最大字节数（超限直接拒绝）
    #[serde(default = "SaveLimitsConfig::default_max_download_bytes")]
    pub max_download_bytes: u64,
    /// 解压后最大字节数（gzip/zlib）
    #[serde(default = "SaveLimitsConfig::default_max_decompress_bytes")]
    pub max_decompress_bytes: u64,
    /// 单个 zip entry 最大字节数
    #[serde(default = "SaveLimitsConfig::default_max_zip_entry_bytes")]
    pub max_zip_entry_bytes: u64,
    /// zip 内允许的最大 entry 数
    #[serde(default = "SaveLimitsConfig::default_max_zip_entries")]
    pub max_zip_entries: u32,
    /// 是否启用 /save 解析结果缓存
    #[serde(default = "SaveLimitsConfig::default_cache_enabled")]
    pub cache_enabled: bool,
    /// /save 缓存最大条目数（0=禁用）
    #[serde(default = "SaveLimitsConfig::default_cache_max_entries")]
    pub cache_max_entries: u64,
    /// /save 缓存 TTL（秒）
    #[serde(default = "SaveLimitsConfig::default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
    /// /save 缓存 TTI（秒）
    #[serde(default = "SaveLimitsConfig::default_cache_tti_secs")]
    pub cache_tti_secs: u64,
    /// PBKDF2 rounds 下限（防止异常小值）
    #[serde(default = "SaveLimitsConfig::default_pbkdf2_rounds_min")]
    pub pbkdf2_rounds_min: u32,
    /// PBKDF2 rounds 上限（防止异常大值）
    #[serde(default = "SaveLimitsConfig::default_pbkdf2_rounds_max")]
    pub pbkdf2_rounds_max: u32,
}

impl SaveLimitsConfig {
    fn default_max_download_bytes() -> u64 {
        64 * 1024 * 1024
    }

    fn default_max_decompress_bytes() -> u64 {
        64 * 1024 * 1024
    }

    fn default_max_zip_entry_bytes() -> u64 {
        32 * 1024 * 1024
    }

    fn default_max_zip_entries() -> u32 {
        16
    }

    fn default_cache_enabled() -> bool {
        true
    }

    fn default_cache_max_entries() -> u64 {
        512
    }

    fn default_cache_ttl_secs() -> u64 {
        120
    }

    fn default_cache_tti_secs() -> u64 {
        60
    }

    fn default_pbkdf2_rounds_min() -> u32 {
        1_000
    }

    fn default_pbkdf2_rounds_max() -> u32 {
        100_000
    }
}

impl Default for SaveLimitsConfig {
    fn default() -> Self {
        Self {
            max_download_bytes: Self::default_max_download_bytes(),
            max_decompress_bytes: Self::default_max_decompress_bytes(),
            max_zip_entry_bytes: Self::default_max_zip_entry_bytes(),
            max_zip_entries: Self::default_max_zip_entries(),
            cache_enabled: Self::default_cache_enabled(),
            cache_max_entries: Self::default_cache_max_entries(),
            cache_ttl_secs: Self::default_cache_ttl_secs(),
            cache_tti_secs: Self::default_cache_tti_secs(),
            pbkdf2_rounds_min: Self::default_pbkdf2_rounds_min(),
            pbkdf2_rounds_max: Self::default_pbkdf2_rounds_max(),
        }
    }
}

/// 统计归档配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsArchiveConfig {
    /// 是否启用 Parquet 归档
    #[serde(default = "StatsArchiveConfig::default_parquet")]
    pub parquet: bool,
    /// 归档目录
    #[serde(default = "StatsArchiveConfig::default_dir")]
    pub dir: String,
    /// 压缩算法：none|zstd|snappy（仅对 Parquet 生效）
    #[serde(default = "StatsArchiveConfig::default_compress")]
    pub compress: String,
}

impl StatsArchiveConfig {
    fn default_parquet() -> bool {
        true
    }
    fn default_dir() -> String {
        "./resources/stats/v1/events".to_string()
    }
    fn default_compress() -> String {
        "zstd".to_string()
    }
}

impl Default for StatsArchiveConfig {
    fn default() -> Self {
        Self {
            parquet: true,
            dir: Self::default_dir(),
            compress: Self::default_compress(),
        }
    }
}

/// 统计配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsConfig {
    /// 是否启用统计
    #[serde(default = "StatsConfig::default_enabled")]
    pub enabled: bool,
    /// 起始统计时间（ISO8601 可选），早于此时间的事件可忽略
    #[serde(default)]
    pub start_at: Option<String>,
    /// 存储类型：sqlite
    #[serde(default = "StatsConfig::default_storage")]
    pub storage: String,
    /// SQLite 文件路径
    #[serde(default = "StatsConfig::default_sqlite_path")]
    pub sqlite_path: String,
    /// 是否启用 WAL
    #[serde(default = "StatsConfig::default_sqlite_wal")]
    pub sqlite_wal: bool,
    /// 批量大小
    #[serde(default = "StatsConfig::default_batch_size")]
    pub batch_size: usize,
    /// 刷新间隔（毫秒）
    #[serde(default = "StatsConfig::default_flush_ms")]
    pub flush_interval_ms: u64,
    /// 热数据保留天数
    #[serde(default = "StatsConfig::default_retention_days")]
    pub retention_hot_days: u32,
    /// 归档配置
    #[serde(default)]
    pub archive: StatsArchiveConfig,
    /// 用户哈希盐
    #[serde(default, alias = "user-hash-salt", alias = "userHashSalt")]
    pub user_hash_salt: Option<String>,
    /// 展示统计的时区（IANA 名称，如 Asia/Shanghai）
    #[serde(default = "StatsConfig::default_timezone")]
    pub timezone: String,
    /// 每日聚合与归档时间（本地时区，如 "03:00"）
    #[serde(default = "StatsConfig::default_daily_time")]
    pub daily_aggregate_time: String,
}

impl StatsConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_storage() -> String {
        "sqlite".to_string()
    }
    fn default_sqlite_path() -> String {
        "./resources/usage_stats.db".to_string()
    }
    fn default_sqlite_wal() -> bool {
        true
    }
    fn default_batch_size() -> usize {
        100
    }
    fn default_flush_ms() -> u64 {
        1000
    }
    fn default_retention_days() -> u32 {
        180
    }
    fn default_timezone() -> String {
        "Asia/Shanghai".to_string()
    }
    fn default_daily_time() -> String {
        "03:00".to_string()
    }
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            start_at: None,
            storage: Self::default_storage(),
            sqlite_path: Self::default_sqlite_path(),
            sqlite_wal: true,
            batch_size: Self::default_batch_size(),
            flush_interval_ms: Self::default_flush_ms(),
            retention_hot_days: Self::default_retention_days(),
            archive: StatsArchiveConfig::default(),
            user_hash_salt: None,
            timezone: Self::default_timezone(),
            daily_aggregate_time: Self::default_daily_time(),
        }
    }
}

/// 水印配置（默认启用显式与隐式水印）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    #[serde(default = "WatermarkConfig::default_explicit")]
    pub explicit_badge: bool,
    #[serde(default = "WatermarkConfig::default_implicit")]
    pub implicit_pixel: bool,
    /// 静态解除口令（为空或缺省则不启用）
    #[serde(default)]
    pub unlock_static: Option<String>,
    /// 是否启用动态解除口令（打印在日志中）
    #[serde(default)]
    pub unlock_dynamic: bool,
    /// 动态口令盐值
    #[serde(default = "WatermarkConfig::default_salt")]
    pub dynamic_salt: String,
    /// 动态口令有效期（秒）
    #[serde(default = "WatermarkConfig::default_ttl")]
    pub dynamic_ttl_secs: u64,
    /// 参与动态口令的可选密钥（提高口令复杂度）
    #[serde(default)]
    pub dynamic_secret: Option<String>,
    /// 生成口令的长度（从hex哈希前缀截取）
    #[serde(default = "WatermarkConfig::default_code_len")]
    pub dynamic_length: usize,
}

impl WatermarkConfig {
    fn default_explicit() -> bool {
        true
    }
    fn default_implicit() -> bool {
        true
    }
    fn default_salt() -> String {
        "phi".to_string()
    }
    fn default_ttl() -> u64 {
        600
    }
    fn default_code_len() -> usize {
        8
    }

    /// 校验解除口令（静态或动态）
    pub fn is_unlock_valid(&self, input: Option<&str>) -> bool {
        let Some(pwd) = input else {
            return false;
        };
        if let Some(st) = &self.unlock_static
            && !st.is_empty()
            && pwd == st
        {
            return true;
        }
        if self.unlock_dynamic
            && let Some(cur) = self.current_dynamic_code()
            && pwd.eq_ignore_ascii_case(&cur)
        {
            return true;
        }
        false
    }

    /// 计算当前窗口的动态口令
    pub fn current_dynamic_code(&self) -> Option<String> {
        if !self.unlock_dynamic {
            return None;
        }
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let ttl = self.dynamic_ttl_secs.max(1);
        let window = now / ttl;
        let salt = if self.dynamic_salt.is_empty() {
            "phi"
        } else {
            &self.dynamic_salt
        };
        let secret = self.dynamic_secret.as_deref().unwrap_or("");
        // 通过 盐值 + 时间窗口 + 可选密钥 计算 SHA-256 哈希，并截取前缀作为口令
        let input = format!("{salt}:{window}:{secret}");
        let hash = Sha256::digest(input.as_bytes());
        let hexed = hex::encode(hash);
        let len = self.dynamic_length.clamp(4, 64);
        Some(hexed[..len].to_string())
    }
}

impl Default for WatermarkConfig {
    fn default() -> Self {
        Self {
            explicit_badge: true,
            implicit_pixel: true,
            unlock_static: None,
            unlock_dynamic: false,
            dynamic_salt: Self::default_salt(),
            dynamic_ttl_secs: Self::default_ttl(),
            dynamic_secret: None,
            dynamic_length: Self::default_code_len(),
        }
    }
}

/// 优雅退出配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownConfig {
    /// 优雅退出超时时间（秒）
    #[serde(default = "ShutdownConfig::default_timeout")]
    pub timeout_secs: u64,
    /// 是否启用强制退出
    #[serde(default = "ShutdownConfig::default_force")]
    pub force_quit: bool,
    /// 强制退出前的等待时间（秒）
    #[serde(default = "ShutdownConfig::default_force_delay")]
    pub force_delay_secs: u64,
    /// Linux systemd 看门狗配置
    #[serde(default)]
    pub watchdog: WatchdogConfig,
}

impl ShutdownConfig {
    fn default_timeout() -> u64 {
        30
    }
    fn default_force() -> bool {
        true
    }
    fn default_force_delay() -> u64 {
        10
    }

    /// 获取优雅退出超时时间
    pub fn timeout_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs)
    }

    /// 获取强制退出等待时间
    pub fn force_delay_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.force_delay_secs)
    }
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            timeout_secs: Self::default_timeout(),
            force_quit: Self::default_force(),
            force_delay_secs: Self::default_force_delay(),
            watchdog: WatchdogConfig::default(),
        }
    }
}

/// systemd 看门狗配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// 是否启用看门狗
    #[serde(default = "WatchdogConfig::default_enabled")]
    pub enabled: bool,
    /// 看门狗超时时间（秒）
    #[serde(default = "WatchdogConfig::default_timeout")]
    pub timeout_secs: u64,
    /// 心跳间隔时间（秒）
    #[serde(default = "WatchdogConfig::default_interval")]
    pub interval_secs: u64,
}

impl WatchdogConfig {
    fn default_enabled() -> bool {
        false
    }
    fn default_timeout() -> u64 {
        60
    }
    fn default_interval() -> u64 {
        10
    }

    /// 获取看门狗超时时间
    pub fn timeout_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs)
    }

    /// 获取心跳间隔时间
    pub fn interval_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.interval_secs)
    }

    /// 获取心跳间隔（纳秒，用于sd_notify）
    pub fn interval_nanos(&self) -> u64 {
        self.interval_secs * 1_000_000_000
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            timeout_secs: Self::default_timeout(),
            interval_secs: Self::default_interval(),
        }
    }
}

/// 排行榜配置（无图片，纯文字）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardConfig {
    /// 是否启用排行榜功能
    #[serde(default = "LeaderboardConfig::default_enabled")]
    pub enabled: bool,
    /// 是否允许公开资料
    #[serde(default = "LeaderboardConfig::default_allow_public")]
    pub allow_public: bool,
    /// 默认展示 RKS 构成（Best27+APTop3）
    #[serde(default = "LeaderboardConfig::default_show_rc")]
    pub default_show_rks_composition: bool,
    /// 默认展示 BestTop3
    #[serde(default = "LeaderboardConfig::default_show_b3")]
    pub default_show_best_top3: bool,
    /// 默认展示 APTop3
    #[serde(default = "LeaderboardConfig::default_show_ap3")]
    pub default_show_ap_top3: bool,
    /// 管理员令牌列表（Header: X-Admin-Token）
    #[serde(
        default = "LeaderboardConfig::default_admin_tokens",
        alias = "admin-tokens",
        alias = "adminTokens"
    )]
    pub admin_tokens: Vec<String>,
}

impl LeaderboardConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_allow_public() -> bool {
        true
    }
    fn default_show_rc() -> bool {
        true
    }
    fn default_show_b3() -> bool {
        true
    }
    fn default_show_ap3() -> bool {
        true
    }
    fn default_admin_tokens() -> Vec<String> {
        if let Ok(raw) = std::env::var("APP_LEADERBOARD_ADMIN_TOKENS") {
            return raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        Vec::new()
    }
}

impl Default for LeaderboardConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            allow_public: Self::default_allow_public(),
            default_show_rks_composition: Self::default_show_rc(),
            default_show_best_top3: Self::default_show_b3(),
            default_show_ap_top3: Self::default_show_ap3(),
            admin_tokens: Self::default_admin_tokens(),
        }
    }
}
