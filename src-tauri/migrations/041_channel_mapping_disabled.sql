-- 041: 渠道模型映射支持开启/关闭。
-- model_mapping_disabled 存 JSON 数组，元素为 [from, to] 映射对；
-- 空数组表示全部映射开启。路由匹配、上游模型解析、/v1/models 聚合
-- 均会跳过被关闭的映射对。
ALTER TABLE channels ADD COLUMN model_mapping_disabled TEXT NOT NULL DEFAULT '[]';
