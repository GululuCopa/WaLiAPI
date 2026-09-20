# Antigravity OAuth token exchange 修复

状态：VERIFIED
日期：2026-09-20
基线：工作区当前状态（存在其他任务的 staged/unstaged 改动，不清理、不覆盖）

## objective

补强浏览器授权完成后的 Google OAuth token exchange 诊断，同时确保仓库不包含任何真实 OAuth client secret。

## non-goals

- 不修改数据库 schema、provider ID、payload 版本或模型同步流程。
- 不改变 localhost callback、state 校验和取消逻辑。
- 不清理、回退、提交或推送工作区中的其他改动。
- 不记录 authorization code、access token、refresh token、client secret 或完整 OAuth 请求体。

## current behavior and supporting evidence

- 浏览器授权和 localhost callback 已完成，但 `GeminiLogin::exchange_code` 默认仅发送 `client_id`，未发送 client secret。
- `parse_token_response` 将 Google 非 2xx 响应统一隐藏为 `TokenExchangeFailed`，因此 UI 只显示泛化错误。
- 本机 `/Applications/Antigravity.app/Contents/Resources/bin/language_server` 含有官方 Antigravity client ID 与对应 OAuth client secret material；官方 auth log 的授权 URL 使用同一 client ID、scope 与 localhost callback。
- 官方 Antigravity OAuth token exchange/refresh 在部分 client 配置下需要 client secret；WaLiAPI 只在用户运行时设置环境变量时附带 secret，仓库和安装包不内置该敏感值。

## approved design decisions

1. 不在仓库或安装包中内置 client secret；`WALIAPI_ANTIGRAVITY_CLIENT_SECRET` 仅作为运行时可选注入，满足自定义部署。
2. authorization-code exchange 和 refresh 仅在运行时提供 secret 时附带 `client_secret`，两条 Google token endpoint 路径保持一致。
3. 对非 2xx token exchange 响应仅提取安全的 `error` / `error_description` 字段写入脱敏日志，不记录敏感凭据；用户可见文案继续保持稳定。
4. 保持现有 payload、回调、取消、refresh 错误分类和前端 API contract。

## expected affected files

- `src-tauri/src/auth_provider/gemini_login.rs`
- 本目录 `execution.md`

## implementation steps

1. 移除源码中的真实 client secret，仅保留环境变量读取；补充非 2xx 响应的安全错误码诊断。
2. 为默认 credentials 与 token form 补充回归测试。
3. 增加安全的 token exchange 非 2xx 诊断日志。
4. 运行 Rust/前端构建、格式和 diff 检查。
5. 重新打包并安装 `/Applications/WaLiAPI.app`，记录构建限制或验证结果。

## acceptance criteria

- 未设置环境变量时，默认 exchange form 只包含内置 client ID，不包含任何 secret。
- refresh form 与 exchange 使用同一套可选 secret 规则；显式环境变量仍可覆盖 client ID，并提供 secret。
- OAuth 敏感值不出现在日志、错误文案或测试输出中。
- 现有 Gemini/Auth 测试与前端构建通过。
- 安装后的应用包含本次修复。

## verification commands

```bash
cd src-tauri && cargo test gemini -- --nocapture
cd src-tauri && cargo test auth -- --nocapture
cargo fmt --check --manifest-path src-tauri/Cargo.toml
pnpm build
git diff --check HEAD --
```

## compatibility requirements

保持 `gemini` provider ID、Antigravity payload v2 marker、Google OAuth endpoint、`localhost:/oauth-callback` 形状以及桌面/Web 管理 API contract 不变。

## rollback or failure considerations

若真实 Google token endpoint 仍拒绝交换，先依据脱敏日志中的安全错误码继续诊断；不得将 authorization code/token/secret 写入日志或提交。回滚范围仅限本目录列出的 OAuth client material 与 exchange 诊断改动。

## unresolved questions

无。当前已确认浏览器授权成功、失败点在 token exchange；真实 client secret 不进入仓库，若上游要求 secret，需由部署环境运行时注入。
