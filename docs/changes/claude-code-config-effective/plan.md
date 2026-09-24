# Claude Code 网关配置生效修正

状态：IMPLEMENTED

## objective

修正 Claude Code 2.1.280 在已有 `ANTHROPIC_MODEL` 或自定义
`CLAUDE_CONFIG_DIR` 时，WaLiAPI 配置显示写入成功但实际请求仍使用旧模型或旧配置目录的问题。

## non-goals

- 不改变 Claude Code 的上游协议转换、认证格式或模型路由策略。
- 不修改用户未被网关配置覆盖的 settings 字段。
- 不修改当前工作树中预先存在的两个 OAuth 文档目录。

## baseline and evidence

- 基线提交：`f8ba37653ebdb8d6a920b58d8128cd346734e1ad`。
- 工作树预先存在未跟踪目录：`docs/changes/antigravity-oauth-public-client/`、
  `docs/changes/gemini-oauth-login-fix/`。
- 本机 Claude Code：`2.1.280`，配置文件当前为 `~/.claude/settings.json`。
- 本机 settings 当前含 `env.ANTHROPIC_MODEL=deepseek-flash`；历史 WaLiAPI 备份证明写入器曾写入 `http://127.0.0.1:8777` 和 `ANTHROPIC_AUTH_TOKEN`。
- 临时本机假 Anthropic 端点验证：Claude Code 请求模型取自 `ANTHROPIC_MODEL`，优先级高于顶层 `model`；settings 中的 `ANTHROPIC_AUTH_TOKEN` 和 `ANTHROPIC_BASE_URL` 能被读取并生成 `/v1/messages` 请求。

## approved design decisions

1. 当 settings 已有 `ANTHROPIC_MODEL`，应用 Claude Code 网关配置时将其同步为本次选择的模型，并将该键记录为 WaLiAPI 管理字段；后续重复应用继续更新它。未存在该环境变量时保留顶层 `model` 行为，避免新增环境覆盖用户的 `/model` 持久化选择。
2. Claude Code 配置目录优先读取非空 `CLAUDE_CONFIG_DIR`，否则回退到现有 `~/.claude`；应用列表、检测、读取、写入和恢复共用该路径。
3. 增加回归测试覆盖旧 `ANTHROPIC_MODEL` 覆盖、配置目录覆盖和本机协议探针记录。

## expected affected files

- `src-tauri/src/commands/app_config.rs` 及内联测试
- `docs/changes/claude-code-config-effective/plan.md`
- `docs/changes/claude-code-config-effective/execution.md`

## implementation steps

1. 先新增旧模型环境变量覆盖的失败回归测试。
2. 修改 Claude Code settings 投影，按批准设计同步 `ANTHROPIC_MODEL`。
3. 抽取 Claude Code 配置目录解析函数并接入应用定义。
4. 补充目录覆盖和事务写入测试，更新执行记录。
5. 运行定向 Rust 测试、全量 Rust 测试、前端构建、格式检查和差异检查。

## acceptance criteria

- 既有 `ANTHROPIC_MODEL=old` 时应用模型 `new`，Claude Code settings 中环境变量为 `new`，且后续应用仍可更新它。
- 未有 `ANTHROPIC_MODEL` 时不新增该环境变量，保留现有顶层 `model` 与 `/model` 选择语义。
- 设置 `CLAUDE_CONFIG_DIR` 时 Claude Code 应用配置路径、检测、读取和恢复均指向该目录。
- 本机 Claude Code 2.1.280 使用临时 settings 和本机端点探针时，请求携带网关 URL、Bearer token，并命中 `/v1/messages`。
- 既有配置字段和备份/恢复语义不回归。

## verification commands

- `cd src-tauri && cargo test commands::app_config`
- `cd src-tauri && cargo test`
- `pnpm build`
- `pnpm --filter waliapi-web build`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `git diff --check f8ba37653ebdb8d6a920b58d8128cd346734e1ad`

## compatibility and rollback

仅调整 Claude Code 配置投影和配置目录解析；回滚本次代码即可恢复旧行为，不涉及数据库或外部 schema 迁移。应用配置前已有 `.waliapi-backup` 机制可恢复原始 settings。

## unresolved questions

无。若 Claude Code 实际配置优先级与探针结果不一致，停止修改并记录新的可复现证据。
