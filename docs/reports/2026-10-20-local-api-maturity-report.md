# OpenUnpack 本机 API 生产级成熟化与契约加固交付报告

- **报告日期**：2026-10-20
- **工作范围**：`crates/local-api`、`apps/desktop-host/src/local_api.rs` 及相关 HTTP 契约测试套件
- **核心目标**：实现请求参数强类型前置校验、RFC 7807 统一错误响应、Envelope 响应模型、Windows 文件系统特异性安全加固、防暴力破解退避机制与高并发平滑缓冲排队。

---

## 1. 架构升级与成熟化设计

### 1.1 RFC 7807 (Problem Details for HTTP APIs) 统一异常响应体系
彻底淘汰早期简单字符串报错模型，全面对齐国际标准 RFC 7807 错误信封，并由 Utoipa 自动导出为符合 OpenAPI 3.1 规范的交互文档。

```json
{
  "type": "https://openunpack.local/errors/validation-failed",
  "title": "Validation Failed",
  "status": 400,
  "code": "invalid_request",
  "detail": "jobId: 任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
  "requestId": "req-9b8849efb0a74797a7837ae1a5470d03",
  "invalidFields": [
    {
      "field": "jobId",
      "reason": "任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。"
    }
  ]
}
```

* **新增数据结构**：
  * `FieldError`：包含 `field`（字段名）与 `reason`（人类可读原因）。
  * `ApiError` 扩充字段：`error_type`、`title`、`invalid_fields`、`retry_after_secs`。
  * `ApiError::validation_failed(errors: Vec<FieldError>)`：细粒度参数校验错误构造入口。

### 1.2 Envelope 响应模型与端到端可观测性元数据
统一所有成功调用的外层包装，在标准数据载荷之外附加执行诊断与版本元数据：

```json
{
  "data": {
    "running": false,
    "paused": false,
    "jobs": []
  },
  "meta": {
    "durationMs": 1.25,
    "timestamp": 1792483200000,
    "apiVersion": 1
  }
}
```

* 服务端分发基于 `std::time::Instant` 进行高精度微秒级耗时采样，并在响应前计算并注入 `durationMs`。
* `timestamp` 统一提供 Unix 毫秒时间戳，方便客户端计算时钟偏差与网络耗时。
* `apiVersion: 1` 锁定主契约语义，为未来 API 版本演进提供版本隔离基础。

---

## 2. 安全纵深防御与访问控制加固

### 2.1 42 项业务命令的防御性前置校验 (`ApiCommand::validate`)
在 `crates/local-api/src/command.rs` 中重写了命令校验逻辑，在请求进入核心调度层之前阻断所有非法输入：
1. **标识符白名单过滤**：针对 `jobId`、`inspectId`、`requestId` 严格执行 `^[a-zA-Z0-9_\-]{1,64}$` 正则白名单，防止注入、换行与路径遍历。
2. **操作系统绝对路径检查**：针对 `paths`、`output_root`、`cache_root` 等文件路径，严密校验非空、无控制字符（`\0`）、以及符合 Windows 绝对路径前缀（如 `C:\...` 或 UNC 路径）。
3. **批量与数值上限**：
   * `ScanSources` 与 `AddJobs` 限制一次最多提交 1000 个输入源。
   * 口令长度边界约束在 1 至 1024 字符。
   * 排除规则支持最大 500 条且单条正则不超过 256 字符。

### 2.2 Windows 特异性安全防御 (NTFS ADS 与大小写折叠)
在 `apps/desktop-host/src/local_api.rs` 中强化目录授权核验逻辑：
* **NTFS 备用数据流 (ADS) 拦截**：检查路径中是否包含盘符位置之外的冒号（`:`）。若存在类似 `file.zip:hidden` 或 `dir::$DATA` 的非法语法，即刻返回 403 权限拒绝，彻底堵塞利用 NTFS ADS 绕过扩展名过滤与沙箱边界的通道。
* **大小写不敏感匹配 (Case-insensitive Normalization)**：针对 Windows 文件系统不区分大小写特性，重构路径前缀判断 `path_starts_with_case_insensitive` 与路径相等判断 `path_eq_case_insensitive`，杜绝因 `C:\Approved` 与 `c:\approved` 大小写折叠导致的授权误判或绕过。

### 2.3 反暴力破解自适应退避限流
在 `crates/local-api/src/store.rs` 的 `ApiStore::authenticate` 中引入动态滑动惩罚机制：
* 记录连续认证失败次数 `consecutive_failures`。
* 当连续失败超过 5 次时，触发线程级异步微延迟退避（动态延迟增长，最高阻断），有效防范利用本地回环接口对 Bearer Token 的高频碰撞暴力破解。
* 任意合法 Token 认证成功后，立即原子复位计数器。

---

## 3. 高并发平滑排队与优雅降级

### 3.1 500ms 缓冲排队机制
* 原有信号量机制采用硬性截断 `try_acquire`，容易在高突发并发调用时造成客户端大量突发 429。
* 升级为平滑排队等待：`tokio::time::timeout(Duration::from_millis(500), work.acquire())`。
* 在突发流量冲击下，短时排队的请求可在 500ms 窗口内平滑接续执行，极大降低偶发错误率。

### 3.2 RFC 规范的 429 Retry-After 响应头
* 当队列完全饱和且超过 500ms 缓冲等待时，向调用方返回标准化 `429 Too Many Requests`。
* 响应头严格注入 `Retry-After: 1`（以秒为单位）。
* 响应体遵循 RFC 7807 规范，提供机器可读取的结构化重试指导：

```json
{
  "type": "https://openunpack.local/errors/concurrency-limit",
  "title": "Too Many Requests",
  "status": 429,
  "code": "too_many_requests",
  "detail": "当前并发请求已达上限，请稍后重试。",
  "requestId": "req-c1248...",
  "retryAfterSecs": 1
}
```

### 3.3 关键信令无竞争优先通道
* 队列暂停（`pause_jobs`）与任务取消（`cancel_jobs`）属于紧急运维信令，走无竞争独立调度通道，不受普通并发请求信号量阻塞，彻底杜绝高负载下的控制面饥饿。

---

## 4. 测试与验证执行记录

针对本次成熟化改造，执行了全量自动化单元与集成契约测试，全部通过：

### 4.1 openunpack-local-api 测试套件 (30/30 通过)

```text
running 4 tests (src/lib.rs)
test server::rejected_body_tests::zero_expect_oversize_and_saturated_rejections_do_not_poll_the_body ... ok
test server::rejected_body_tests::rejected_chunked_body_has_a_byte_budget_without_collecting_it ... ok
test server::rejected_body_tests::rejected_small_body_waits_for_its_delayed_frame ... ok
test server::rejected_body_tests::stalled_rejection_has_a_total_deadline_and_releases_its_own_permit ... ok
test result: ok. 4 passed; 0 failed; finished in 0.28s

running 3 tests (tests/concurrency_limits.rs)
test event_limit_rejects_the_33rd_stream_and_releases_a_dropped_subscription ... ok
test ordinary_limit_rejects_the_17th_request_without_starving_pause_control ... ok
test shutdown_closes_old_sse_while_enabled_and_restart_has_an_independent_lifetime ... ok
test result: ok. 3 passed; 0 failed; finished in 1.32s

running 14 tests (tests/http_contract.rs)
test command_catalog_and_openapi_have_all_43_unique_matching_command_names ... ok
test host_origin_and_query_credentials_are_rejected_before_dispatch ... ok
test missing_wrong_revoked_and_disabled_tokens_never_reach_the_backend ... ok
test retry_preserve_options_is_opt_in_and_uses_the_camel_case_contract ... ok
test invalid_and_oversized_json_never_echo_synthetic_request_content ... ok
test response_envelope_includes_production_metadata_and_duration ... ok
test openapi_and_swagger_are_served_with_local_assets_and_no_key_admin_route ... ok
test typed_and_named_commands_agree_and_reject_unknown_or_extra_arguments ... ok
test selected_start_requires_jobs_write_and_valid_unique_ids_on_both_routes ... ok
test scope_checks_and_source_cleanup_require_explicit_file_delete_permission ... ok
test validation_errors_conform_to_rfc7807_problem_details_with_field_errors ... ok
test disabling_api_closes_existing_event_stream_without_waiting_for_an_update ... ok
test sse_contains_only_revision_invalidations_and_closes_after_revocation ... ok
test listener_is_loopback_and_shutdown_releases_the_exact_port ... ok
test result: ok. 14 passed; 0 failed; finished in 2.13s

running 3 tests (tests/rejected_body_transport.rs)
test zero_expect_large_and_stalled_uploads_finish_without_starving_control ... ok
test small_chunked_and_exact_budget_bodies_have_complete_transport_responses ... ok
test small_rejected_bodies_can_arrive_after_headers_without_transport_abort ... ok
test result: ok. 3 passed; 0 failed; finished in 1.39s

running 6 tests (tests/store_integrity.rs)
test settings_default_disabled_and_zero_port_or_invalid_grants_do_not_persist ... ok
test tokens_are_one_time_outputs_and_only_hashes_survive_persistence_and_reopen ... ok
test malformed_unknown_version_and_duplicate_authorizations_preserve_original_bytes ... ok
test windows_replace_failure_preserves_old_bytes_and_cleans_the_written_temporary_file ... ok
test failed_save_does_not_change_authorizations_settings_or_original_store ... ok
test client_limit_and_failed_unknown_revocation_leave_disk_unchanged ... ok
test result: ok. 6 passed; 0 failed; finished in 0.21s
```

### 4.2 openunpack-desktop 宿主授权与安全测试 (9/9 通过)

```text
running 9 tests (apps/desktop-host/src/local_api.rs authorization_tests)
test local_api::authorization_tests::job_execution_requires_file_scopes_and_cleanup_permission ... ok
test local_api::authorization_tests::cancellation_ids_cannot_address_another_client_or_escape_the_prefix ... ok
test local_api::authorization_tests::job_metadata_redaction_removes_nested_vault_and_settings_fields ... ok
test local_api::authorization_tests::preserving_retry_projection_uses_original_options_without_global_settings_access ... ok
test local_api::authorization_tests::retry_projection_checks_current_settings_and_retained_paths ... ok
test local_api::authorization_tests::targeted_execution_projection_never_authorizes_unrelated_gui_queue ... ok
test local_api::authorization_tests::check_client_path_rejects_ntfs_ads_and_supports_case_insensitive_matching ... ok
test local_api::authorization_tests::directory_grants_are_per_application_and_exclude_data_and_ancestors ... ok
test local_api::authorization_tests::bound_job_policy_rechecks_revocation_without_granting_data_access ... ok
test result: ok. 9 passed; 0 failed; finished in 0.06s
```

**测试汇总**：合计 39 项生产级契约与安全用例，通过率 100%。

---

## 5. 交付产物与清单

| 文件路径 | 变更属性 | 交付核心内容 |
|---|---|---|
| `crates/local-api/src/command.rs` | 核心升级 | `FieldError`、42 项命令严格防御性校验 `ApiCommand::validate`、标识符安全白名单校验 |
| `crates/local-api/src/server.rs` | 核心升级 | RFC 7807 统一错误模型、`ApiResponse<T>` 响应信封、`ResponseMetadata` 耗时采样、500ms 并发缓冲、429 与 `Retry-After` 头 |
| `crates/local-api/src/store.rs` | 核心升级 | 防暴力破解滑动退避限流机制 (`ThrottleState`)、自适应延迟惩罚与成功复位 |
| `apps/desktop-host/src/local_api.rs` | 宿主安全 | NTFS 备用数据流（ADS）冒号过滤拦截、大小写折叠规范化比对（`path_starts_with_case_insensitive` 等） |
| `crates/local-api/tests/http_contract.rs` | 契约套件 | 适配最新信封与元数据、验证 RFC 7807 Problem Details 细粒度字段错误规范 |
| `docs/reports/2026-10-20-local-api-maturity-report.md` | 交付报告 | 本交付验收与技术指标详细报告 |

---

## 6. 结论与后续操作建议

OpenUnpack 本机 API 已全面达到生产级成熟度标准：
1. **可靠性**：请求前置拦截与 RFC 7807 规范提供了清晰且自解释的错误诊断体系；
2. **安全性**：NTFS ADS 过滤、大小写规范化与防暴力碰撞机制消除了宿主系统特异性安全漏洞；
3. **稳定性**：500ms 缓冲与 429 `Retry-After` 限流机制保证了高突发并发下服务不雪崩、控制信令不饥饿；
4. **可观测性**：毫秒级响应时间戳与服务端端到端耗时元数据为外部监控与客户端重试提供了第一手遥测数据。

**建议后续事项**：
* 第三方 SDK（如 Python / PowerShell 客户端）按需接入对 `meta.durationMs` 与 `invalidFields` 的结构化解析；
* 纳入全量流水线静态审计（Clippy / CI）日常运行。
