# 执行记录

状态：IMPLEMENTED
日期：2026-09-22

## 基线与范围

- 基线：`origin/v0.3.6` / `f207634585231afab345113e8aa04b265a2a847a`。
- 分支：`fix/antigravity-oauth-v036`。
- 仅修改：`src-tauri/src/auth_provider/gemini_login.rs` 与本目录任务文档。
- 原工作区的未提交改动未进入本分支。

## 实现

- 内置公开 Antigravity client material；`WALIAPI_ANTIGRAVITY_CLIENT_ID` 与 `WALIAPI_ANTIGRAVITY_CLIENT_SECRET` 仍可覆盖。
- 每次浏览器登录生成独立 PKCE verifier；授权 URL 增加 S256 challenge；授权码 exchange 携带对应 verifier。
- refresh 与授权码 exchange 都携带 client material。
- 本地 token endpoint 测试现在拒绝缺少 client secret 或授权码 verifier 的请求，避免只验证“返回成功”而漏测请求形状。
- 保留 localhost callback、state 校验、payload marker/version、刷新重试与错误分类。

## 验证

通过：

```text
rustfmt --edition 2021 --check src/auth_provider/gemini_login.rs
cargo test --locked --lib auth_provider::gemini_login::tests
  14 passed, 0 failed
cargo test --locked auth
  277 passed, 0 failed；另有 4 个 auth_repository 集成测试通过
pnpm build
  tsc 与 Vite production build 通过
 git diff --check
```

`cargo fmt --all -- --check` 未通过，但失败来自基线已有的 `src/auth_provider/types.rs` 格式差异；本次修改的 `gemini_login.rs` 已单文件 rustfmt 检查通过，未修改该无关文件。

构建与测试存在仓库既有 unused/dead-code 和 chunk-size warning；没有新增编译错误。

## 限制

- 未执行真实 Google 账号授权闭环、签名安装包启动或发布流水线；这些需在 PR 合并后以包含本修复的 v0.3.6 发布提交执行。
- 本次不执行合并、打标签或发布。创建 PR 时目标分支固定为 `v0.3.6`，避免修复只停留在旧版本分支。
