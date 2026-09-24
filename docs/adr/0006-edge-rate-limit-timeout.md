# ADR-0006：网关边缘防护（限流 / 请求超时 / 请求体上限）

- 日期：2026-09
- 状态：**已接受**
- 相关章节：ARCHITECTURE.md §6（已确诊病灶 D1/D3）；AGENTS.md 修改纪律（依赖、契约红线）；ADR-0002（双库）

## 背景

大版本上线前的性能复查发现：`/api/v2/*` 全站**无限流、无请求超时、无显式请求体上限**
（`src/router.rs` 此前只有 request_id / CORS / 压缩 / 统计四层）。仅 Open Platform
子路由有一套内存限流（`token_auth/rate_limit.rs`）。

后果：最昂贵的端点（`/image/bn`、`/image/song`、`/save`、`/songs/search`）除渲染信号量
外没有任何节流保护；慢 handler 会长时间占用连接与 tokio worker；超大 body 直接进内存。
这些在 DAU 100–200 的稳态无感，但在大版本上线的流量尖峰下是最可能的雪崩点。

同时，`/image/bn/user` 等路径无缓存、歌曲搜索在 async worker 上跑 CPU 密集扫描
（另见性能修复方案 Phase 2），进一步放大了无节流时的风险。

## 决策

引入**可配置、默认宽松/关闭**的三层网关防护，全部由 `[limits]` 配置段（phi-common
`LimitsConfig`）驱动：

1. **限流**：`governor`（GCRA，`RateLimiter::keyed` + dashmap 状态存储，无全局锁）。
   - 两层：全局限流（宽松）+ 昂贵端点（图片/存档/搜索子路由）更严一层。
   - key = 客户端 IP：优先 `X-Forwarded-For`（取最左）→ `X-Real-IP` → 连接地址；
     **全部缺失兜底 `"unknown"`，绝不因取不到 IP 返回 5xx**。
   - `rate_limit_enabled = false` 为默认：**不装配中间件**，零行为变化。
   - 超限返回 429（`application/problem+json`，code `RATE_LIMITED`，含 request_id）。
   - 静态资源 `/_ill/*`、`/health`、Swagger 文档不参与限流。
2. **请求超时**：`tower_http::timeout::TimeoutLayer`（启用 `tower-http` 的 `timeout`
   feature），超时返回 **504 Gateway Timeout**。默认 30s（对最慢的图片渲染足够宽松）。
3. **请求体上限**：`axum::extract::DefaultBodyLimit`，默认 2MB。JSON 端点超限经既有
   提取器映射为 422（`error.rs` 的 `JsonRejection → AppError::Validation`），原始 body
   端点返回 413。

## 对外契约（C1）影响说明

本 ADR 是 C1「API 面不可变」的**显式受控变更**：新增 429 / 504 / 422·413 三种**仅在
异常或过载时**出现的响应。正常流量下三者均不触发，故对合法客户端逐字节无感。

回滚开关：`[limits] rate_limit_enabled = false`（限流整体关闭）；超时/body 上限可通过
把 `request_timeout_secs` / `max_body_bytes` 调到极大值近似关闭。

## 后果

- 正面：昂贵端点获得明确节流；慢 handler 不再无限占用 worker；超大 body 早拒。
  governor 的 per-key 存储替代了 Open Platform 里 `Mutex<HashMap>` 的全局串行化
  （后者后续可迁移到同一实现，见性能修复方案 Phase 6.3）。
- 负面：新增依赖 `governor`（+ dashmap/quanta/nonzero_ext/futures-timer，均 MIT/Apache，
  进 `deny.toml` 白名单）；`main.rs` 的 `axum::serve` 改为
  `into_make_service_with_connect_info` 以提供直连场景的客户端 IP。
- 默认关闭意味着**上线需显式开启**；建议先以宽松值（如全局 600/min）试跑观察，再收紧。

## 实施状态

与本 ADR 同提交落地：`LimitsConfig`（phi-common）、`src/rate_limit.rs`、`router.rs` 层叠
（body limit + 全局限流 + API 子路由的昂贵限流 + 超时）、`main.rs` ConnectInfo、
`config.example.toml` 模板、`rate_limit` 单测。
