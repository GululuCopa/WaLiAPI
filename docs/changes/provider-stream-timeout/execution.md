# 执行记录

## result
IMPLEMENTED — 已构建 release 安装到 `/Applications/WaLiAPI.app`，长流式请求端到端验证通过。

## changed files

- `src-tauri/src/auth_provider/mod.rs`
  - 新增 `streaming_http_client()`：只设 10 秒 `connect_timeout`，无总超时；
    文档说明 `Client::timeout` 会切断长 SSE。
- `src-tauri/src/auth_provider/grok_backend.rs`
  - `GrokProvider` 增加 `stream_client`；`outbound` 按 `request.is_stream` 选择客户端。
  - 新增测试 `streaming_outbound_ignores_blocking_total_timeout`：慢 mock 上游
    （600ms）下，流式请求成功、非流式请求被 200ms 总超时切断 —— 直接锁定
    "流式/非流式客户端被正确选择"。
  - 新增测试专用构造器 `with_api_base_and_blocking_timeout`。
- `src-tauri/src/auth_provider/kimi_backend.rs`、`gemini_backend.rs`
  - 同样增加 `stream_client` 并在 `outbound` 中按 `is_stream` 选择（同一根因，
    同类修复；Kimi 的 `build_request` 只用于取 URL/headers，真正发送在 `outbound`）。

## verification

- `cargo test grok_backend` → 26 passed（含新增的超时行为测试）。
- `cargo test`（全量）→ **1100 passed / 0 failed**。
- **复现（旧实例 8777，修复前代码）**：长输出请求在**正好 30 秒**处终止，
  2486 个事件后收到 `event: response.failed` +
  `stream interrupted: error decoding response body (root: operation timed out)`。
- **对照（临时实例 8899，修复后 debug 构建）**：同一请求跑满 **40 秒**，
  3539 个事件，`response.failed` 出现 **0** 次，正常 `response.completed`。
- **安装后（8777，release）**：
  - `/v1/messages`（Claude Code 协议）长输出 28 秒完成 → `message_stop`、0 错误；
  - 加长到 4000 行的请求持续 **117 秒**、1.29 MB、11338 output tokens →
    `message_stop`、0 错误事件（修复前该形态必在 30 秒处断流）。
- 安装前照例校验二进制内嵌前端资源（`index-CIGcmKCe.js` 命中 1 次），
  替换 + adhoc 重签名 + 重启（PID 18671，01:24:10 启动），`/health` 正常。

## limitation

- 同一根因也存在于 Kimi / Gemini provider，本次一并修复，但只对 Grok 写了
  行为级测试（Kimi/Gemini 依赖同一共享客户端 + 全量测试回归）。
- 会话级 idle 超时（`stream.idle_timeout_secs`，缺省 120 秒）保持不变：它约束的是
  "长时间没有任何数据"，与本次被修的"整段耗时上限"是两回事。

## security
未涉及任何凭据变更；验证脚本与临时数据副本已清理。
