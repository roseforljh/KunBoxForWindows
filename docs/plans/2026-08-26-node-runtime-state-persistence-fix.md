# 节点运行态污染修复计划

## 背景

节点配置文件同时承载了 sing-box 出站配置和 KunBox 界面运行状态。测速期间，前端会给节点视图加入 `isTesting`、`latencyMs`、`latencyStatus`、`healthStatus`、`isTimeout`。节点编辑器完整复制这个对象，Rust 的 `SingBoxOutbound.extra` 又会接收所有未知字段，最终 `node_update` 把整个对象写回配置文件。

本机活动配置已经验证存在：

```json
{
  "tag": "🇬🇧 VL | UK",
  "isTesting": true
}
```

应用重启后，`node_list` 原样返回该字段，节点卡片根据 `node.isTesting` 持续显示转圈。

## 目标

1. 节点配置文件永远不保存界面运行状态。
2. 已经被污染的所有现有配置在升级后自动清理。
3. 即使历史迁移写盘失败，本次运行也不能继续显示假转圈。
4. 所有现有和未来的节点保存入口统一受后端保护。
5. 保留 sing-box 协议扩展字段以及 KunBox 的长期策略字段。
6. 不新增依赖，不引入新的节点实体，不改变订阅和测速协议。

## 不在范围内

1. 不重写 Zustand 节点仓库。
2. 不改变延迟缓存的持久化策略。
3. 不改变测速并发、超时和取消逻辑。
4. 不修改用户的订阅内容或节点业务字段。

## 数据边界

### 持久配置

允许写入节点文件：

- sing-box 出站字段，例如 `server`、`uuid`、`tls`、`transport`。
- 协议专用扩展字段。
- KunBox 长期策略字段，例如 `x_kunbox_auto_selection_eligible`、`x_kunbox_metered_protected`、`detour`。

### 临时运行状态

只允许存在于界面或运行内存：

```text
isTesting
latencyMs
latencyStatus
healthStatus
isTimeout
sourceProfileId
sourceProfileName
```

### 新数据流

```text
配置文件
   │
   ▼
后端读取清洗 ──────────────► 前端节点视图
   │                            │
   │                            ├─ 临时加入测速和健康状态
   │                            │
   │                            └─ 编辑器只提取持久配置
   │                                         │
   └──────── 后端保存清洗 ◄──────────────────┘
                    │
                    ▼
               干净配置文件
```

## 方案

### 1. 建立唯一的运行态字段表

在 Rust 类型层定义 `NODE_RUNTIME_META_KEYS`，集中列出七个临时字段，并给 `SingBoxOutbound` 增加清洗方法。方法返回是否发生修改，供迁移判断是否需要写盘。

生成 sing-box 配置时复用同一字段表，避免前后维护两份名字不同步。

### 2. 后端读取边界清洗

`load_profile_nodes` 反序列化后立即清除运行态字段，再返回节点。

作用：

- 旧配置即使仍有 `isTesting: true`，本次运行也不会显示转圈。
- 迁移因磁盘权限失败时，界面仍保持正确。
- `node_list`、`node_list_all`、节点编辑、节点导出等读取方统一得到干净配置。

`load_profile_nodes_raw` 只服务临时 sing-box 测速配置。生成运行配置已有元数据清洗，保持现有职责，不额外引入写盘行为。

### 3. 后端保存边界清洗

`save_profile_nodes` 在序列化前复制节点列表并统一清除运行态字段。

这是最终保护边界。任何前端调用方或未来新增入口传入临时字段，磁盘文件仍保持干净。

### 4. 启动迁移历史数据

新增同步迁移函数：

1. 读取 `profiles.json` 中登记的所有配置。
2. 逐个读取节点文件。
3. 清除运行态字段。
4. 仅在确实发现脏字段时重新写入。
5. 单个配置读取或写入失败时记录警告，继续处理其他配置。
6. 返回已清理的配置数，用启动日志记录结果。

迁移在 Tauri `setup` 创建 `AppState` 后、窗口显示前执行。用户首次启动新版本时即可清掉历史污染。

### 5. 前端编辑器隔离

在现有 `node-editor.ts` 增加纯函数，把节点视图转换为可编辑配置：

- 深复制原配置。
- 删除七个运行态字段。
- 保留协议扩展字段和长期策略字段。

`NodeDetailModal` 使用该函数创建草稿。编辑器从源头不再携带运行态字段。

## 文件改动

1. `src-tauri/src/types.rs`
   - 增加统一运行态字段常量。
   - 增加 `SingBoxOutbound` 清洗方法。
2. `src-tauri/src/commands/profiles/catalog.rs`
   - 读取后清洗。
   - 保存前清洗。
   - 增加启动迁移函数。
   - 增加迁移和保存边界测试。
3. `src-tauri/src/commands/singbox.rs`
   - 生成配置时复用统一字段表。
4. `src-tauri/src/lib.rs`
   - 在窗口显示前执行迁移并记录结果。
5. `kunbox-electron/src/renderer/components/ui/node-editor.ts`
   - 增加节点视图到持久配置的转换函数。
6. `kunbox-electron/src/renderer/components/ui/NodeDetailModal.tsx`
   - 使用转换函数创建编辑草稿。
7. `kunbox-electron/src/renderer/components/ui/node-editor.test.ts`
   - 检查运行态字段被移除，业务字段被保留。

共修改 7 个代码文件，不新增类、服务或依赖。

## 错误处理

1. 迁移无法解析某个配置时不覆盖原文件，记录文件路径和错误。
2. 迁移无法写入某个配置时不影响应用启动，读取边界仍会在内存中清洗。
3. 保存节点时序列化或写盘失败继续返回错误，保持现有调用语义。
4. 清洗只删除明确列出的 KunBox 临时字段，不使用协议字段白名单，避免误删新协议参数。

## 回归检查

### Rust

1. 构造包含全部运行态字段、协议扩展字段和长期策略字段的节点。
2. 验证清洗只删除运行态字段。
3. 写入被污染的配置文件，验证 `load_profile_nodes` 返回的内存对象已经清理。
4. 执行迁移，验证磁盘文件已清理。
5. 验证协议扩展字段和长期策略字段完整保留。
6. 再次执行迁移，验证没有重复改写。
7. 同时准备一个损坏配置和一个正常脏配置，验证损坏配置保持原样，其他配置继续完成迁移。

### TypeScript

1. 构造包含运行态字段的节点视图。
2. 转换为编辑草稿。
3. 验证七个运行态字段不存在。
4. 验证节点地址、UUID、TLS、自定义协议字段和长期策略字段不变。

## 必要验证

只运行与改动直接相关的检查：

```powershell
cd src-tauri
cargo test runtime_metadata

cd ..\kunbox-electron
node --experimental-strip-types src/renderer/components/ui/node-editor.test.ts
npm run typecheck
```

如果前端依赖尚未安装，记录 `typecheck` 的环境阻塞；Rust 回归测试和 Node 纯函数检查仍必须完成。

## 验收标准

1. 活动配置中的 UK 节点升级后不再转圈。
2. 所有登记配置里的七个运行态字段被自动清除。
3. 新保存的节点文件无法再次出现这些字段。
4. 节点编辑器提交内容不包含这些字段。
5. sing-box 生成配置继续排除 KunBox 元数据。
6. 协议扩展字段、前置代理和长期策略字段无损。
7. 针对根因的 Rust 和 TypeScript 检查通过。

## 实施顺序

1. 增加统一字段表和清洗方法。
2. 接入读取、保存和 sing-box 生成边界。
3. 增加启动迁移。
4. 接入前端编辑器转换。
5. 补回归检查。
6. 运行必要验证。
7. 对照本计划逐项复核差异。

## 已有能力复用

1. `SingBoxOutbound.extra` 继续承载不同协议的扩展字段，不新建协议字段白名单。
2. `save_profile_nodes` 是节点写盘的公共入口，保存清洗直接放在这里。
3. `load_profile_nodes` 是业务读取的公共入口，内存清洗直接放在这里。
4. `process_node` 已经在生成 sing-box 配置时删除同一批临时字段，本次提取公共字段表继续复用。
5. Zustand 的 `latencyCache` 已经负责持久化测速结果，本次保持不变。
6. `node-editor.test.ts` 和 Rust 内联测试沿用现有轻量检查方式。

## 测试路径图

```text
代码路径                                               用户路径
[+] SingBoxOutbound::strip_runtime_metadata()          [+] 重启后打开节点页
  ├─ [计划测试] 含脏字段 -> 删除并返回 changed=true      ├─ [计划测试] 旧 isTesting=true 不再显示转圈
  └─ [计划测试] 无脏字段 -> 保留并返回 changed=false     └─ [计划测试] 正常节点字段保持完整

[+] load_profile_nodes()                               [+] 编辑测速过的节点
  ├─ [计划测试] 合法脏 JSON -> 返回干净节点              ├─ [计划测试] 编辑草稿不含运行态
  └─ [沿用行为] 非法 JSON -> 返回空列表                  └─ [计划测试] 保存后磁盘不含运行态

[+] save_profile_nodes()
  ├─ [计划测试] 输入含运行态 -> 磁盘干净
  └─ [计划测试] 协议扩展及长期策略字段 -> 原样保留

[+] migrate_persisted_node_runtime_metadata()
  ├─ [计划测试] 脏配置 -> 清理并计数
  ├─ [计划测试] 干净配置 -> 不重复改写
  └─ [计划测试] 一个损坏配置 -> 保持原样并继续其他配置

[+] toEditableNode()
  ├─ [计划测试] 七个运行态字段 -> 全部删除
  └─ [计划测试] 嵌套配置及未知协议字段 -> 完整保留
```

全部新增分支都有直接检查。该问题属于配置边界回归，单元和文件级集成检查足够，不增加浏览器 E2E。

## 生产失败模式

| 路径 | 可能失败 | 处理方式 | 检查 |
|------|----------|----------|------|
| 读取清洗 | 历史节点包含运行态字段 | 只删除明确字段，内存继续使用 | Rust 测试 |
| 启动迁移 | 某个配置 JSON 已损坏 | 不覆盖原文件，记录警告，继续迁移其他配置 | Rust 测试 |
| 启动迁移 | 某个配置无法写入 | 记录警告并继续启动，读取边界保证界面正确 | 代码分支检查 |
| 保存清洗 | 前端再次传入运行态字段 | 公共保存入口统一清除 | Rust 测试 |
| 前端编辑 | 视图对象带嵌套协议字段 | 深复制后只删除顶层运行态字段 | TypeScript 检查 |
| 重复启动 | 配置已经干净 | 不重新写盘 | Rust 测试 |

没有无测试、无错误处理且静默失败的关键路径。

## 实施任务

- [x] **T1（P1）**：建立 Rust 统一运行态字段表和清洗方法。
- [x] **T2（P1）**：把读取、保存和 sing-box 生成配置接入统一字段表。
- [x] **T3（P1）**：实现启动迁移，单文件失败不能中断其他配置。
- [x] **T4（P1）**：前端编辑器只生成持久配置草稿。
- [x] **T5（P1）**：补读取、保存、迁移和前端转换回归检查。
- [x] **T6（P1）**：运行定向测试、类型检查并逐项核对验收标准。

## 并行化

顺序实施，不做并行拆分。Rust 字段表、持久化边界和迁移彼此依赖；前端改动很小，拆工作区增加合并成本。

## TODO 处理

项目当前没有 `TODOS.md`。本计划没有需要延期的相关工作，不新增 TODO。

## 自审结果

- Step 0 范围检查：范围保持不变，7 个代码文件，0 个新类或服务，未触发复杂度警报。
- 架构审查：0 个未解决问题。读取、保存、启动迁移三层边界职责清楚。
- 代码质量审查：0 个未解决问题。复用现有公共入口，不增加依赖或协议白名单。
- 测试审查：发现 2 个缺口，已经补入计划，分别是读取清洗和损坏配置继续迁移。
- 性能审查：0 个问题。启动迁移线性扫描已登记配置，只改写脏文件。
- 既有能力复用：已写明。
- 不在范围内：已写明。
- 生产失败模式：已写明，0 个关键缺口。
- 并行化：单路线顺序实施。
- Outside voice：当前运行在 Codex 内，按规则跳过嵌套 Codex 调用。
- Lake Score：2/2 个审查建议采用完整方案。

审查结论：通过，可以实施。

## 落地结果

1. Rust 统一字段表和清洗方法已完成。
2. 业务读取、所有节点保存、sing-box 配置生成已接入统一字段表。
3. Tauri 启动迁移已完成，只改写含脏字段的登记配置。
4. 单个损坏配置不会阻断其他配置迁移。
5. 前端节点编辑草稿会删除全部七个运行态字段。
6. Rust 定向测试 3 个通过，TypeScript 纯函数检查通过。
7. 前端生产构建和 TypeScript 类型检查通过。
8. `git diff --check` 通过。

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | 未运行 | 本次为根因明确的缺陷修复，无产品范围变化 |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | 跳过 | 当前已运行在 Codex 内，禁止嵌套复审 |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | CLEAR | 2 个测试缺口已吸收，0 个关键缺口 |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | 未运行 | 不改变视觉和交互设计 |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | 未运行 | 不改变开发接口和构建流程 |

**VERDICT:** ENG CLEARED，允许开始实施

NO UNRESOLVED DECISIONS
