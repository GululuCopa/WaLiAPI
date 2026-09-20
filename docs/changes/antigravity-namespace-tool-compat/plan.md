# Antigravity namespace 工具兼容调整

状态：VERIFIED
日期：2026-09-20
基线：`9fea551ebed4309e543fa17b5d66adc3d1b5bc23`

## objective

让 WaLiCode 发送到 Antigravity 的 Responses 请求兼容 `tools[].type = "namespace"` 声明，避免该类工具在 Responses→Gemini 转换入口被直接拒绝。

## non-goals

- 不修改 Antigravity OAuth、token、账号状态或 onboarding 流程。
- 不修改 Gemini Code Assist 上游 envelope、请求地址、headers 或响应解码。
- 不把 `namespace` 猜测转换成 `mcp` 或 `function`，也不保证该类工具可在 Antigravity 上执行。
- 不影响 Codex、Kimi、Grok 或其他 codec。
- 不执行 commit、push、reset、checkout，不写入任何 secret。

## current behavior and supporting evidence

- Antigravity provider 内部使用 `ProviderKind::Gemini`，请求路径为 Responses→Gemini。
- `src-tauri/src/protocol/codec/gemini/mod.rs::validate_responses_for_gemini` 当前将所有非 `function` 工具拒绝。
- 因此 WaLiCode 的 `namespace` 工具会在本地 codec 阶段失败，尚未到达 Antigravity Code Assist 上游。
- Grok 已有 provider-specific 的 namespace 清理，但 Antigravity 的过滤点必须位于 Gemini codec 的转换入口之前。

## approved design decisions

1. 在 `ResponsesToGemini::encode_request` 内对请求做最小归一化：删除 `tools` 数组中 `type == "namespace"` 的项目。
2. 若 `tool_choice.type == "namespace"`，删除 `tool_choice`；若过滤后没有工具，同时删除 `tools` 与 `tool_choice`。
3. 保留 `function` 工具以及其他非 namespace 工具，让现有校验继续拒绝不支持的内置工具。
4. 通过回归测试锁定：namespace 被忽略、function 保留、仅 namespace 时不会产生空工具配置、web_search 等其他不支持工具仍被拒绝。

## expected affected files

- `src-tauri/src/protocol/codec/gemini/mod.rs`
- `docs/changes/antigravity-namespace-tool-compat/plan.md`
- `docs/changes/antigravity-namespace-tool-compat/execution.md`

## implementation steps

1. 增加 Gemini Responses 请求的 namespace 归一化函数。
2. 在验证和 `responses_to_openai` 转换前使用归一化请求。
3. 增加最小单元测试。
4. 运行 Gemini/protocol 测试、格式检查、diff 检查，并确认前端无需变更。
5. 复核精确 diff，保留工作区其他既有修改。

## acceptance criteria

- 含 namespace 与 function 工具的请求可以转换，生成的 Gemini 请求只包含 function 声明。
- 仅含 namespace 工具的请求可以转换为无 tools 的普通 Gemini 请求。
- `tool_choice.type = "namespace"` 不会传入下游转换器。
- `web_search` 等非 namespace 的不支持工具仍然被拒绝。
- Antigravity OAuth 和其他 provider 的现有测试不回归。

## verification commands

```bash
cargo test --manifest-path src-tauri/Cargo.toml gemini -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml protocol::codec -- --nocapture
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
git diff --check HEAD --
```

## compatibility requirements

保持内部 provider ID `gemini`、Antigravity OAuth contract、Gemini Code Assist 请求结构和现有 function tool 行为不变。过滤仅针对明确标记为 `namespace` 的 Responses 工具。

## rollback or failure considerations

如果测试表明 `namespace` 过滤会改变既有 function tool 转换或破坏其他 Gemini codec 行为，应撤回本任务改动，仅保留现有 fail-closed 行为。不得扩大为公共 Responses codec 的全局转换规则。

## unresolved questions

- `namespace` 工具在 WaLiCode 上游的实际执行语义不属于本次范围；本次只保证文本请求不因该 marker 被 Antigravity 本地转换拒绝。

## baseline dirty files

执行前工作区已有多个 Rust 文件修改、`.omx/` 及 Gemini/Grok 任务文档未跟踪文件；本任务只修改上述 codec 文件和本任务文档，不覆盖或整理其他既有改动。
