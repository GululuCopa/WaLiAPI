-- 042: Auth 账号模型映射支持开启/关闭（与渠道迁移 041 对齐）。
-- model_mapping_disabled 存 JSON 数组，元素为 [from, to] 映射对；
-- 空数组表示全部映射开启。路由匹配、别名目标解析、/v1/models 聚合
-- 均会跳过被关闭的映射对。
ALTER TABLE auth_accounts ADD COLUMN model_mapping_disabled TEXT NOT NULL DEFAULT '[]';
