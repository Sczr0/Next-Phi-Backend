use config::{Config as ConfigBuilder, ConfigError, Environment, File};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use sha2::{Digest, Sha256};

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
    /// info 数据目录（包含 difficulty.csv）
    pub info_path: String,
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
    /// 统计配置
    #[serde(default)]
    pub stats: StatsConfig,
    /// 品牌/展示配置
    #[serde(default)]
    pub branding: BrandingConfig,
    /// 水印配置
    #[serde(default)]
    pub watermark: WatermarkConfig,
    /// 优雅退出配置
    #[serde(default)]
    pub shutdown: ShutdownConfig,
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

        builder.try_deserialize()
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
                info_path: "./info".to_string(),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "full".to_string(),
            },
            api: ApiConfig {
                prefix: "/api/v1".to_string(),
            },
            stats: StatsConfig::default(),
            branding: BrandingConfig::default(),
            watermark: WatermarkConfig::default(),
            shutdown: ShutdownConfig::default(),
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
    fn default_parquet() -> bool { true }
    fn default_dir() -> String { "./resources/stats/v1/events".to_string() }
    fn default_compress() -> String { "zstd".to_string() }
}

impl Default for StatsArchiveConfig {
    fn default() -> Self {
        Self { parquet: true, dir: Self::default_dir(), compress: Self::default_compress() }
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
    #[serde(default)]
    pub user_hash_salt: Option<String>,
    /// 展示统计的时区（IANA 名称，如 Asia/Shanghai）
    #[serde(default = "StatsConfig::default_timezone")] 
    pub timezone: String,
    /// 每日聚合与归档时间（本地时区，如 "03:00"）
    #[serde(default = "StatsConfig::default_daily_time")] 
    pub daily_aggregate_time: String,
}

impl StatsConfig {
    fn default_enabled() -> bool { true }
    fn default_storage() -> String { "sqlite".to_string() }
    fn default_sqlite_path() -> String { "./resources/usage_stats.db".to_string() }
    fn default_sqlite_wal() -> bool { true }
    fn default_batch_size() -> usize { 100 }
    fn default_flush_ms() -> u64 { 1000 }
    fn default_retention_days() -> u32 { 180 }
    fn default_timezone() -> String { "Asia/Shanghai".to_string() }
    fn default_daily_time() -> String { "03:00".to_string() }
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
    fn default_explicit() -> bool { true }
    fn default_implicit() -> bool { true }
    fn default_salt() -> String { "phi".to_string() }
    fn default_ttl() -> u64 { 600 }
    fn default_code_len() -> usize { 8 }

    /// 校验解除口令（静态或动态）
    pub fn is_unlock_valid(&self, input: Option<&str>) -> bool {
        let Some(pwd) = input else { return false; };
        if let Some(st) = &self.unlock_static {
            if !st.is_empty() && pwd == st { return true; }
        }
        if self.unlock_dynamic {
            if let Some(cur) = self.current_dynamic_code() {
                if pwd.eq_ignore_ascii_case(&cur) { return true; }
            }
        }
        false
    }

    /// 计算当前窗口的动态口令
    pub fn current_dynamic_code(&self) -> Option<String> {
        if !self.unlock_dynamic { return None; }
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let ttl = self.dynamic_ttl_secs.max(1);
        let window = now / ttl;
        let salt = if self.dynamic_salt.is_empty() { "phi" } else { &self.dynamic_salt };
        let secret = self.dynamic_secret.as_deref().unwrap_or("");
        // 通过 盐值 + 时间窗口 + 可选密钥 计算 SHA-256 哈希，并截取前缀作为口令
        let input = format!("{salt}:{window}:{secret}");
        let hash = Sha256::digest(input.as_bytes());
        let hexed = hex::encode(hash);
        let len = self.dynamic_length.max(4).min(64);
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
    fn default_timeout() -> u64 { 30 }
    fn default_force() -> bool { true }
    fn default_force_delay() -> u64 { 10 }

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
    fn default_enabled() -> bool { false }
    fn default_timeout() -> u64 { 60 }
    fn default_interval() -> u64 { 10 }

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
