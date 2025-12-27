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
