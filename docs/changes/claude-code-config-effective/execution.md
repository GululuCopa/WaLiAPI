# Claude Code 网关配置生效修正执行记录

状态：IMPLEMENTED
日期：2026-09-23
基线：`f8ba37653ebdb8d6a920b58d8128cd346734e1ad`

## 诊断结论

本机 Claude Code `2.1.280` 同时存在顶层 `model` 和 `env.ANTHROPIC_MODEL` 时，实际请求使用环境变量中的模型。WaLiAPI 旧实现只更新顶层 `model`，因此界面提示写入成功后，Claude Code 仍可能沿用旧的 `ANTHROPIC_MODEL`。

本机探针使用临时 `CLAUDE_CONFIG_DIR` 和回环 Anthropic mock 端点验证了以下行为：

- settings 中的 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN` 会被 Claude Code 读取；
- 请求路径为 `POST /v1/messages?beta=true`，认证头为 Bearer token；
- `ANTHROPIC_MODEL=env-model` 与顶层 `model=root-model` 同时存在时，请求体模型为 `env-model`。

探针只使用占位 token 和临时配置目录，没有修改真实用户配置。

## 实现内容

- `src-tauri/src/commands/app_config.rs`
  - 应用 Claude Code 网关配置时，如果已有 `ANTHROPIC_MODEL`，同步写入当前选择的模型，并把该键纳入 WaLiAPI 管理字段，保证后续重复应用仍可更新。
  - 未有 `ANTHROPIC_MODEL` 时继续只写顶层 `model`，保留 Claude Code `/model` 持久化选择语义。
  - Claude Code 配置目录优先使用非空 `CLAUDE_CONFIG_DIR`，否则回退到 `~/.claude`；应用、检测、读取、写入和恢复共用该路径。
  - 增加旧模型环境变量覆盖、模型环境变量重复应用和配置目录覆盖回归测试。

## 验证

- `cd src-tauri && cargo test commands::app_config`：31 passed, 0 failed。
- `cd src-tauri && cargo test`：1126 个单元测试及集成测试通过。
- `pnpm build`：通过。
- `pnpm --filter waliapi-web build`：通过。
- `rustfmt --edition 2021 --check src-tauri/src/commands/app_config.rs`：通过。
- `git diff --check`：通过。
- 本机 Claude Code `2.1.280` 回环 mock 探针：通过，确认配置目录、网关 URL、Bearer token、`/v1/messages` 和环境变量模型优先级。

## 限制

- 全仓库 `cargo fmt --check` 仍被基线中未修改的 `src-tauri/src/auth_provider/types.rs` 与 `src-tauri/src/endpoint_executor/grok_arguments.rs` 阻断；本任务涉及文件已单独格式化检查。
- 未修改真实 `~/.claude/settings.json`，因此没有做真实账号请求；上游请求行为由本机 Claude Code 与本地 mock 端点验证。
- 工作树中预先存在的 `docs/changes/antigravity-oauth-public-client/` 和 `docs/changes/gemini-oauth-login-fix/` 未修改。

## 安全与清理

- 未记录或提交真实 API key、OAuth token、授权码或 session 内容。
- 探针脚本和临时配置目录已在验证后清理。
