# 部署手册（Charter §8）

> 目标：**单一静态二进制 + 一条 docker compose 命令**即完成部署；升级可回滚、密钥零泄露。

## 1. 三种形态

| 形态 | 适用 | 命令 |
|---|---|---|
| 源码运行 | 开发/调试 | `cargo run --release`（自动回退 config.toml → config.example.toml） |
| 单二进制 | 直接部署（无 Docker 环境） | `cargo build --profile release-dist --target x86_64-unknown-linux-musl`，产物为静态二进制，拷走即用 |
| Docker Compose | 推荐生产 | `docker compose up -d --build` |

> 快速自测（任意形态）：`curl http://127.0.0.1:3939/health` 返回 200；
> 启动后 `resources/usage_stats.db`、`resources/open_platform.db` 自动创建。

## 2. 目录与挂载

| 路径（容器 /app 下） | 内容 | 挂载建议 |
|---|---|---|
| `resources/ill/` | 曲绘仓库（启动自动同步，git2） | 卷（phi-data）；首次启动耗时取决于仓库大小 |
| `resources/*.db` | SQLite：usage_stats.db（统计/榜单/会话）+ open_platform.db | 卷（phi-data）——**备份 = 拷贝这两个文件** |
| `resources/templates/` | SVG 模板（neo/firstlook/default/song） | 卷（phi-data）或容器内 |
| `info/` | difficulty.csv / info.csv / nicklist.yaml（落后自动拉取） | 卷（phi-info） |

## 3. 环境变量清单（密钥只经环境变量，优先级高于配置文件）

| 变量 | 用途 | 注意 |
|---|---|---|
| `APP_SESSION_JWT_SECRET` | access token HS256 签名 | 换 = 在线用户全踢（C3；重叠轮换方案见 ARCHITECTURE §5.2） |
| `APP_SESSION_EXCHANGE_SHARED_SECRET` | /auth/session/exchange 共享密钥 | |
| `APP_SESSION_AUTH_EMBED_SECRET` | 会话凭证 AES-GCM 加密 | 缺省回退 JWT 密钥 |
| `APP_STATS_USER_HASH_SALT` | **用户身份键（HMAC-SHA256 盐）** | ⚠️ **永不轮换**（C2；换盐=全量身份更替事故） |
| `APP_LEADERBOARD_ADMIN_TOKENS` | 管理后台 Bearer 令牌（逗号分隔） | |
| `APP_OPEN_PLATFORM_GITHUB_CLIENT_SECRET` | GitHub OAuth | |
| `APP_WATERMARK_DYNAMIC_SECRET` 等 | 水印/签名 | 详见 config.example.toml 顶注 |

## 4. 升级与回滚（单实例，秒级）

1. 升级前：**备份数据库**（拷贝 `resources/usage_stats.db` 与 `resources/open_platform.db`，可加 `VACUUM INTO 'backup.db'` 获得一致快照——SQLite 在线备份的等价手法）。
2. 升级：新镜像 tag / 新二进制替换后重启（`docker compose up -d`）。
3. 回滚：换回旧镜像/二进制 + 恢复数据库备份。**秒级**（单二进制 + 单文件数据库）。
4. 维护窗：选低峰；`/health` 作为探针，重启窗口通常 <5s（优雅停机广播 + 宽限窗口见 `shutdown.rs`）。

## 5. 国内镜像（可选）

`.cnb.yml` 驱动 cnb.cool 云原生构建，自动推 `:latest` / `:nightly` / tag 到 CNB Docker 制品库，
**国内直拉无需翻墙**：

```bash
docker pull <CNB_DOCKER_REGISTRY>/<CNB_REPO_SLUG_LOWERCASE>:latest
```

启用前提与产物地址说明见 `.cnb.yml` 头注。

## 5b. 双库拆分迁移（D1/ADR-0002——一次性的维护窗操作）

`state_db_path` 已默认写入 config.example.toml。**首次启用（或从旧单库升级）时必须先执行迁移**：

```bash
# 1) 停止服务（维护窗）
# 2) 执行拆分工具（旧库自动备份为 <usage_stats.db>.bak-<ts>）
cargo run -p impl-storage --bin db_split -- ./resources/usage_stats.db ./resources/state.db
# 3) 启动服务（config.toml [stats] state_db_path = "./resources/state.db" 已生效）
```

工具含 **表集互斥断言**（两库各自恰好包含预期表集，任何漏删/多删立即失败并保留快照）+
`VACUUM` + `integrity_check`。回滚：恢复 `.bak-<ts>` + 旧二进制（秒级）。
注：`export` 单文件模式（删除 `state_db_path` 配置）仍向后兼容——未迁移的旧库直接跑双库配置会
因断言缺失表而失败（安全失败，不会静默产生混合库）。

## 6. CI 冒烟验证

`.github/workflows/docker-build.yml`：push 触碰构建面（Dockerfile/crates/Cargo.lock/config.example/工作流）时，
在 GitHub runner 上**真实构建镜像 + 起容器验证 `/health` 200 + SQLite 文件生成**——防止"能编译不能跑"的部署漂移。
（本项目仓库当前为私人仓库；请在 GitHub 项目设置中开放 Actions 权限后生效。）

## 7. 已知部署约束

- 镜像 amd64 为主（musl 静态二进制）；arm64 需要交叉编译 variant（TODO：发布矩阵扩展）。
- 曲绘仓库同步需要 GitHub 出网；无网环境可预置 `resources/ill/` 于挂载卷（启动自动检测跳过）。
- 系统字体：容器已内置 Noto CJK（中文曲名渲染必需）；宿主机直跑二进制时需安装相应字体，
  否则 B27/单曲图中文显示为 tofu（`fontdb::load_system_fonts` 扫描系统字体目录）。
