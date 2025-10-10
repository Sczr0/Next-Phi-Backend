# Phi-Backend 部署指南（基于代码）

本文档严格依据仓库代码与配置实现，说明如何在常见环境下构建与部署本服务（Axum + Tokio）。

## 功能与端点

- 默认监听地址与端口：由 `config.toml` 的 `[server]` 控制（默认 `0.0.0.0:3939`）
- 公开端点：
  - 健康检查：`GET /health`
  - API 文档（Swagger UI）：`GET /docs`
  - OpenAPI 文档：`GET /api-docs/openapi.json`
  - 业务前缀：`/api/v1`（可通过配置覆盖）

## 前置条件

- 已安装支持 `edition = 2024` 的 Rust 工具链与 Cargo
- 服务器需能访问 GitHub（首次启动会克隆曲绘仓库：`https://github.com/Catrong/phi-plugin-ill`）
- 准备数据目录：`./info`（默认），需包含：
  - `difficulty.csv`
  - `info.csv`
  - `nicklist.yaml`
- 可选字体（提升渲染效果）：`resources/fonts/Source Han Sans & Saira Hybrid-Regular #5446.ttf`

备注：首次启动会自动创建 `resources/` 目录，并在 `resources/<illustration_folder>` 下克隆或更新曲绘仓库（默认 `ill`，由配置控制）。

## 构建

```bash
cargo build --release
```

产物位置：
- Linux/macOS：`./target/release/phi-backend`
- Windows：`.\n+target\release\phi-backend.exe`

可设置日志级别：

```bash
RUST_LOG=phi_backend=info,tower_http=info ./target/release/phi-backend
```

## 配置（config.toml + 环境变量覆盖）

应用在工作目录查找 `config.toml`（代码中固定为相对路径 `config.toml`），并允许被前缀为 `APP_` 的环境变量覆盖，分隔符为下划线 `_`。例如：

- `APP_SERVER_HOST=127.0.0.1` 覆盖 `[server].host`
- `APP_SERVER_PORT=8080` 覆盖 `[server].port`
- `APP_API_PREFIX=/api/v2` 覆盖 `[api].prefix`
- `APP_RESOURCES_BASE_PATH=/data/phi/resources` 覆盖 `[resources].base_path`
- `APP_STATS_ENABLED=false` 覆盖 `[stats].enabled`
- `APP_STATS_SQLITE_PATH=/data/phi/usage_stats.db` 覆盖 `[stats].sqlite_path`
- `APP_BRANDING_FOOTER_TEXT=Powered by Phi` 覆盖 `[branding].footer_text`
- `APP_WATERMARK_UNLOCK_DYNAMIC=true` 覆盖 `[watermark].unlock_dynamic`
- `APP_SHUTDOWN_WATCHDOG_ENABLED=true` 覆盖 `[shutdown.watchdog].enabled`

最小可用配置示例（与仓库根目录中的 `config.toml` 等价）：

```toml
[server]
host = "0.0.0.0"
port = 3939

[api]
prefix = "/api/v1"

[resources]
base_path = "./resources"
illustration_repo = "https://github.com/Catrong/phi-plugin-ill"
illustration_folder = "ill"
info_path = "./info"

[logging]
level = "info"
format = "full"

[branding]
footer_text = ""

[stats]
enabled = true
storage = "sqlite"
sqlite_path = "./resources/usage_stats.db"
sqlite_wal = true
batch_size = 100
flush_interval_ms = 1000
retention_hot_days = 180
daily_aggregate_time = "03:00"

[stats.archive]
parquet = true
dir = "./resources/stats/v1/events"
compress = "zstd"

[watermark]
explicit_badge = true
implicit_pixel = true
unlock_static = ""
unlock_dynamic = true
dynamic_salt = "phi"
dynamic_ttl_secs = 10
dynamic_secret = ""
dynamic_length = 32

[shutdown]
timeout_secs = 30
force_quit = true
force_delay_secs = 10

[shutdown.watchdog]
enabled = false
timeout_secs = 60
interval_secs = 10
```

说明：代码中未实现命令行参数解析，无法通过 `--config` 指定配置路径；如需放置到其他目录，请配合 `WorkingDirectory`（例如 systemd）或环境变量覆盖。

## 配置项详解（逐项说明）

环境变量覆盖规则：以 `APP_` 为前缀，使用下划线连接层级名，全部大写。例如 `APP_SERVER_HOST` 覆盖 `[server].host`。

### [server]
- host（字符串，默认 0.0.0.0）
  - 监听地址；生产环境通常保持 0.0.0.0 以便外部访问
  - 环境变量：APP_SERVER_HOST
- port（整数，默认 3939）
  - 监听端口
  - 环境变量：APP_SERVER_PORT

### [api]
- prefix（字符串，默认 /api/v1）
  - 所有业务路由的前缀（不影响 /health 与 /docs）
  - 环境变量：APP_API_PREFIX

### [resources]
- base_path（字符串，默认 ./resources）
  - 运行时资源根目录；首次启动会自动创建
  - 环境变量：APP_RESOURCES_BASE_PATH
- illustration_repo（字符串，默认 https://github.com/Catrong/phi-plugin-ill）
  - 曲绘仓库地址；启动时若本地不存在将克隆，存在则尝试更新
  - 环境变量：APP_RESOURCES_ILLUSTRATION_REPO
- illustration_folder（字符串，默认 ill）
  - 存放曲绘仓库的子目录名：`<base_path>/<illustration_folder>`
  - 环境变量：APP_RESOURCES_ILLUSTRATION_FOLDER
- info_path（字符串，默认 ./info）
  - 歌曲元数据目录，必须包含 `difficulty.csv`、`info.csv`、`nicklist.yaml`
  - 环境变量：APP_RESOURCES_INFO_PATH

### [logging]
- level（字符串，默认 info）
  - 期望的日志级别；当前主流程使用 `RUST_LOG` 环境变量控制更直接
  - 环境变量：APP_LOGGING_LEVEL（仅作为配置占位，主流程未直接消费）
- format（字符串，默认 full）
  - 日志格式占位：full/compact/pretty/json（当前主流程未直接消费）
  - 环境变量：APP_LOGGING_FORMAT

提示：实际运行时建议通过 `RUST_LOG=phi_backend=info,tower_http=info` 控制日志；上述 logging 配置为未来扩展预留。

### [branding]
- footer_text（字符串，默认空）
  - 图片渲染右下角自定义文字；为空则不显示
  - 环境变量：APP_BRANDING_FOOTER_TEXT

### [stats]
- enabled（布尔，默认 true）
  - 是否启用统计采集与归档；关闭后相关路由仍存在但可能返回空
  - 环境变量：APP_STATS_ENABLED
- start_at（可选字符串，默认 null）
  - ISO8601 起始时间；早于该时间的事件可忽略（当前主要用于后续扩展）
  - 环境变量：APP_STATS_START_AT
- storage（字符串，默认 sqlite）
  - 明细存储类型；当前仅支持 sqlite
  - 环境变量：APP_STATS_STORAGE
- sqlite_path（字符串，默认 ./resources/usage_stats.db）
  - SQLite 文件路径；建议放置到数据盘
  - 环境变量：APP_STATS_SQLITE_PATH
- sqlite_wal（布尔，默认 true）
  - 是否启用 WAL，提升并发写入性能
  - 环境变量：APP_STATS_SQLITE_WAL
- batch_size（整数，默认 100）
  - 批量插入大小；过大可能增加尾延迟，过小影响吞吐
  - 环境变量：APP_STATS_BATCH_SIZE
- flush_interval_ms（整数，默认 1000）
  - 后台 flush 周期（毫秒）；在低流量时保障数据落盘
  - 环境变量：APP_STATS_FLUSH_INTERVAL_MS
- retention_hot_days（整数，默认 180）
  - 热数据保留天数（在线 SQLite）；当前仅作为策略占位
  - 环境变量：APP_STATS_RETENTION_HOT_DAYS
- user_hash_salt（可选字符串，默认 null）
  - 用户去敏哈希盐；不设置则不记录 user_hash/client_ip_hash
  - 强烈建议通过环境变量注入，勿写入仓库
  - 环境变量：APP_STATS_USER_HASH_SALT
- timezone（字符串，默认 Asia/Shanghai）
  - 展示统计的时区（IANA 名称），用于接口聚合展示
  - 环境变量：APP_STATS_TIMEZONE
- daily_aggregate_time（字符串，默认 03:00）
  - 每日归档触发时间（本地时区），归档“前一日”明细为 Parquet
  - 环境变量：APP_STATS_DAILY_AGGREGATE_TIME
- archive（对象）
  - 见下节 `[stats.archive]`

### [stats.archive]
- parquet（布尔，默认 true）
  - 是否导出 Parquet 文件
  - 环境变量：APP_STATS_ARCHIVE_PARQUET
- dir（字符串，默认 ./resources/stats/v1/events）
  - Parquet 根目录，按 year=YYYY/month=MM/day=DD 分区
  - 环境变量：APP_STATS_ARCHIVE_DIR
- compress（字符串，默认 zstd）
  - 压缩算法：zstd | snappy | none（其他值视为 none）
  - 环境变量：APP_STATS_ARCHIVE_COMPRESS

### [watermark]
- explicit_badge（布尔，默认 true）
  - 显式水印：在图片上以标记方式体现
  - 环境变量：APP_WATERMARK_EXPLICIT_BADGE
- implicit_pixel（布尔，默认 true）
  - 隐式水印：在 PNG 首像素写入可追踪标记
  - 环境变量：APP_WATERMARK_IMPLICIT_PIXEL
- unlock_static（可选字符串，默认空字符串/None）
  - 静态解除口令；填写后，提交正确口令可关闭水印
  - 环境变量：APP_WATERMARK_UNLOCK_STATIC
- unlock_dynamic（布尔，默认 false 或示例中为 true）
  - 动态解除口令；启用后服务会在日志周期性打印当前口令
  - 环境变量：APP_WATERMARK_UNLOCK_DYNAMIC
- dynamic_salt（字符串，默认 phi）
  - 动态口令盐
  - 环境变量：APP_WATERMARK_DYNAMIC_SALT
- dynamic_ttl_secs（整数，默认 600 或示例 10）
  - 动态口令有效期（秒），窗口越短越难被复用
  - 环境变量：APP_WATERMARK_DYNAMIC_TTL_SECS
- dynamic_secret（可选字符串，默认 null）
  - 参与口令生成的额外密钥，提高复杂度
  - 环境变量：APP_WATERMARK_DYNAMIC_SECRET
- dynamic_length（整数，默认 8 或示例 32）
  - 取 SHA-256 hex 的前缀长度（4~64）
  - 环境变量：APP_WATERMARK_DYNAMIC_LENGTH

安全提示：静态/动态口令属于敏感信息，建议仅通过环境变量注入，避免写入仓库或镜像层。

### [shutdown]
- timeout_secs（整数，默认 30）
  - 优雅退出总超时；超过后进入强制退出分支
  - 环境变量：APP_SHUTDOWN_TIMEOUT_SECS
- force_quit（布尔，默认 true）
  - 超时后是否强制退出
  - 环境变量：APP_SHUTDOWN_FORCE_QUIT
- force_delay_secs（整数，默认 10）
  - 触发强退前的等待时间
  - 环境变量：APP_SHUTDOWN_FORCE_DELAY_SECS
- watchdog（对象）
  - 见下节 `[shutdown.watchdog]`

### [shutdown.watchdog]
- enabled（布尔，默认 false）
  - 启用 systemd 看门狗；仅 Linux 且在 systemd 环境有效
  - 环境变量：APP_SHUTDOWN_WATCHDOG_ENABLED
- timeout_secs（整数，默认 60）
  - systemd 端期望的超时（需同时在 unit 中设置 `WatchdogSec=` 并 `Type=notify`）
  - 环境变量：APP_SHUTDOWN_WATCHDOG_TIMEOUT_SECS
- interval_secs（整数，默认 10）
  - 心跳发送间隔；建议小于 WatchdogSec 的一半
  - 环境变量：APP_SHUTDOWN_WATCHDOG_INTERVAL_SECS

注意：要让看门狗真正生效，需要同时满足：
1) 配置中开启 `[shutdown.watchdog].enabled=true`
2) systemd unit 设置 `Type=notify` 和合适的 `WatchdogSec=`

## 运行

### 直接运行

```bash
./target/release/phi-backend   # Linux/macOS
.
target\release\phi-backend.exe  # Windows
```

启动日志将打印：绑定地址、文档与健康检查 URL、曲绘目录、以及（启用时）动态水印口令。

### 使用 systemd（Linux，推荐）

1) 创建目录与用户（示例）：

```bash
sudo useradd -r -s /usr/sbin/nologin phi || true
sudo mkdir -p /opt/phi-backend/{resources,info}
sudo chown -R phi:phi /opt/phi-backend
```

2) 放置二进制与配置：

```bash
sudo cp target/release/phi-backend /opt/phi-backend/
sudo cp config.toml /opt/phi-backend/
sudo cp -r info/* /opt/phi-backend/info/
sudo chown -R phi:phi /opt/phi-backend
sudo chmod +x /opt/phi-backend/phi-backend
```

3) 创建服务单元 `/etc/systemd/system/phi-backend.service`：

```ini
[Unit]
Description=Phi Backend Service
After=network-online.target
Wants=network-online.target

[Service]
User=phi
Group=phi
WorkingDirectory=/opt/phi-backend
ExecStart=/opt/phi-backend/phi-backend
Environment=RUST_LOG=phi_backend=info,tower_http=info
# 可选：统计用户去敏盐
# Environment=APP_STATS_USER_HASH_SALT=your-secret-salt
Restart=on-failure
RestartSec=5s

# 如需启用 systemd 看门狗（需同时在 config.toml 打开 [shutdown.watchdog].enabled）
# Type=notify
# WatchdogSec=60s

[Install]
WantedBy=multi-user.target
```

4) 启动与查看：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now phi-backend
sudo systemctl status phi-backend
sudo journalctl -u phi-backend -f
```

## 统计与持久化

- 在线明细：SQLite 文件位于 `[stats].sqlite_path`（默认 `./resources/usage_stats.db`）
- 每日归档：按本地时区 `[stats].daily_aggregate_time` 导出 Parquet 至 `[stats.archive].dir`
- 详见仓库根目录《STATS.md》

## 首次启动与离线准备

- 首次启动会：
  - 自动创建 `resources/` 目录
  - 检测 `resources/<illustration_folder>` 是否存在，不存在则克隆，存在则尝试更新
  - 检查字体是否存在（缺失仅警告，不阻断启动）
- 离线环境建议：提前将曲绘仓库内容放入 `resources/<illustration_folder>`，并保证写权限

## 健康检查与验证

```bash
curl http://127.0.0.1:3939/health
# 期望返回：{"status":"healthy","service":"phi-backend","version":"..."}

# 打开文档
# http://127.0.0.1:3939/docs
```

## 常见问题

- 端口占用：调整 `[server].port` 或释放占用
- 无法克隆曲绘仓库：检查到 GitHub 的网络连通；离线模式请预置资源
- SQLite 权限：确保进程对 `./resources` 有读写权限（WAL 默认开启）
- 配置未生效：确认工作目录下存在 `config.toml` 或使用 `APP_` 环境变量覆盖

## 升级

替换二进制并重启服务即可；启动时会自动尝试更新曲绘仓库。升级前建议备份 `config.toml` 与 `resources/`。

## 安全与上线建议

- 建议置于反向代理之后（TLS 终止、限流、IP 过滤）
- 不要将 `APP_STATS_USER_HASH_SALT` 等敏感值写入仓库；改用环境变量
- 最小权限运行（独立用户、限制写权限至 `resources/`）