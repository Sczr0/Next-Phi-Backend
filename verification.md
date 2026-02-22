# 验证记录（2025-12-25，Codex）

## 任务

为对外统计接口添加更多参数（公开透明 + 满足好奇心）：扩展 `GET /stats/daily` 与 `GET /stats/summary` 的 Query 参数，并补充可选维度汇总与测试。

## 环境

- OS：Windows（PowerShell 5.1）
- Rust：以 `cargo` 实际执行结果为准

## 执行命令

- `cargo fmt`
- `cargo test`

## 结果摘要

- `cargo fmt`：通过
- `cargo test`：通过（lib unit 46、main unit 3、integration 1；性能用例 1 ignored）

## 原始输出（完整）

```text
   Compiling phi-backend v0.1.0 (D:\git\2 - Phi-Backend\phi-backend)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2m 19s
     Running unittests src\lib.rs (target\debug\deps\phi_backend-790676d3f8e44bb2.exe)

running 46 tests
test features::image::handler::tests::supports_svg_format_code_and_content_type ... ok
test features::image::renderer::tests::generate_song_svg_uses_remote_illust_when_missing_path ... ok
test features::image::renderer::tests::generate_svg_uses_remote_cover_url_when_base_provided ... ok
test features::image::renderer::tests::maps_local_path_to_public_url ... ok
test features::image::renderer::tests::maps_local_path_to_somnia_url_for_external_base ... ok
test features::image::renderer::tests::generate_bn_svg_renders_with_neo_template ... ok
test features::image::renderer::tests::generate_song_svg_renders_with_external_template ... ok
test features::image::renderer::tests::generate_bn_svg_renders_with_external_template ... ok
test features::leaderboard::handler::tests::test_alias_validation_with_chinese ... ok
test features::leaderboard::handler::tests::test_is_cjk_char ... ok
test features::leaderboard::handler::tests::test_mask_user_prefix ... ok
test features::leaderboard::handler::tests::test_require_admin_env ... ok
test features::rks::engine::tests::simulate_rks_increase_simplified_matches_reference ... ok
test features::rks::handler::tests::test_rks_history_item_serialize ... ok
test features::save::inspector::tests::redact_b64_long_is_truncated ... ok
test features::save::inspector::tests::redact_b64_short_keeps_original ... ok
test features::save::inspector::tests::redact_url_removes_query_fragment_and_keeps_host ... ok
test features::save::provider::tests::try_decompress_gzip_failure_falls_back_to_raw ... ok
test features::save::provider::tests::try_decompress_gzip_success_returns_decompressed ... ok
test features::save::provider::tests::try_decompress_unknown_header_falls_back_to_raw ... ok
test features::save::provider::tests::try_decompress_zip_magic_returns_raw ... ok
test features::save::provider::tests::try_decompress_zlib_success_returns_decompressed ... ok
test features::rks::engine::tests::calculate_target_chart_push_acc_matches_reference ... ok
test features::stats::handler::tests::include_flags_parse_all_and_partials ... ok
test features::stats::handler::tests::normalize_top_limits_and_rejects_invalid ... ok
test features::stats::handler::tests::parse_date_bound_utc_uses_timezone ... ok
test features::image::renderer::tests::returns_none_when_not_under_base_dir ... ok
test features::stats::middleware::tests::client_ip_falls_back_to_x_real_ip ... ok
test features::stats::middleware::tests::client_ip_prefers_x_forwarded_for_first_item ... ok
test features::stats::middleware::tests::client_ip_returns_none_for_missing_or_empty ... ok
test features::stats::middleware::tests::client_ip_returns_none_for_non_utf8_header ... ok
test features::stats::tests::prefers_session_token_over_external_credentials ... ok
test features::stats::tests::returns_none_when_salt_missing ... ok
test features::stats::tests::uses_external_api_user_id_when_present ... ok
test features::stats::tests::uses_external_sessiontoken_when_present ... ok
test features::stats::tests::uses_platform_pair_when_present ... ok
test shutdown::tests::test_multiple_triggers ... ok
test shutdown::tests::test_shutdown_handle ... ok
test shutdown::tests::test_shutdown_manager_basic ... ok
test shutdown::tests::test_timeout_functionality ... ok
test watchdog::tests::test_systemd_notifications ... ok
test watchdog::tests::test_watchdog_config_validation ... ok
test watchdog::tests::test_watchdog_creation ... ok
test features::stats::handler::tests::daily_stats_supports_route_and_method_filters ... ok
test features::stats::handler::tests::stats_summary_supports_include_and_filters ... ok
test features::image::renderer::tests::webp_encoding_respects_quality_and_lossless ... ok

test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.74s

     Running unittests src\main.rs (target\debug\deps\phi_backend-2a867b2b5dcf177a.exe)

running 3 tests
test compression_predicate_tests::compression_predicate_disables_sse ... ok
test compression_predicate_tests::compression_predicate_disables_images_but_allows_svg ... ok
test compression_predicate_tests::compression_predicate_disables_common_binary_downloads ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src\bin\save_inspect.rs (target\debug\deps\save_inspect-9876fe6228add2bf.exe)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\b27_performance_test.rs (target\debug\deps\b27_performance_test-5b4d0e1e0d08fd56.exe)

running 1 test
test test_b27_generation_with_flamegraph ... ignored

test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\leaderboard_storage.rs (target\debug\deps\leaderboard_storage-1a500be883c37f04.exe)

running 1 test
test upsert_improves_only ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

   Doc-tests phi_backend

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

---

# 验证记录（2025-12-28，Codex）

## 任务

在 `feat` 分支落地 API v2 契约：

- 默认 API 前缀切换为 `/api/v2`
- JSON 字段命名统一为 `camelCase`
- 错误响应统一为 RFC7807 `ProblemDetails`（`application/problem+json`）
- 重新导出 OpenAPI，并重新生成 TypeScript SDK

## 执行命令

- `cargo run --example dump_openapi -q`
- `python -c ...`（统计 OpenAPI 统一度，结果写入 `.codex/context-question-13-openapi-v2-uniformity.json`）
- `cd sdk/ts; pnpm i; pnpm run generate; pnpm run build`
- `cargo test -q`

## 结果摘要

- OpenAPI 统一度（v2）：错误响应 content-type 全部收敛为 `application/problem+json`；schema 字段命名下划线占比为 0（详见 `.codex/context-question-13-openapi-v2-uniformity.json`）。
- TypeScript SDK：已基于最新 OpenAPI 重新生成，默认 `OpenAPI.BASE` 为 `/api/v2`，并可将 `application/problem+json` 解析为 JSON（`ApiError.body` 为 `ProblemDetails`）。
- 测试：`cargo test -q` 全部通过（新增 `tests/api_contract_v2.rs` 校验关键契约）。

---

# 验证记录（2026-01-10，Codex）

## 任务

解释并修复 `/rks/history` 在“未游玩”情况下出现的 1e-15 量级极小跳变：

- RKS 计算对 HashMap 遍历顺序不敏感（确定性）
- rksJump 对浮点噪声归零（写入与查询侧）

## 环境

- OS：Windows（PowerShell）
- 仓库：`D:\git\2 - Phi-Backend\phi-backend`

## 执行命令

- `cargo test -q`

## 结果摘要

- `cargo test -q`：通过。
- 证据日志：`.codex/logs/cargo-test-2026-01-10-rks-history-tiny-jump.log`。


# 验证记录（2025-12-28，Codex）

## 任务

按现状落地一版 Auth 改造：

- `/auth/qrcode` 从 **GET -> POST**，并将版本参数统一为 `taptapVersion`
- 扫码相关响应统一添加 `Cache-Control: no-store`
- TapTapClient 日志与错误信息脱敏（禁止上游响应体进入对外 `detail`）
- TapTap 默认版本选择遵循 `config.taptap.default_version`
- 二维码过期语义使用上游 `expires_in` 驱动（过期后返回 Expired 并清理缓存）

## 执行命令

- `cargo test -q`

## 结果摘要

- 编译与测试：`cargo test -q` 通过（新增 `tests/auth_contract_v2.rs` 与 `src/features/auth/client.rs` 单元测试）。
- 接口契约：扫码相关响应增加 `Cache-Control: no-store`；`/auth/qrcode` 的 OpenAPI 已切换为 POST 且 query 参数名为 `taptapVersion`。

## 风险与兼容性说明

- **破坏性变更**：调用方若仍以 GET 调用 `/auth/qrcode` 或继续使用旧参数名，将无法按预期工作；需同步更新客户端/SDK 调用方式。

---

# 验证记录（2026-01-02，Codex）
## 任务

检查代码实际实现与自动生成 OpenAPI 描述的一致性：补充 `POST /auth/qrcode` 可能返回的 `422 Unprocessable Entity`（`application/problem+json`）响应描述，并重新导出 `sdk/openapi.json`。

## 环境

- OS：Windows（PowerShell）
- 仓库：`D:\git\2 - Phi-Backend\phi-backend`

## 执行命令

- `cmd /c "cargo test 2>&1" | Tee-Object -FilePath .codex/logs/cargo-test-cmdredir-20260102-163852.log`
- `cmd /c "cargo run --example dump_openapi 2>&1" | Tee-Object -FilePath .codex/logs/dump-openapi-20260102-163924.log`
- `python -c "import json; spec=json.load(open('sdk/openapi.json','r',encoding='utf-8')); print(sorted(spec['paths']['/auth/qrcode']['post']['responses'].keys()))"`

## 结果摘要

- `cargo test`：通过
- `dump_openapi`：成功写入 `sdk/openapi.json`
- OpenAPI 核对：`/auth/qrcode` responses 为 `['200', '422', '500', '502']`

## 原始输出（完整）

### cargo test
```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.94s
     Running unittests src\\lib.rs (target\\debug\\deps\\phi_backend-df9bbe28ca502b67.exe)

running 52 tests
test features::image::handler::tests::supports_svg_format_code_and_content_type ... ok
test features::auth::client::tests::get_config_uses_default_version_when_none ... ok
test features::auth::client::tests::get_config_prefers_explicit_version_over_default ... ok
test features::image::renderer::tests::generate_song_svg_uses_remote_illust_when_missing_path ... ok
test features::image::renderer::tests::generate_svg_uses_remote_cover_url_when_base_provided ... ok
test features::image::renderer::tests::generate_song_svg_renders_with_external_template ... ok
test features::image::renderer::tests::maps_local_path_to_public_url ... ok
test features::image::renderer::tests::generate_bn_svg_renders_with_neo_template ... ok
test features::image::renderer::tests::maps_local_path_to_somnia_url_for_external_base ... ok
test features::image::renderer::tests::generate_bn_svg_renders_with_external_template ... ok
test features::leaderboard::handler::tests::test_alias_validation_with_chinese ... ok
test features::image::renderer::tests::returns_none_when_not_under_base_dir ... ok
test features::leaderboard::handler::tests::test_is_cjk_char ... ok
test features::leaderboard::handler::tests::test_mask_user_prefix ... ok
test features::leaderboard::handler::tests::test_require_admin_env ... ok
test features::rks::engine::tests::simulate_rks_increase_simplified_matches_reference ... ok
test features::rks::handler::tests::test_rks_history_item_serialize ... ok
test features::save::inspector::tests::redact_b64_long_is_truncated ... ok
test features::save::inspector::tests::redact_b64_short_keeps_original ... ok
test features::save::inspector::tests::redact_url_removes_query_fragment_and_keeps_host ... ok
test features::save::provider::tests::try_decompress_gzip_failure_falls_back_to_raw ... ok
test features::save::provider::tests::try_decompress_gzip_success_returns_decompressed ... ok
test features::save::provider::tests::try_decompress_unknown_header_falls_back_to_raw ... ok
test features::save::provider::tests::try_decompress_zip_magic_returns_raw ... ok
test features::save::provider::tests::try_decompress_zlib_success_returns_decompressed ... ok
test features::rks::engine::tests::calculate_target_chart_push_acc_matches_reference ... ok
test features::image::renderer::tests::webp_encoding_respects_quality_and_lossless ... ok
test features::stats::handler::tests::daily_features_outputs_counts_and_unique_users ... ok
test features::stats::handler::tests::daily_http_computes_error_rates_and_respects_top_per_day ... ok
test features::stats::handler::tests::include_flags_parse_all_and_partials ... ok
test features::stats::handler::tests::normalize_top_limits_and_rejects_invalid ... ok
test features::stats::handler::tests::daily_dau_fills_missing_days_with_zero ... ok
test features::stats::handler::tests::parse_date_bound_utc_uses_timezone ... ok
test features::stats::middleware::tests::client_ip_falls_back_to_x_real_ip ... ok
test features::stats::middleware::tests::client_ip_prefers_x_forwarded_for_first_item ... ok
test features::stats::middleware::tests::client_ip_returns_none_for_missing_or_empty ... ok
test features::stats::middleware::tests::client_ip_returns_none_for_non_utf8_header ... ok
test features::stats::tests::prefers_session_token_over_external_credentials ... ok
test features::stats::tests::returns_none_when_salt_missing ... ok
test features::stats::tests::uses_external_api_user_id_when_present ... ok
test features::stats::tests::uses_external_sessiontoken_when_present ... ok
test features::stats::tests::uses_platform_pair_when_present ... ok
test shutdown::tests::test_multiple_triggers ... ok
test shutdown::tests::test_shutdown_handle ... ok
test shutdown::tests::test_shutdown_manager_basic ... ok
test features::stats::handler::tests::daily_stats_respects_timezone_day_boundary ... ok
test watchdog::tests::test_systemd_notifications ... ok
test watchdog::tests::test_watchdog_config_validation ... ok
test watchdog::tests::test_watchdog_creation ... ok
test shutdown::tests::test_timeout_functionality ... ok
test features::stats::handler::tests::daily_stats_supports_route_and_method_filters ... ok
test features::stats::handler::tests::stats_summary_supports_include_and_filters ... ok

test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.86s

     Running unittests src\\main.rs (target\\debug\\deps\\phi_backend-cf0a52e46844fa11.exe)

running 3 tests
test compression_predicate_tests::compression_predicate_disables_images_but_allows_svg ... ok
test compression_predicate_tests::compression_predicate_disables_common_binary_downloads ... ok
test compression_predicate_tests::compression_predicate_disables_sse ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src\\bin\\save_inspect.rs (target\\debug\\deps\\save_inspect-679c7b166847adb7.exe)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\\api_contract_v2.rs (target\\debug\\deps\\api_contract_v2-3a6164f75f4a2dca.exe)

running 2 tests
test render_bn_request_serializes_as_camel_case ... ok
test app_error_into_response_is_problem_details ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\\auth_contract_v2.rs (target\\debug\\deps\\auth_contract_v2-4a02ae2a89170c8f.exe)

running 2 tests
test qrcode_status_expires_in_zero_becomes_expired ... ok
test qrcode_status_missing_is_expired_and_no_store ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\\b27_performance_test.rs (target\\debug\\deps\\b27_performance_test-0dfeaf87f55f1d08.exe)

running 1 test
test test_b27_generation_with_flamegraph ... ignored

test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\\leaderboard_storage.rs (target\\debug\\deps\\leaderboard_storage-5c3ecf4a811e4b63.exe)

running 1 test
test upsert_improves_only ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s

     Running tests\\song_search_controls.rs (target\\debug\\deps\\song_search_controls-d80434c9b1fb124d.exe)

running 7 tests
test song_catalog_search_is_stably_ordered_for_same_name ... ok
test songs_search_last_page_has_no_next_offset ... ok
test songs_search_default_limit_is_applied ... ok
test songs_search_limit_is_clamped_to_max ... ok
test songs_search_limit_zero_is_validation_error ... ok
test songs_search_query_too_long_is_validation_error ... ok
test songs_search_offset_paginates_and_reports_next_offset ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

   Doc-tests phi_backend

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### dump_openapi
```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.90s
     Running `target\\debug\\examples\\dump_openapi.exe`
wrote sdk/openapi.json
```

---

# 验证记录（2026-01-02，Codex）
## 任务

OpenAPI 一致性二次修复：
- `/save`：补充 422（存档解密/校验/解析失败等）
- `/auth/qrcode`：补充 401（TapTap 认证错误）
- `/image/bn`、`/image/song`：移除 401/502（当前实现不会返回这些状态码）

## 执行命令

- `cmd /c "cargo run --example dump_openapi 2>&1" | Tee-Object -FilePath .codex/logs/dump-openapi-20260102-231916.log`
- `cmd /c "cargo fmt 2>&1" | Out-File -FilePath .codex/logs/cargo-fmt-20260102-232235.log -Encoding utf8`
- `cmd /c "cargo test 2>&1" | Tee-Object -FilePath .codex/logs/cargo-test-20260102-232252.log`
- `python -c "import json; spec=json.load(open('sdk/openapi.json','r',encoding='utf-8')); print('/save', sorted(spec['paths']['/save']['post']['responses'].keys())); print('/auth/qrcode', sorted(spec['paths']['/auth/qrcode']['post']['responses'].keys())); print('/image/bn', sorted(spec['paths']['/image/bn']['post']['responses'].keys())); print('/image/song', sorted(spec['paths']['/image/song']['post']['responses'].keys()));"`

## 结果摘要

- OpenAPI 已重导出（`sdk/openapi.json`）
- `cargo test`：通过（完整输出见 `.codex/logs/cargo-test-20260102-232252.log`）
- responses keys 校验结果：

```text
/save ['200', '400', '401', '422', '500', '502', '504']
/auth/qrcode ['200', '401', '422', '500', '502']
/image/bn ['200', '400', '422', '500']
/image/song ['200', '400', '404', '409', '422', '500']
```

---

# 验证记录：2026-01-03，Codex
## 任务

新增统计接口：支持按天/周/月聚合各端点请求返回耗时（平均/最大/最小）。

## 变更点

- 新增 `GET /stats/latency`（Query：start/end/timezone/bucket/day|week|month + 可选 route/method/feature 过滤；Response：timezone/start/end/bucket/filters/rows）。
- OpenAPI 已更新并重新导出到 `sdk/openapi.json`，TS SDK 已重新生成/构建。

## 执行命令

- `cargo test -q 2>&1 | Tee-Object -FilePath .\\.codex\\cargo-test-2026-01-03.log`
- `cargo run --example dump_openapi -q`
- `cd sdk/ts && pnpm i && pnpm run generate && pnpm run build`

## 结果摘要

- `cargo test`：通过（完整输出见 `.codex/cargo-test-2026-01-03.log`）
- `dump_openapi`：成功写入 `sdk/openapi.json`
- `sdk/ts`：generate/build 成功

---

# 验证记录：2026-01-04（扩大 /leaderboard/rks/top 每页数量），Codex
## 任务

在尽量不影响性能的情况下，使 `GET /leaderboard/rks/top` 支持更多的每页数量。

## 变更点

- 普通模式：`limit` 仍最大 200（避免放大 BestTop3/APTop3 查询与 JSON 反序列化成本）。
- `lite=true`：`limit` 最大提升到 1000（仅返回轻量字段）。

## 执行命令

- `cargo test -q 2>&1 | Tee-Object -FilePath .\\.codex\\logs\\cargo-test-2026-01-04-leaderboard-limit.log`
- `cargo run --example dump_openapi -q`
- `cd sdk/ts && pnpm run generate && pnpm run build`

## 结果摘要

- `cargo test`：通过（完整输出见 `.codex/logs/cargo-test-2026-01-04-leaderboard-limit.log`）
- `dump_openapi`：成功写入 `sdk/openapi.json`
- `sdk/ts`：generate/build 成功

## 使用示例

```bash
curl \"http://localhost:3939/api/v2/stats/latency?start=2025-12-24&end=2026-01-05&timezone=Asia/Shanghai&bucket=week\"
```

---

# 验证记录：2026-01-04，Codex
## 任务

修复排行榜接口：
1) `/leaderboard/rks/top` 的 `nextAfterUser` 不应暴露原始用户标识；
2) `/leaderboard/rks/top`（以及同类型列表）提供轻量模式，不返回每个用户的 BestTop3/APTop3。

## 变更点

- `GET /leaderboard/rks/top`：`nextAfterUser` 与 `items[].user` 一致脱敏；新增 `lite=true` 时不返回 `bestTop3/apTop3`。
- `GET /leaderboard/rks/by-rank`：同样支持 `lite`，并对 `nextAfterUser` 做一致脱敏。
- TS SDK：重新生成后 `LeaderboardService.getTop()` 与 `LeaderboardService.getByRank()` 支持 `lite` 参数。

## 执行命令

- `cargo test -q 2>&1 | Tee-Object -FilePath .\\.codex\\logs\\cargo-test-2026-01-04.log`
- `cargo run --example dump_openapi -q`
- `cd sdk/ts && pnpm run generate && pnpm run build`

## 结果摘要

- `cargo test`：通过（完整输出见 `.codex/logs/cargo-test-2026-01-04.log`）
- `dump_openapi`：成功写入 `sdk/openapi.json`
- `sdk/ts`：generate/build 成功

---

# 验证记录：2026-01-04（OpenAPI /save 字段命名一致性），Codex
## 任务

检查 OpenAPI 与实现的一致性：发现 `/save` 响应的 `ParsedSaveDoc` 在 OpenAPI 中按 camelCase（gameRecord 等）描述，但实际返回体来自 `ParsedSave` 的序列化（game_record 等 snake_case，updatedAt/summaryParsed 为显式 rename）。按“以实现为准”的原则，修正 OpenAPI 表述并重新导出。

## 变更点

- `src/features/save/models.rs`：调整 OpenAPI 专用 `ParsedSaveDoc` 字段命名以匹配实际返回：
  - 保持 `game_record/game_progress/game_key/...` 为 snake_case；
  - `updated_at` 与 `summary_parsed` 显式 `serde(rename)` 为 `updatedAt` / `summaryParsed`（与实际返回一致）。
- OpenAPI：重新导出 `sdk/openapi.json`。
- TS SDK：重新生成/构建，确保模型字段与 OpenAPI 一致（`sdk/ts/src/models/ParsedSaveDoc.ts`）。

## 执行命令

- `cargo run --example dump_openapi -q`
- `cd sdk/ts; pnpm run generate`
- `cd sdk/ts; pnpm run build`
- `cargo test -q 2>&1 | Tee-Object -FilePath .codex/logs/cargo-test-2026-01-04-openapi-save-schema.log`
- `python -c "import json; spec=json.load(open('sdk/openapi.json','r',encoding='utf-8')); print(sorted(spec['components']['schemas']['ParsedSaveDoc']['properties'].keys())); print('required=', spec['components']['schemas']['ParsedSaveDoc'].get('required'))"`

## 结果摘要

- `cargo test -q`：通过（完整输出见 `.codex/logs/cargo-test-2026-01-04-openapi-save-schema.log`）
- OpenAPI schema 校验：

```text
['game_key', 'game_progress', 'game_record', 'settings', 'summaryParsed', 'updatedAt', 'user']
required= ['game_record', 'game_progress', 'user', 'settings', 'game_key']
```

---

# 验证记录：2026-01-04（性能无行为变更优化），Codex
## 任务

从全局性能角度审计并优先落地“不影响现有行为”的优化：
1) template 模式布局 JSON 覆盖缓存（mtime/len 失效，保留热更新语义）
2) `/image/bn` TopN 构造去除 RenderRecord clone

## 变更点

- `src/features/image/renderer/svg_templates.rs`
  - 为 `{bn|song}/{template_id}.json` 覆盖读取引入缓存，避免每次渲染读盘与反序列化
  - 增加单元测试覆盖：文件变更/删除/无效 JSON 的行为
- `src/features/image/handler.rs`
  - `/image/bn` 计算块 TopN 由 clone 改为 drain move（统计计算完成后再 move TopN，保持统计仍基于全量 all）
  - 增加等价性单元测试：drain 方案与 clone+take 方案结果一致

## 执行命令

- `cargo fmt`
- `cargo test -q 2>&1 | Tee-Object -FilePath .\\.codex\\logs\\cargo-test-2026-01-04-performance.log`

## 结果摘要

- `cargo test`：通过（完整输出见 `.codex/logs/cargo-test-2026-01-04-performance.log`）

## 未执行项与风险

- 未执行 `tests/b27_performance_test.rs`（需要 `PHI_SESSION_TOKEN` + 网络，且属于性能基准/火焰图而非功能回归）。风险：无法在本机无 token 环境量化“模板模式读盘减少/TopN clone 去除”的实际收益，但功能等价性已由单元测试与全量回归覆盖。

---

# 验证记录：2026-01-10（Song Search：分页下推 + unique 候选预览），Codex

## 任务

修复 `docs/performance/song-search.md` 中列出的两项 P1：
- `GET /songs/search`：分页下推到 `SongCatalog`，避免“全量构建结果再切片”
- `unique=true`：歧义查询返回受控数量的候选 `{id,name}` 预览，并提供 `candidatesTotal`，避免无收益 clone

## 执行命令

- `cargo fmt 2>&1 | Tee-Object -FilePath .codex/logs/cargo-fmt-20260110-song-search-improvements.log`
- `cargo test -q 2>&1 | Tee-Object -FilePath .codex/logs/cargo-test-20260110-song-search-improvements.log`

## 结果摘要

- `cargo fmt`：通过
- `cargo test -q`：全量通过
- 合约验证：`tests/song_search_controls.rs` 新增用例覆盖 `unique=true` 多命中时 409 响应体包含 `candidates/candidatesTotal` 且候选数量受控

## 原始输出（完整）

见 `.codex/logs/cargo-fmt-20260110-song-search-improvements.log`、`.codex/logs/cargo-test-20260110-song-search-improvements.log`。

---

# 验证记录（2026-01-10，Codex）

## 任务

- RKS：仅保留简化口径（Best27 + APTop3 允许重叠）并将计算改为 TopK 流式选择/惰性构造
- Image：为 `/image/bn/user` 增加 scores 条数硬上限（`image.max_user_scores`）

## 环境

- OS：Windows（PowerShell 5.1）
- Rust：以 `cargo` 实际执行结果为准

## 执行命令

- `cargo test -q`

## 结果摘要

- `cargo test -q`：通过（全量测试通过；性能用例 `tests/b27_performance_test.rs` 仍为 ignored）

## 原始输出（完整）

```text
running 66 tests
..................................................................
test result: ok. 66 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.01s


running 3 tests
...
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 2 tests
..
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 2 tests
..
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 1 test
i
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 1 test
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.94s


running 3 tests
...
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.10s


running 9 tests
.........
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

（同内容已写入：`.codex/cargo-test-2026-01-10-rks-topk.log`）

---

# 验证记录（2026-01-10，Codex）

## 任务

- Auth：修复 `docs/performance/auth.md` 的 P0（TapTapClient 缺少 timeout/connect_timeout；并补齐 504 超时语义）

## 环境

- OS：Windows（PowerShell）
- 仓库：`D:\\git\\2 - Phi-Backend\\phi-backend`

## 执行命令

- `cargo test -q 2>&1 | Tee-Object -FilePath .codex/logs/cargo-test-20260110-auth-p0-timeout.log`

## 结果摘要

- `cargo test -q`：通过（全量测试通过；性能用例 `tests/b27_performance_test.rs` 仍为 ignored）

## 原始输出（完整）

- 见 `.codex/logs/cargo-test-20260110-auth-p0-timeout.log`

---

# 验证记录：2026-01-12（文档：存档链路说明），Codex

## 任务

- 新增 `docs/save/*`：用“胎教式”说明本项目内 Phigros 存档获取/解密/解析与保留/舍弃规则

## 环境

- OS：Windows（PowerShell）
- 仓库：`D:\\git\\2 - Phi-Backend\\phi-backend`

## 执行命令

- `cargo test -q --lib 2>&1 | Tee-Object -FilePath .codex/logs/cargo-test-20260112-save-docs-lib.log`

## 结果摘要

- `cargo test -q --lib`：通过（70 passed）

## 原始输出（完整）

- 见 `.codex/logs/cargo-test-20260112-save-docs-lib.log`

---

# 验证记录：2026-02-22（模块归档：api/contracts 聚合），Codex

## 任务

- 将新增 *_api/*_contract 模块在 src 下按目录聚合，不改行为。

## 变更

- 新目录：src/api、src/contracts。
- *_api 文件归档到 src/api。
- *_contract 文件归档到 src/contracts。
- src/lib.rs 通过 #[path] 保持原模块名导出（调用路径不变）。

## 结果摘要

- 模块物理结构已聚合，crate::auth_contract 等现有调用路径无需改动。
- 两个 Stage2 门禁脚本均 PASS。
- 按项目约束未执行本地 Cargo 编译/测试，请以 GitHub Actions 为准。