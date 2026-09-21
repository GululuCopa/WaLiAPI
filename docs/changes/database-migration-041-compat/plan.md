# 数据库迁移 041 向后兼容修复

状态：READY_FOR_EXECUTION
日期：2026-09-21
目标分支：main

## 目标

补回 v0.3.4 已使用但 main 缺失的 041 号数据库迁移，避免用户使用包含 041 的数据库后启动较旧构建时因 `VersionMissing(41)` 直接崩溃。

## 当前行为与证据

- 用户本机数据库 `_sqlx_migrations` 已成功记录 version 41，description 为 `channel mapping disabled`。
- 实际启动 panic 为：`failed to run database migrations: VersionMissing(41)`。
- `origin/main` 缺少 `src-tauri/migrations/041_channel_mapping_disabled.sql`。
- `origin/v0.3.4` 已包含该迁移，内容为给 `channels` 增加 `model_mapping_disabled`，默认值为 `[]`。
- `Database::new_with_path` 通过 `sqlx::migrate!` 执行迁移，未知版本会触发 `expect` panic。

## 修复方式

1. 将 041 迁移按 v0.3.4 上游已发布内容补回 main。
2. 保持迁移编号、描述、SQL 和 checksum 一致，确保已有 version 41 数据库可被识别。
3. 增加回归测试，防止迁移 041 被后续分支遗漏。

## 非目标

- 不修改用户数据、不删除数据库、不回滚迁移。
- 不改变 OAuth、渠道路由或业务逻辑。
- 不处理其他未知 migration 版本的产品交互设计；本 PR 只解决已知 041 缺失问题。
- 不纳入其他工作区文件或任何 secret。

## 验收标准

- main 包含 `041_channel_mapping_disabled.sql`。
- 已记录 version 41 的数据库启动迁移不再返回 `VersionMissing(41)`。
- 迁移 041 checksum 与已发布数据库记录一致。
- 数据库相关测试、格式检查和 diff 检查通过。

## 验证命令

```bash
cargo test --manifest-path src-tauri/Cargo.toml db -- --nocapture
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
git diff --check HEAD --
```

## 回滚

删除该迁移文件并回退本 PR 即可；不涉及数据删除或不可逆业务变更。
