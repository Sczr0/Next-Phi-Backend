# 仓库指南

## 项目结构与模块组织
- `src/` —— Rust crate 源码。
  - `main.rs` —— 启动 Axum HTTP 服务、路由与 Swagger UI。
  - `lib.rs` —— 公共模块导出（`config`、`error`、`startup`、`features`）。
  - `config.rs` —— 读取 `config.toml`，支持 `APP_*` 环境变量覆盖。
  - `startup/` —— 启动检查（创建 `resources/`、克隆/更新曲绘仓库）。
  - `features/save/` —— 存档流程：`handler`、`provider`、`client`、`decryptor`、`parser`、`record_parser`、`models`。
- `resources/` —— 本地资源，首次运行会在此克隆曲绘仓库。
- `info/` —— 静态数据文件。
- `config.toml` —— 默认配置。
- `API_USAGE.md` —— API 使用说明。

## 构建、测试与本地开发
- 构建：`cargo build --release`
- 运行：`cargo run`
- 带日志运行：`RUST_LOG=phi_backend=debug cargo run`（PowerShell：`$env:RUST_LOG='phi_backend=debug'; cargo run`）
- Swagger UI：访问 `http://localhost:3939/docs`
- 健康检查：`GET http://localhost:3939/health`

配置覆盖示例：
```
APP_SERVER_HOST=127.0.0.1 APP_SERVER_PORT=8080 cargo run
APP_API_PREFIX=/api/v1 APP_LOGGING_LEVEL=debug cargo run
```
键对应 `config.toml` 中的 `server.*`、`resources.*`、`logging.*`、`api.*`。

## 代码风格与命名约定
- 使用 Rust 2024；统一执行 `cargo fmt`，并以 `cargo clippy --all-targets --all-features -D warnings` 保持零警告。
- 文件/模块用 snake_case，类型用 CamelCase，函数用 snake_case。
- 优先使用 `Result<T, AppError>` 与 `?` 传播错误，模块单一职责（如解析放在 `parser.rs`）。

## 测试规范
- 单元测试放在同文件，使用 `#[cfg(test)]`。
- 集成测试放在 `tests/`，运行 `cargo test`。
- 测试命名清晰（如 `parses_valid_save_zip`），覆盖 `SaveProviderError` 的错误路径。

## 提交与拉取请求
- 采用 Conventional Commits：如 `feat(save): add unified save parser`、`fix(startup): handle missing repo`。
- PR 应包含：目的、范围、配置变更（环境键）、API 变更（路由/结构）、以及截图或 cURL 示例。
- 提交前请运行 `cargo fmt`、`cargo clippy`、`cargo test`。

## 安全与配置提示
- 不要提交任何密钥/令牌；优先使用环境变量而非直接修改 `config.toml`。
- 程序启动会克隆/更新曲绘仓库；可通过 `APP_RESOURCES_ILLUSTRATION_REPO`/`APP_RESOURCES_ILLUSTRATION_FOLDER` 自定义，或预置目标文件夹以跳过克隆。

## 语言约定
- 全仓库默认语言为简体中文。
- AI 对话、代码注释、提交信息与文档均使用简体中文。
