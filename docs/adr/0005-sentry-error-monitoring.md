# ADR-0005：接入 Sentry 错误监控（panic + error 日志上报）

- 日期：2026-09
- 状态：**已接受**
- 相关章节：AGENTS.md 修改纪律（配置/密钥、依赖方向）；ARCHITECTURE.md（组合根）

## 背景

服务运行于 ECS（systemd 托管），此前故障只能靠 systemd 日志事后翻查：tokio task
内的 panic 被吞为 JoinError、启动失败、`tracing::error!` 告警均无主动通知与聚合
视图。需要一种零侵入的错误监控：崩溃栈、错误聚合去重、按版本的健康度统计。

## 决策

1. **选型**：Sentry SaaS + 官方 `sentry = "0.49"` Rust SDK（免费档容量满足当前
   事件量，聚合/告警/release-health 闭环现成）。备选自建上报（Grafana Loki 等）
   需自建聚合与告警，无现成闭环，不选。
2. **依赖形态**：SDK 只进组合根 `phi-backend`，契约/impl crate 零感知（依赖方向
   纪律不受影响；C1-C4 均未触碰）。`default-features = false`：SDK 默认 transport
   硬绑 `reqwest + native-tls`，与 musl 静态构建（muslrust 容器）冲突，且 SDK 依赖
   的 reqwest 是 0.13（业务在用 0.12），会引入第二份 reqwest；故显式启用
   `ureq`（自带 rustls，纯 Rust，musl 安全）+ `backtrace/contexts/panic/release-health/tracing`。
3. **采集面**：
   - panic：SDK 默认集成全局钩子，覆盖 tokio task 内 panic，不需要 tower 中间件；
   - 日志桥接：`sentry-tracing` 层挂入 registry，`error!` → 事件、`warn!` → 面包屑、
     info 以下忽略（控量）；不接 tower 层（axum handler 返回 Response 而非 Err，
     Err 捕获面几乎为零），后续需要请求上下文再增量启用 `tower` feature；
   - `send_default_pii = false`：不采集 IP/Cookie/敏感请求头；用户身份本就是
     HMAC 哈希（C2），不外发。
   - `auto_session_tracking = true`：core 默认关闭，显式开启 release-health 会话
     跟踪才能按进程统计崩溃率。
4. **配置**：`[sentry] dsn` 段（phi-common `AppConfig`），空 = 完全禁用（本地/CI
   零开销）；环境变量 `APP_SENTRY_DSN` 覆盖（与 `APP_API_PREFIX` 同一映射规则），
   **DSN 不得进入版本库**。环境名由 SDK 兜底：`SENTRY_ENVIRONMENT` 环境变量，
   缺省 debug 构建 = development / release 构建 = production。
   初始化位于 main：config 加载后、首个 `tokio::spawn` 之前（保证 worker 线程
   派生 hub 时已携带 client）。
5. **退出路径**：`std::process::exit` 会跳过 guard 的 drop-flush，启动失败事件会
   随进程丢失；main.rs 全部失败退出统一走 `fatal_exit()`（flush ≤3s 后 exit）。

## 后果

- 正面：panic / 启动失败 / `error!` 日志三类故障获得聚合视图与告警；崩溃率
  （release-health）按 release（编译期 `SENTRY_RELEASE`，缺省为 Cargo 版本号）统计。
- 负面：新增 SDK 依赖树（sentry-core/ureq/rustls 复用现有栈，直接传递依赖约十余个）；
  DSN 网络不可达时事件按 SDK 策略重试后丢弃（不落盘），启动失败路径依赖 `fatal_exit`
  冲刷的 3s 窗口。
- 实施状态：与本 ADR 同提交落地（`startup/observability.rs` 引导模块、config 段、
  `config.example.toml` 模板、main 接线、单测）。
