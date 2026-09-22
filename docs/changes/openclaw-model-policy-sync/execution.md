# 执行记录

## result
IMPLEMENTED — OpenClaw 现场配置已修复并验证可用；WaLiAPI 代码修复已构建安装，防止下次应用配置时复发。

## changed files

- `src-tauri/src/commands/app_config.rs`
  - `write_openclaw`：
    - 新增 `provider_prefix`（`waliapi/`）。
    - `agents.defaults.models`：先移除 `waliapi/*` 旧键再写入当前模型。
    - 新增 `agents.defaults.modelPolicy.allow` 维护：移除 `waliapi/*` 后追加
      `waliapi/<model>`，其它 provider 条目保持不变。
  - 新增测试 `openclaw_config_syncs_model_policy_allowlist`（含旧 `waliapi/wali`
    条目的配置 → 断言 allow 与 models 都被换成当前模型，其它 provider 条目保留）。
- `docs/changes/openclaw-model-policy-sync/{plan.md,execution.md}`

## 现场修复（用户机器）

1. 备份原配置：`/tmp/openclaw.json.bak-20260923-004336`，并持久化为
   `~/.openclaw/openclaw.json.backup.waliapi-fix`。
2. 修正 `~/.openclaw/openclaw.json`：
   - `modelPolicy.allow` → `["cliproxy/gpt-5.2", "zai/glm-4.7", "waliapi/Copazk"]`
   - `agents.defaults.models` → 同样移除 `waliapi/wali`、保留其它 provider 配置
     （`zai/glm-4.7` 的 `alias: GLM` 未动）
3. `openclaw gateway restart`（LaunchAgent `ai.openclaw.gateway`）。

## verification

- `openclaw models list`（重启后）：
  `cliproxy/gpt-5.2`、**`waliapi/Copazk`（default, configured）**、`zai/glm-4.7`。
- `openclaw agent -m "…" --json` → `status: ok`，`provider: waliapi`，`model: Copazk`，
  两次实测分别返回 `hi` 与 `openclaw ok`。
- WaLiAPI 日志对照：
  - 修复前 `16:41–16:43`：`model=wali` → **503** ×6（`No available upstream candidate`）
  - 修复后 `16:44:23`：`model=Copazk` → **200**，`upstream_model=grok-4.7`，41100 tokens
- `cargo test app_config` → 25 passed。
- `cargo test`（全量）→ **1099 passed / 0 failed**。
- `./node_modules/.bin/tauri build --no-bundle`（8 分 00 秒）→ 安装前校验前端资源已嵌入
  （`index-CIGcmKCe.js` 命中 1 次）→ 替换 `.app` 内二进制、adhoc 重签名、重启
  （PID 61628，00:53:08 启动，`/health` 正常）。

## limitation

- 本次未在"通过 WaLiAPI 界面重新应用 OpenClaw 配置"的真实链路上做端到端复验
  （需要在 GUI 中操作）；该路径由 `cargo test app_config` 的单元测试覆盖，
  现场配置则是按同一规则手工修正并实测可用的。
- OpenClaw 的 `modelPolicy` 还可能承载其它字段（本次只触碰 `allow`，其余原样保留）。
- 用户的 `cliproxy/gpt-5.2`、`zai/glm-4.7` 等其它 provider 条目未被修改。

## 现场复验与追加修正（2026-09-23 凌晨）

用户在界面上重新应用配置后，OpenClaw 报 `Invalid config`。排查结论与处理：

1. **配置被回退成了旧版本快照**：`meta.lastTouchedAt`、
   `gateway.tailscale.resetOnExit`、`channels.telegram.streamMode` 三个当前版本
   （2026.9.5）不认识的键出现，且 `meta.lastTouchedVersion` 变回 `0.1.7`。
   WaLiAPI 写入前的快照 `openclaw.json.waliapi-backup` 里这些键**已经存在**，
   说明不是本网关引入（`write_openclaw` 只写 provider 目录与 `agents.defaults`
   的指定字段，从不碰 `meta` / `gateway` / `channels`）。
   处理：删除两个未知键，`channels.telegram.streamMode` 交由
   `openclaw doctor --fix` 迁移为 `channels.telegram.streaming.mode`；随后
   `openclaw gateway start` 恢复（doctor 修复过程中 gateway 被停机且未自动拉起）。
2. **发现并修复了本网关的真实缺陷**：现场 `modelPolicy` 原本不存在
   （即“不限制”），`write_openclaw` 却新建了只含 `waliapi/<model>` 的 `allow`，
   把用户其它 provider 的模型全部隐藏。现已改为**仅在 `modelPolicy` 已存在时**
   维护 `allow`，并新增回归测试 `openclaw_config_does_not_create_model_policy`。
3. 复验：`openclaw models list` 恢复列出 `waliapi/grok-4.7`（default）、
   `cliproxy/gpt-5.2` 与 zai 目录模型；`openclaw agent` 回合正常。

## security
未写入或输出任何 token / API key；配置备份保留在用户目录，验证脚本已清理。
