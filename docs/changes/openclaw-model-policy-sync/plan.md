# OpenClaw modelPolicy.allow 同步（不可用修复）

状态：IMPLEMENTED（现场配置已修复，代码修复已构建安装）

## objective

修复 OpenClaw 通过 WaLiAPI 不可用的问题：agent 反复请求一个网关不存在的模型
（`wali`）并进入 provider cooldown。

## 现象与证据

- OpenClaw CLI：
  `All models failed (1): waliapi/wali: Provider waliapi is in cooldown (timeout) | MODEL_FALLBACK_SKIPPED`
- WaLiAPI 日志：`16:41–16:43` 连续 6 条 `model=wali` → `503 No available upstream candidate for model: wali`
  （`/v1/chat/completions`，与配置的 `api: openai-completions` 一致）。
- `openclaw models list` 只列出 `cliproxy/gpt-5.2`、`waliapi/wali`、`zai/glm-4.7`，
  **看不到** 配置里 primary 指向的 `waliapi/Copazk`。
- `~/.openclaw/openclaw.json` 实测状态：
  - `models.providers.waliapi.models = [{id: "Copazk"}]`（WaLiAPI 写入）
  - `agents.defaults.model.primary = "waliapi/Copazk"`（WaLiAPI 写入）
  - `agents.defaults.modelPolicy.allow = ["cliproxy/gpt-5.2", "waliapi/wali", "zai/glm-4.7"]`
    —— **没有 `waliapi/Copazk`**，却留着已不存在的 `waliapi/wali`

## 根因

`agents.defaults.modelPolicy.allow` 是 OpenClaw 的**模型可见性策略**（白名单）：
只有列表内的模型会暴露给 agent（`openclaw models list` 的输出即为该列表的子集）。
`write_openclaw` 只写了 provider 目录、`model.primary` 与 `agents.defaults.models`，
**没有同步 `modelPolicy.allow`**，于是：

1. primary 指向的 `waliapi/Copazk` 不可见；
2. agent 退回白名单里仍然存在的 `waliapi/wali`；
3. `wali` 既不在 provider 的 models 目录里，网关也不认识 → 503；
4. 连续失败使 provider 进入 cooldown → 整个 OpenClaw 不可用。

## approved design decisions

1. `write_openclaw` 同步维护 `agents.defaults.modelPolicy.allow`：移除
   `waliapi/*` 历史条目后加入本次写入的 `waliapi/<model>`，其它 provider 的条目原样保留。
   waliapi provider 的模型目录由本网关整体覆盖，因此这些条目属于我们管理的范围。
   **仅当 `modelPolicy` 已经存在时**才维护：OpenClaw 缺少 `modelPolicy` 表示
   “不限制”，凭空新建白名单会把用户其它 provider 的模型全部挡掉（现场复验发现）。
2. 同样清理 `agents.defaults.models` 里陈旧的 `waliapi/*` 键，保持两处一致
   （其它 provider 的条目与其配置值不动）。
3. OpenClaw 的配置 schema 对未知键严格校验，仍不写任何自定义标记
   （沿用 `models.providers.waliapi` 作为「已应用」证据）。

## expected affected files

- `src-tauri/src/commands/app_config.rs`
- `docs/changes/openclaw-model-policy-sync/{plan.md,execution.md}`

## verification commands

```bash
cd src-tauri && cargo test app_config
cd src-tauri && cargo test
openclaw models list
openclaw agent -m "reply with hi" --json
```

## acceptance criteria

- 应用配置后 `modelPolicy.allow` 包含本次写入的 `waliapi/<model>`，且不含其它 `waliapi/*` 旧条目。
- 其它 provider 的 allow 条目与其 `agents.defaults.models` 配置保持不变。
- OpenClaw 能通过 `waliapi/<model>` 正常完成一次 agent 回合。
