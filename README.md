# Phi-Backend

Phi-Backend 是一个为 **Phigros 玩家社区** 提供成绩查询、图片渲染与榜单服务的高性能后端项目，为社区工具与站点提供存档解析、B27 成绩图生成、RKS 排行榜、开放平台 API 等核心能力支持。

基于 Rust 2024 + axum + SQLite 构建，无 Node 运行时依赖，单二进制部署。

## 功能特性

- **认证（Auth）**：TapTap 二维码登录、Session 签发 / 刷新 / 登出、Session Token 换取 user-id
- **存档服务（Save）**：拉取官方存档并解密解析，输出成绩明细、RKS 与推分数据
- **成绩图渲染（Image）**：B27 总览图、单曲成绩图、用户 B27 图（SVG 模板 + 服务端 PNG 渲染），内置曲绘仓库同步与代理（`/_ill`），支持图片签名校验
- **排行榜（Leaderboard）**：RKS 实时排行榜（Top / 按名次 / 个人）、别名与公开资料、管理员扫描可疑用户与封禁
- **RKS 历史（RKS）**：个人 RKS 历史曲线数据
- **开放平台（Open Platform）**：开发者注册、API Key 管理与配额，对外开放成绩图 / 排行榜 / RKS / 歌曲搜索等 API（可独立开关）
- **统计（Stats）**：全量请求埋点、每日预聚合、DAU / HTTP / 延迟统计，对外提供公开统计查询接口
- **其他**：健康检查、优雅停机、systemd watchdog、Swagger UI 在线文档

## 技术栈

| 领域 | 选型 |
| --- | --- |
| 语言 / 运行时 | Rust 2024, tokio |
| Web 框架 | axum 0.7 |
| 数据库 | SQLite（sqlx 异步连接池，WAL 模式） |
| 图片渲染 | minijinja（SVG 模板）+ resvg / tiny-skia（PNG 光栅化） |
| 缓存 | moka（成绩图、统计结果） |
| API 文档 | utoipa + Swagger UI |
| 加密 | aes-gcm / pbkdf2 / ed25519（存档解密与图片签名） |
| 协议 | AGPL-3.0 |

## 快速开始

### 1. 环境要求

- Rust 工具链（edition 2024）
- 网络可访问（拉取曲绘仓库 / 远端 info 数据）

### 2. 准备配置

```bash
cp config.example.toml config.toml
```

按需修改 `config.toml`，所有配置项均有中文注释说明。

### 3. 准备资源

启动时会自动处理（也可手动预置）：

- `resources/ill`：Phigros 曲绘仓库（自动从 GitHub 同步）
- `info/`：歌曲与难度数据（`difficulty.csv`、`info.csv`、`nicklist.yaml`，本地落后时自动从远端拉取）

### 4. 运行

```bash
cargo run --release
```

启动成功后：

```
API:      http://127.0.0.1:3939/api/v2
文档:     http://127.0.0.1:3939/docs
健康检查: http://127.0.0.1:3939/health
```

## 主要 API

所有接口挂在 `/api/v2` 前缀下，完整定义见 Swagger UI（`/docs`）：

| 模块 | 路由示例 |
| --- | --- |
| 认证 | `POST /auth/qrcode`、`POST /auth/session/exchange` |
| 存档 | `POST /save` |
| 成绩图 | `POST /image/bn`、`POST /image/song`、`POST /image/bn/user` |
| 排行榜 | `GET /leaderboard/rks/top`、`GET /leaderboard/rks/by-rank`、`PUT /leaderboard/alias` |
| RKS | `POST /rks/history` |
| 统计 | `GET /stats/daily`、`GET /stats/summary`、`GET /stats/daily/dau` |
| 开放平台 | `POST /open/save`、`GET /open/leaderboard/rks/top`、`GET /open/songs/search`、`GET /auth/github/login`、`GET /developer/api-keys`（需在配置中启用） |

## 目录结构

```
├── src/
│   ├── features/          # 业务功能模块
│   │   ├── auth/          # 登录认证与 Session
│   │   ├── save/          # 存档拉取与解析
│   │   ├── image/         # 成绩图渲染 / 曲绘代理
│   │   ├── leaderboard/   # 排行榜与管理员工具
│   │   ├── rks/           # RKS 历史
│   │   ├── song/          # 歌曲搜索
│   │   ├── stats/         # 统计埋点 / 预聚合 / 查询
│   │   ├── open_platform/ # 开放平台（开发者 / API Key / 开放 API）
│   │   └── health/        # 健康检查
│   ├── bin/
│   │   ├── admin_cli.rs   # 本地管理工具（扫描可疑用户 / 封禁等）
│   │   └── save_inspect.rs# 存档解密链路诊断工具
│   ├── config.rs          # 配置加载
│   └── router.rs          # 路由装配与中间件
├── crates/phi-save-codec/ # 存档加解密 codec（独立 crate）
├── resources/             # 曲绘 / 模板 / SQLite 数据文件
├── info/                  # 歌曲与难度数据
├── sdk/ts/                # TypeScript SDK（由 OpenAPI 生成）
├── tests/                 # 集成测试（含 B27 性能测试，见 tests/README.md）
└── config.example.toml    # 配置模板
```

## 附带工具

```bash
# 管理员命令行（查看/扫描/封禁用户）
cargo run --bin admin_cli -- --help

# 存档诊断（拉取官方存档并输出解密链路信息）
cargo run --bin save_inspect -- --help
```

## 测试

```bash
cargo test
```

B27 渲染性能测试等特殊用例见 [`tests/README.md`](tests/README.md)。

## License

[AGPL-3.0](LICENSE)
