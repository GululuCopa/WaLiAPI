# Grok 上游客户端工具兼容（Codex / Claude / OpenCode / OpenClaw / Hermes）

状态：IMPLEMENTED（待重启验证运行实例）

## objective

让 Codex CLI、Claude Code、OpenCode、OpenClaw、Hermes 五类下游客户端通过
WaLiAPI 调用 Grok OAuth 账号（cli-chat-proxy）时不再因工具声明或采样字段被
上游拒绝而整段失败，问题形态与既有的 `antigravity-namespace-tool-compat` 同类。

## non-goals

- 不修改 Grok OAuth 登录、token 刷新、模型同步与路由选路。
- 不改动 Codex / Kimi / Gemini / Antigravity 的 provider 出站行为。
- 不把上游不支持的工具体转换成其他类型（不做 namespace → function 展平、不做
  `custom` → `function` 猜测转换）。
- 不修改客户端配置文件内容，不写入任何 token / secret。

## current behavior and evidence

真实环境（本机数据目录 + 真实 Grok OAuth 账号）实测：

1. Codex CLI 0.155.1 用 `grok-4.7` 发 `/v1/responses`（identity 路径）报
   `400 Argument not supported: external_web_access`；紧跟一条
   `400 Invalid request content: A tool_choice was set on the request but no tools were specified.`
   抓包确认 Codex 该请求带 15 个工具：12 个 `function`、2 个 `namespace`
   （`multi_agent_v1`、`mcp__computer_use`，内部嵌套 function 子工具）、
   1 个 `{"type":"web_search","external_web_access":false}`。
2. Grok 上游 `tools[].type` 白名单（其 422 错误直接回报）：
   `function, web_search, x_search, image_generation, collections_search,
   file_search, code_execution, code_interpreter, mcp, shell, tool_search`。
   白名单之外的 `namespace` / `custom` / `local_shell` / `web_search_preview`
   会让整段请求 422 `unknown variant`。
3. `web_search` 工具内 `external_web_access`、`search_context_size` 被上游
   `400 Argument not supported`；`filters` / `user_location` /
   `indexed_web_access` / `search_content_types` 上游接受，应保留。
4. `tools` 缺失或为空时携带 `tool_choice`（任何形态）→ 400；`function`
   tool_choice 指向已不存在的工具同样会被拒。
5. OpenCode 形态的 `/v1/chat/completions`（带 `temperature`）报
   `400 Chat field "temperature" has no Responses backend representation`，
   即 `Chat→Responses` 的字段白名单过窄，客户端默认采样参数即触发失败。
6. 顶层 `metadata` / `background` 被 Grok 上游拒绝（客户端当前不发，未纳入本次改动）。

## approved design decisions

1. 归一化保持在 Grok provider 内（`GrokProvider::normalize_responses_body`），
   只作用于 OAuth 账号的原生 Responses 上游，其他 provider 不受影响。
2. `tools` 按上游白名单过滤：仅保留 `GROK_TOOL_TYPES` 内的类型；被移除的工具
   只记日志，不做类型转换。
3. `web_search` 工具剥离 `GROK_UNSUPPORTED_WEB_SEARCH_FIELDS`，其余字段原样保留。
4. `tool_choice` 只在“存在实际工具”且“引用的工具体仍然存在”时保留；
   否则移除，避免上游 400。
5. `Chat→Responses` 补齐客户端常用字段：`temperature` / `top_p` / `stop` /
   `parallel_tool_calls` 原样透传（与 `Messages→Responses` 的现有映射一致，两个
   上游实测均接受）；`presence_penalty` / `frequency_penalty` / `seed` /
   `user` / `n` / `logit_bias` / `service_tier` 丢弃并写入 `normalized`
   审计上下文，而不是让整段请求 400。

## expected affected files

- `src-tauri/src/auth_provider/grok_backend.rs`
- `src-tauri/src/protocol/codec/responses_codec/encode_chat.rs`
- `src-tauri/src/protocol/codec/responses_codec/tests.rs`
- `docs/changes/grok-client-tool-compat/{plan.md,execution.md}`

## implementation steps

1. 在 Grok provider 内加入工具白名单与 `web_search` 字段剥离，扩充 `tool_choice` 清理。
2. 在 `Chat→Responses` 内加入透传 / 丢弃两组字段与类型校验。
3. 补单元与 mock HTTP 回归测试（Codex 形状工具集、字段剥离、悬空 tool_choice）。
4. 运行格式化、相关测试，并用真实 Grok 上游做端到端验证。

## acceptance criteria

- Codex 形状请求（namespace + web_search + external_web_access）经 Grok 上游返回 200。
- 仅含 namespace 的请求不会留下指向已删除工具的 `tool_choice`。
- 支持类型（function / mcp / shell / tool_search …）与 `web_search` 的其余字段不被改写。
- Chat 客户端带 `temperature` / `top_p` / penalties / seed 时请求可正常完成，
  丢弃字段在 `normalized` 中可见。
- 非 Grok provider 的代码路径无行为变化，现有测试不回归。

## verification commands

```bash
cd src-tauri && cargo test grok_backend
cd src-tauri && cargo test responses_codec
cd src-tauri && cargo test protocol::codec
cd src-tauri && cargo test auth_provider
cargo fmt --all -- --check
```

## rollback or failure considerations

归一化只在出站前删除字段，不会伪造语义；若某个被移除的工具实为上游必需，
请求仍会被上游拒绝而不是静默改变行为。回滚只需还原 provider 的归一化调用与
`Chat→Responses` 字段表。

## unresolved questions

- `namespace` 内嵌套的 function 工具（`multi_agent_v1` / `mcp__computer_use`）
  当前随 namespace 一并移除：这些能力在 Grok 下不可用，但不会让会话失败。
  是否展平为顶层 function 属于产品决策，未在本任务内实施。
- Anthropic 内置工具（`web_search_20250305` 等）在 `Messages→Responses`
  仍是 fail-closed；Claude Code 默认工具为自定义类型，未触发。
- 运行中的桌面实例仍是旧二进制，需重启后生效。
