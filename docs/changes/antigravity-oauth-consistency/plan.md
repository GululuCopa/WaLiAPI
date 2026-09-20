# Antigravity OAuth 与 Codex/Kimi 登录行为一致化

状态：VERIFIED
日期：2026-09-20
基线：93cee4b（工作区已有用户改动，禁止清理或覆盖）

## 目标

让 Antigravity/Gemini 的 OAuth 登录入口与 Codex、Kimi 一致：用户点击登录即可启动授权，不要求在启动 WaLiAPI 前手动设置 `WALIAPI_ANTIGRAVITY_CLIENT_ID` 或 `WALIAPI_ANTIGRAVITY_CLIENT_SECRET`。

## 当前行为与证据

- `GeminiLogin::new()` 从两个环境变量读取 client material；缺失时在浏览器尚未打开前返回 `OAuthConfigurationMissing`。
- `commands/auth.rs` 将该错误直接展示为“请在启动 WaLiAPI 前设置环境变量”。
- Codex 使用 provider 内置公开 client id；Kimi 使用 provider 内置 device-flow client id，二者都不要求启动前配置 OAuth 环境变量。
- 本机 Antigravity IDE 日志中的授权 URL 使用固定公开 client id `1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com`，没有显示用户可配置的 client secret；该值只作为公开 OAuth client identifier 使用，不记录 token 或授权码。

## 设计决策

1. 内置官方 Antigravity OAuth client id；不再把 client id/secret 作为登录前置条件。
2. 授权码交换和 refresh 请求默认只提交公开 client id，匹配桌面端 public OAuth client 行为；不再强制提交 client secret。
3. 保留环境变量作为可选兼容覆盖（若部署方明确提供自有 client，可覆盖 client id，并可附带 secret），但缺失时必须仍能进入浏览器授权流程。
4. 删除 `OAuthConfigurationMissing` 的用户可见错误分支及对应前端错误码，避免再显示误导提示。
5. 不改变回调地址、state 校验、token payload 版本、Antigravity onboarding、refresh 失败分类和账号持久化边界。

## 预计影响文件

- `src-tauri/src/auth_provider/gemini_login.rs`
- `src-tauri/src/auth_provider/types.rs`
- `src-tauri/src/commands/auth.rs`
- `src/types/index.ts`
- 相关单元测试与本任务 execution 记录

## 验收标准

- 未设置任何 `WALIAPI_ANTIGRAVITY_*` 变量时，点击 Antigravity 登录不会在启动阶段报 OAuth configuration 错误，并会调用浏览器 opener。
- token exchange/refresh 的默认表单包含内置 client id，不包含空的 client secret。
- 自定义环境变量覆盖仍可用，且 secret 仅在确实配置时提交。
- Codex/Kimi 行为不变。
- 原有 Gemini/Antigravity 单测、Auth 单测和前端构建通过。

## 验证命令

```bash
cd src-tauri && cargo test gemini -- --nocapture
cd src-tauri && cargo test auth -- --nocapture
pnpm build
git diff --check HEAD --
```

## 兼容与回滚

- 已保存的 Antigravity payload 格式不变，不需要迁移。
- 环境变量部署方式继续兼容，只从“必需配置”降级为可选覆盖。
- 若上游 token endpoint 实际要求 secret，可通过环境变量提供 secret；回滚仅涉及本任务列出的 OAuth client 解析和错误映射文件。

## 未决问题

无。官方公开 client id 已从本机 Antigravity 授权日志得到，真实 token exchange 仍受当前网络/账号授权条件限制，使用 mock 回归测试覆盖表单形状。
