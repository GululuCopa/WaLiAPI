# Antigravity OAuth 修复进入 v0.3.6

日期：2026-09-22

## 请求与基线

- 用户要求修复新发布资产的 Antigravity 登录问题并提交 PR；维护者指定目标分支为 `v0.3.6`。
- 基线：`origin/v0.3.6` = `f207634585231afab345113e8aa04b265a2a847a`，与本轮获取的 `origin/main` 相同。
- 独立 worktree：`/private/tmp/waliapi-main-review`，开始时干净；分支 `fix/antigravity-oauth-v036`。
- 原工作区仍在 `v0.3.4`，16 个已修改文件及 `.omx/`、两份未跟踪任务目录均保留，不进入本 PR。
- 用户的修复 PR 请求授权本轮 Codex 直接执行、提交、推送修复分支并创建 PR；不授权合并、发布或更改已有标签。

## 已核实的行为

- `gemini_login.rs` 默认只从运行时环境变量读取 client secret；授权 URL 不含 PKCE，exchange 不含 verifier。
- 已有修复 `e27aa9c` 经 PR #126 合入 `v0.3.4`，不在 `v0.3.6` / `waliapi-v0.3.5` 的祖先链中。
- 发布流程按所选 ref checkout；发布名称取项目版本号，不保证包含旧维护分支的修复。
- 旧 mock token server 未检查 secret 或 verifier，旧登录测试成功不能证明实际请求正确。

## 范围

只修改 `src-tauri/src/auth_provider/gemini_login.rs` 及本任务文档。保留协议、payload、数据库、其他 provider 和发布工作流；不升级版本号。真实账号授权和签名安装包验证需维护者在发布前执行，不冒充已验证。
