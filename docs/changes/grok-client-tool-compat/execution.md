# 执行记录

## result
IMPLEMENTED（代码 + 单元/集成测试 + 真实上游端到端验证通过；运行中的桌面实例待重启生效）

## changed files

- `src-tauri/src/auth_provider/grok_backend.rs`
  - 新增 `GROK_TOOL_TYPES`（cli-chat-proxy 的工具类型白名单，来自上游 422 错误回显）
    与 `GROK_UNSUPPORTED_WEB_SEARCH_FIELDS`（`external_web_access`、`search_context_size`）。
  - `normalize_responses_body` 由“只删 namespace”扩展为通用归一化：按白名单过滤
    `tools`、剥离 `web_search` 上游不认的字段、`tool_choice` 仅在“有工具且引用仍存在”
    时保留（覆盖“无 tools 却带 tool_choice”与悬空 function 引用两种上游 400）。
  - 新增 4 个测试：web_search 字段剥离、未知工具类型移除、悬空 tool_choice 清理、
    Codex 形状工具集的 mock HTTP 出站断言。
- `src-tauri/src/protocol/codec/responses_codec/encode_chat.rs`
  - `CHAT_TOP_LEVEL` 增加 `temperature` / `top_p` / `stop` / `parallel_tool_calls`，
    并新增 `CHAT_PASSTHROUGH_FIELDS`（原样透传，与 Messages→Responses 一致）与
    `CHAT_DROPPED_FIELDS`（丢弃并记入 `normalized`），另补三处类型校验。
- `src-tauri/src/protocol/codec/responses_codec/tests.rs`
  - 新增 2 个测试：透传/丢弃行为与类型校验失败路径。

## verification

- `cargo test grok_backend` → 22 passed（含 4 个新增）。
- `cargo test responses_codec` → 30 passed（含 2 个新增）。
- `cargo test protocol::codec` → 167 passed。
- `cargo test auth_provider` → 200 passed。
- `cargo fmt --all -- --check` → 仅报 `auth_provider/types.rs`（仓库基线既有的 rustfmt
  版本差异，本次未触碰该文件；本任务涉及文件无格式差异）。
- `cargo clippy --lib` → 本任务涉及文件零告警（既有告警集中在 endpoint_executor /
  server / services::wiki 等未改动文件）。

### 真实上游端到端验证（新代码，临时 headless 实例 8899 端口 + 数据目录副本）

| 场景 | 请求特征 | 结果 |
| --- | --- | --- |
| A Codex | 抓包原样请求（15 工具：12 function + 2 namespace + web_search(external_web_access)） | **HTTP 200**（改前 400 `Argument not supported: external_web_access`） |
| B OpenCode | `/v1/chat/completions` + temperature/top_p/presence_penalty/seed | **HTTP 200**（改前 400 `Chat field "temperature" …`） |
| C Responses | 无 `tools` 仅 `tool_choice` | **HTTP 200**（改前 400 `A tool_choice was set … no tools`） |
| D Claude Code | `/v1/messages` + `type:"custom"` 工具 + thinking + cache_control | **HTTP 200** |
| E Hermes/OpenClaw | `/v1/chat/completions` + stream_options + parallel_tool_calls + tool_choice | **HTTP 200** |

规则完整性另以手工归一化的完整 Codex 请求（83KB，含 instructions / reasoning /
client_metadata / include / prompt_cache_key）直接打真实 Grok 上游复核为 200。

### 上游行为实测（用于确定规则，全部为一次性小请求）

- 工具类型白名单来自上游 422 回显：`function, web_search, x_search, image_generation,
  collections_search, file_search, code_execution, code_interpreter, mcp, shell, tool_search`。
- `web_search.external_web_access` / `search_context_size` → 400；`filters` /
  `user_location` / `indexed_web_access` / `search_content_types` → 200。
- 顶层 `temperature` / `top_p` / `include` / `prompt_cache_key` / `store` /
  `parallel_tool_calls` / `reasoning` / `text.verbosity` / `max_output_tokens` /
  `client_metadata` → 200；`metadata` / `background` → 400。
- input 条目类型白名单包含 `custom_tool_call`、`web_search_call`、`shell_call` 等；
  `local_shell_call` → 422（Codex 0.155.1 未使用该类型）。

## limitation

- 运行中的桌面实例（0.3.5）仍是旧二进制，本次改动需重新构建并重启后生效；
  端到端验证是在临时 headless 实例上完成的。
- `namespace` 内嵌的 function 子工具（子代理、computer_use）随 namespace 一并移除，
  在 Grok 下不可用（会话本身可用）；展平属产品决策，未实施。
- Claude Code 若启用 Anthropic 内置工具（`web_search_20250305` 等），
  `Messages→Responses` 仍是 fail-closed，未在本次范围内。
- 验证使用真实 Grok 账号产生了少量推理用量；未访问生产 OAuth，未写入任何配置或 secret。

## security
未新增或提交 token、client secret 或其他敏感信息；抓包与临时数据副本仅用于本地验证，
验证后已清理。
