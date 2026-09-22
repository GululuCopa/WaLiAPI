# 执行记录

## result
IMPLEMENTED — 已构建 release 并安装到 `/Applications/WaLiAPI.app`，真实 Codex 多轮验证通过。

## changed files

- `src-tauri/src/auth_provider/grok_backend.rs`
  - 新增常量 `GROK_ENCRYPTED_REASONING_INCLUDE`（`reasoning.encrypted_content`）。
  - `normalize_responses_body` 扩展：
    - `include` 移除 `reasoning.encrypted_content`，数组为空时删除整个 `include`（其它值保留）；
    - `input` 中 `type == "reasoning"` 的条目剥离 `encrypted_content`；
    - 上述条目 `content == null` 时补成 `[]`（上游要求数组）。
  - 新增 3 个测试：`normalize_responses_body_drops_encrypted_reasoning_blobs`、
    `normalize_responses_body_keeps_other_include_entries`、
    `responses_profile_strips_encrypted_reasoning_before_http`。
- `docs/changes/grok-encrypted-reasoning-blob/{plan.md,execution.md}`

## verification

- `cargo test grok_backend` → 25 passed（含 3 个新增）。
- 临时 headless 实例（debug 二进制 + 数据目录副本，端口 8899）上，真实 Codex CLI
  `exec` + `resume` 连续三轮 → 全部成功（轮2 即原先 400 的场景）。
- `./node_modules/.bin/tauri build --no-bundle`（8 分 08 秒）→ 安装前校验二进制
  内含前端资源（`index-CIGcmKCe.js` 命中 1 次），再替换 `.app` 内二进制、
  adhoc 重签名并重启。
- 安装后以用户真实配置（默认 provider = waliapi，`model=grok-4.7`）直连 8777
  再跑三轮 `exec`/`resume` → 全部成功（ok / done / finished）。
- 网关 `/health` 正常，进程 PID 67327（00:05:41 启动）。

## 诊断要点（供后续参考）

- 抓包确认 Codex 第二轮请求里的 reasoning `(id, encrypted_content)` 与第一轮响应
  逐字节一致，网关 identity 路径不改 reasoning 条目 → 排除网关篡改。
- 受控实验（curl 直接生成并回放、Codex 真实请求体生成后回放、流式与非流式组合、
  0/15/45/60/75/120/150/210 秒延迟）**全部 200**，无法复现 400；
  仅真实 Codex 会话必现 → 判定为上游依赖短时服务端状态，不可作为可回放上下文。
- 其它被排除的因素：`client_metadata`（含 turn/root_turn_id）、`prompt_cache_key`、
  `tools` / `tool_choice` / `parallel_tool_calls`、`instructions`、`store:false`、
  `stream`、reasoning `summary` 内容、reasoning `id` 伪造与否。

## limitation

- 剥离 encrypted reasoning 后，模型每轮重新推理，跨轮不携带加密推理上下文
  （明文 summary 仍保留）。这是为绕开上游限制付出的代价。
- 上游拒绝自身 blob 的确切机制未定位（服务端状态时效或绑定关系），本次为规避式修复。
- `.app` 内的 `waliapi-web`（headless 二进制）仍未包含本次改动（缺 `web/dist`，
  需要时可单独构建）。
- 旧版本二进制备份仍在 `/Applications/WaLiAPI.app/Contents/MacOS/waliapi.bak-20260922-231708`。

## security
未提交任何 token / secret；抓包与数据副本仅本地使用，验证后已清理。
