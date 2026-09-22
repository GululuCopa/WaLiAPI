# Provider 流式出站超时修复（Claude Code 中途断流）

状态：IMPLEMENTED（已构建安装，端到端复现与对照验证通过）

## objective

修复 Claude Code（以及任何长回答场景）通过 WaLiAPI 调用账号型上游时，流在
固定 30 秒处被切断、下游报 `Server error mid-response` 的问题。

## 现象与证据

- Claude Code：`API Error: Server error mid-response. The response above may be incomplete.`
- WaLiAPI 日志（`17:08:42Z`，Claude Code 触发）：
  `502 /v1/messages` → `grok-4.7`，`stream_committed=1`，
  `stream interrupted: error decoding response body (root: operation timed out)`
- 复现：对一个长输出请求，旧实例在**正好 30 秒**处停止，在 2486 个事件后收到
  `event: response.failed` / `operation timed out`（此前一切正常）。

## 根因

`grok_backend` / `kimi_backend` / `gemini_backend` 的 provider HTTP client 都用
`.timeout(<provider>_HTTP_TIMEOUT)`（均为 30 秒）构造，并**同一个 client 同时服务
流式与非流式请求**。reqwest 的 `Client::timeout` 覆盖"发送 + 读完响应体"的整段耗时，
因此任何持续超过 30 秒的 SSE 流都会在固定点被切断，与上游是否正常输出无关。
`codex_backend` 未设该超时，渠道路径则已区分 `streaming_client`（只设 connect
timeout）与 `blocking_client`（总超时）。

## approved design decisions

1. 在 `auth_provider` 增加共享的 `streaming_http_client()`：只设 10 秒
   `connect_timeout`（与渠道路径的 `CONNECT_TIMEOUT_SECS` 一致），**不设总超时**。
2. 三个 provider 各持有 `client`（非流式，保留原 30 秒总超时）与 `stream_client`
   （流式），`outbound` 按 RoutePlan 冻结的 `request.is_stream` 选择，禁止从请求体
   或 Content-Type 猜测。
3. 登录 / 模型列表等短请求继续走原 client（总超时保护不变）。

## expected affected files

- `src-tauri/src/auth_provider/mod.rs`（新增共享客户端）
- `src-tauri/src/auth_provider/{grok,kimi,gemini}_backend.rs`
- `docs/changes/provider-stream-timeout/{plan.md,execution.md}`

## verification commands

```bash
cd src-tauri && cargo test grok_backend
cd src-tauri && cargo test
./node_modules/.bin/tauri build --no-bundle
```

## acceptance criteria

- 长流式回答（>30 秒）不再出现 `operation timed out` / `response.failed`。
- 非流式请求仍受各自的总超时约束。
- 现有 provider 测试与全量测试无回归。
