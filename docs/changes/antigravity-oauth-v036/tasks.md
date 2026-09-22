# 任务包

状态：READY_FOR_EXECUTION

基线、预存改动、目标与设计见同目录 context.md / design.md。未决问题：无。

## 步骤

1. 增加严格本地登录回归并在基线上执行，确认缺失 secret / PKCE 能使测试失败。
2. 移植旧修复至最新 v0.3.6，增强随机性、默认配置隔离、严格 exchange/refresh 验证。
3. 审查准确 diff，执行下述验证，记录命令、结果与限制。
4. 仅提交范围内文件，推送到 fork；创建 base=`v0.3.6` 的 PR，复核目标与变更文件。

## 验收与验证

- 未配置环境变量时的生产默认配置包含配对 client material；环境变量覆盖保留。
- 实际授权 URL 与 exchange 请求的 challenge/verifier 对应；缺任一参数时 mock 拒绝，且基线复现失败。
- refresh 实际携带 client material，回调 state / payload / 重试既有行为不回退。
- 必须通过（Rust 命令 cwd=`src-tauri`）：
  - `cargo test --locked --lib auth_provider::gemini_login::tests`
  - `cargo test --locked auth`
  - `cargo fmt --all -- --check`
  - `pnpm build`（仓库根目录；前端未修改，使用已有依赖）
  - `git diff --check`
- 补充检查：完整后端测试用于识别基线问题；如失败须基线复现并记载，不把失败说成全绿。
- 发布前人工检查（非本轮可自动验证项）：合并提交在发布 ref 中，升级版本号后重新构建；实际账号完成首次登录及刷新。

用户请求覆盖创建 PR 所需的本地分支、commit、push 和 GitHub PR 创建；不做 rebase、强推、合并或发布。
