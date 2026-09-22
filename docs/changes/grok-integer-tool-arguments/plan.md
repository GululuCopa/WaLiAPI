# Grok 整数工具参数（浮点字面量）规范化

状态：IMPLEMENTED（已构建安装，端到端 A/B 验证通过）

## objective

让 Codex CLI 通过 WaLiAPI 调用 Grok 模型时，不再因为 grok 把整数参数写成浮点
（`{"yield_time_ms":4000.0}`）而在本地反序列化失败。

## 现象与证据

- Codex 侧报错：`failed to parse function arguments: invalid type: floating point
  `4000.0`, expected usize at line 1 column 428`（另有 `expected u64` 变体）。
- 真实会话统计（`~/.codex/sessions/2026/09/23/rollout-00-06-38…`）：24 次
  `exec_command` 调用中 `max_output_tokens` 10 次、`yield_time_ms` 4 次是浮点。
- 上游 SSE 里工具参数出现在三个位置：`response.function_call_arguments.delta`
  （实测为一次性完整 JSON，与 done 一致）、`response.function_call_arguments.done`
  的 `arguments`、`response.output_item.*` 的 `item.arguments`。

## approved design decisions

1. 归一化点放在 **Grok provider 的上游响应**（`endpoint_executor` 的 auth 账号
   流式/非流式两条路径），下游无论 Responses / Chat / Messages 都受益；
   其它 provider 完全不受影响（`needs_normalization(auth_provider) == "grok"`）。
2. 规则：把 JSON 里**数值上等于整数**的浮点字面量写成整数
   （`4000.0` → `4000`，`-2.0` → `-2`）。数值不变，对确实期望浮点的字段同样安全。
   小数（`1.5`）与超过 2^53 的值保持原样。
3. **只在语义真的变化时改写**：数值全为整数时返回 `None`，保持上游原始字节
   （避免无谓的键序重排与字节漂移）。
4. 失败安全：非目标事件、非完整 JSON（分片）、非 UTF-8 record 一律原样透传；
   跨 chunk 的 record 缓冲到边界到达后再处理，尾部未闭合字节照常发出。
5. 流式改写只在 Grok 路径上包装字节流；非流式在 `decode_non_stream` 之前改写
   `output[]` 里 `function_call` 的 `arguments`。

## expected affected files

- `src-tauri/src/endpoint_executor/grok_arguments.rs`（新增）
- `src-tauri/src/endpoint_executor/mod.rs`
- `docs/changes/grok-integer-tool-arguments/{plan.md,execution.md}`

## verification commands

```bash
cd src-tauri && cargo test grok_arguments
cd src-tauri && cargo test endpoint_executor
cd src-tauri && cargo test
./node_modules/.bin/tauri build --no-bundle
```

## acceptance criteria

- 含浮点整数参数的 Grok 响应到达下游时已是整数字面量。
- 参数里本就没有浮点时，响应字节与上游完全一致（不重排、不改写）。
- 非 Grok provider 的响应路径零变化。
- 现有测试无回归。

## unresolved questions

- JSON 对象键序在**确实需要改写**时会变成 serde_json 的排序结果（默认 BTreeMap）。
  对客户端无影响（按键取值），如需保持原序需引入 `preserve_order`（indexmap），
  本次未做。
