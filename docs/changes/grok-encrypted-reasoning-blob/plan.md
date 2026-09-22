# Grok reasoning encrypted_content（compaction blob）兼容

状态：IMPLEMENTED（已构建安装，真实 Codex 多轮验证通过）

## objective

修复 Codex CLI 通过 WaLiAPI 调用 Grok 模型时，多轮会话第二轮起报
`400 Could not decode the compaction blob. Ensure it is unmodified from the compact response.`
的问题，让 grok 路径不再依赖不可回放的加密推理上下文。

## non-goals

- 不修改 Grok OAuth、路由、模型同步。
- 不改变其它 provider 的 encrypted reasoning 行为（Codex / Kimi / Gemini 账号照旧）。
- 不尝试"改写 blob 或 id 让上游接受"（上游校验机制不明，属于猜测）。

## current behavior and evidence

现象：Codex CLI 0.155.1 在 `Copazk`（映射到 `grok-4.7`）会话里，第一轮 `hi` 成功，
第二轮（含历史 reasoning）直接 400，错误来自 Grok 上游（cli-chat-proxy），
网关以 `route_plan_error / caller_terminal` 透传。

实测定位（全部针对真实 Grok 账号）：

1. Codex 第一轮请求带 `include: ["reasoning.encrypted_content"]`，上游返回的
   reasoning 条目含 `encrypted_content`；Codex 第二轮**原样**回放该条目
   （抓包比对：请求里的 blob 与响应里的 blob 逐字节一致，网关未改动）。
2. 上游同时要求 reasoning 条目的 `content` 是数组：剥离 `encrypted_content`
   但保留 `content: null` 时，上游返回 422
   `invalid "reasoning" item: content: invalid type: null, expected a sequence`。
3. 受控实验（curl 生成的 blob）在"生成→立即回放"和"生成→120/210 秒后回放"
   两种情况下均返回 200；改用 Codex 真实请求体（含 tools / instructions /
   client_metadata / store:false / stream:true）生成 blob 后立即回放同样 200。
4. 但只要落到**真实 Codex 会话**，同一对 (id, blob) 回放必然 400 —— 抓包与
   受控回放都无法复现成功路径，说明上游解码依赖某段无法在受控实验中保持的
   短时服务端状态（ZDR 上下文），该 blob 不能当作可回放历史。
5. 剥离 `encrypted_content`（并把 `content: null` 补成 `[]`）后，第二轮请求
   返回 200，会话继续可用。

## approved design decisions

1. Grok 出站请求不再请求加密推理：从 `include` 数组移除
   `reasoning.encrypted_content`，数组为空时删除 `include`；其它 include 值保留。
2. 历史 `input[].type == "reasoning"` 的 `encrypted_content` 一律剥离，
   保证既有会话（已带 blob）也能继续；`content: null` 补成 `[]`（上游要求数组）。
3. 明文 `summary` 完整保留，模型每轮重新推理；代价是不再跨轮携带加密推理上下文。
4. 归一化仍限定在 `GrokProvider::normalize_responses_body`，不影响其它 provider。

## expected affected files

- `src-tauri/src/auth_provider/grok_backend.rs`
- `docs/changes/grok-encrypted-reasoning-blob/{plan.md,execution.md}`

## verification commands

```bash
cd src-tauri && cargo test grok_backend
cd src-tauri && cargo test auth_provider
./node_modules/.bin/tauri build --no-bundle
```

## acceptance criteria

- Codex 第二轮及后续轮次不再出现 `Could not decode the compaction blob`。
- 出站请求不含 `reasoning.encrypted_content`（include 与历史条目均无）。
- reasoning 条目的明文 summary 保留，`content` 为数组。
- 其它 provider 与现有测试不回归。

## unresolved questions

- Grok 上游在真实会话中拒绝自身签发的 blob 的底层机制（服务端状态时效 / 绑定）
  未完全定位；本次采取"不使用该字段"的规避方案，与上游行为无关地保持可用。
