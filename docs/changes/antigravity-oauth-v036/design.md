# 设计

## 目标与决策

1. 在实际发布分支 `v0.3.6` 上移植已合入旧分支的 `e27aa9c`：默认配对的公开桌面应用 client material；保留现有运行时环境变量覆盖，不接触用户个人凭据。
2. 每次登录生成独立、至少 256 位随机性的 URL-safe PKCE verifier；授权 URL 发送 S256 challenge，token exchange 发送同一次登录的 verifier。secret 与 PKCE 不是替代关系。
3. 登录与刷新使用同一 client 配置，保持 localhost callback、state、取消、超时、payload version 和错误分类。
4. 用严格的本地 token endpoint 驱动真实 login / refresh 路径，拒绝缺少 secret、缺少 verifier 或 challenge 不匹配的请求。检查默认配置、显式覆盖、请求形状和 PKCE 已知向量。
5. 默认配置测试不读取或修改进程环境：提取私有构造入口，生产入口仍从同名环境变量取值。

## 非目标 / 兼容性

不修改 OAuth scopes、端点、客户端公共接口、数据库、Codex/Kimi、UI、发布版本号或 CI 架构。公开应用级 client material 复用已合入 PR #126 的值；真实 token、授权码、运行时覆盖不写入日志、快照、文档。

## 发布边界与失败处理

PR base 必须是 `v0.3.6`，不得再次提交到 `v0.3.4`。合并后发布必须选择包含本修复的提交；从旧 ref 构建仍不会包含修复。本 PR 不自动发布，也不承诺上游 OAuth client 永远有效。

回滚只撤销本 PR 的单一源码文件与任务文档；若测试失败，先与基线对照，不顺带修复 codec 等无关模块。真实 Google 登录仍可能受网络、账号资格及上游 client 政策影响，需发布前人工验证。
