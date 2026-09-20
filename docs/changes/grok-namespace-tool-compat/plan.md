# Grok Responses namespace 工具兼容

## status
IMPLEMENTED

## objective
修复 Grok OAuth 原生 Responses 路径将 `tools[].type = "namespace"` 原样透传导致上游 422 的问题，参考项目现有 Kimi/Claude 转换路径的非 function 工具兼容行为，并保持其他 provider 不变。

## non-goals
- 不修改公共 Responses codec 的全局行为。
- 不修改 Codex、Kimi、Gemini、Claude provider 的请求格式。
- 不将 `namespace` 盲目转换成 `mcp` 或 function。
- 不提交 OAuth token、client secret 或其他敏感信息。

## current behavior and evidence
- Grok provider 固定发送 Responses 请求，`grok_backend.rs` 的 `outbound` 对 `request.body` 直接 `.json(...)`。
- Responses → Responses 使用 identity codec，仅替换模型并保留 tools。
- Grok 上游已返回 `tools[7].type` 不支持 `namespace` 的 422。
- Kimi Chat/Claude 转换路径会过滤非 function 工具；Responses → Messages 路径会对非 function 工具本地拒绝。
- `xai-org/grok-build` 的工具模型将 namespace 作为工具描述的分组元数据，而非 wire-level `tools[].type` 变体；因此不做 `namespace -> mcp` 猜测转换。

## approved design decisions
1. 仅在 `GrokProvider::outbound` 前对 Responses JSON 做 provider-scoped normalization。
2. 从 `tools` 数组移除 `type == "namespace"` 的声明项，其他 Grok 原生工具和 function 工具保持原样。
3. 若过滤后 tools 为空，移除 `tools` 及 `tool_choice`，避免 tool_choice 指向已删除工具。
4. 若 `tool_choice` 本身是 `type == "namespace"`，移除该 tool_choice；普通字符串和 function/tool 类型保持原样。
5. 增加单元/集成级 provider mock 断言：上游收到 function，未收到 namespace；无 namespace 的请求不改变。

## expected affected files
- `src-tauri/src/auth_provider/grok_backend.rs`
- `docs/changes/grok-namespace-tool-compat/plan.md`
- `docs/changes/grok-namespace-tool-compat/execution.md`

## implementation steps
1. 添加 Grok Responses body normalization helper。
2. 在 Grok outbound 发送前使用归一化后的 body。
3. 为 namespace removal、tool_choice 清理和 function 保留增加测试。
4. 运行格式化、Grok/provider/protocol 测试、前端构建和 diff 检查。

## acceptance criteria
- Grok 上游请求不再包含 `tools[].type == "namespace"`。
- function 工具及 Grok 已支持的其他工具声明不被删除或改写。
- 过滤后不会保留指向已删除 namespace 工具的 `tool_choice`。
- Codex/Kimi/Gemini/Claude 公共路径无代码变化。
- 不引入任何 secret。

## verification commands
- `cd src-tauri && cargo test grok_backend -- --nocapture`
- `cd src-tauri && cargo test auth -- --nocapture`
- `cd src-tauri && cargo test responses -- --nocapture`
- `pnpm build`
- `cargo fmt --all -- --check`
- `git diff --check HEAD --`

## compatibility requirements
保留现有 API、RoutePlan、ProviderRequest 和凭证处理接口；只改变 Grok OAuth 原生 Responses 的请求体兼容行为。

## rollback or failure considerations
若上游实际需要某种 namespace 子工具语义，当前过滤会使该工具不可用，但不会伪造 mcp/function 调用。可根据后续抓取到的完整工具定义增加明确映射；回滚仅需移除 Grok normalization 调用和对应测试。

## unresolved questions
无阻塞问题。namespace 的完整请求结构未包含在用户错误文本中，因此本次只做安全的 marker 过滤，不做结构猜测转换。

## baseline
- Branch/HEAD: `v0.3.4` / `9fea551ebed4309e543fa17b5d66adc3d1b5bc23`
- Pre-existing dirty files: multiple existing auth/provider/server/test files plus `.omx/` and `docs/changes/gemini-oauth-login-fix/`; all are out of scope and must be preserved.
