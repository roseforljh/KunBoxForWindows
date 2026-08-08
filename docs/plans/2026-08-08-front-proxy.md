# 节点编辑与前置代理实施计划

> 状态：功能已实施并完成自动验证。安卓仓库仅用于行为对照。

## 目标

Windows 端节点编辑页对齐安卓端的核心编辑能力，并把前置代理和节点策略保存到节点自身。

链路语义：

```text
应用流量 -> 当前节点 -> 当前节点配置的前置代理 -> 网络
```

每个节点独立决定是否使用前置代理。设置页不保留全局前置代理状态。

## 安卓端对照

对照文件：

1. `C:/Users/33039/Desktop/KunBox/app/src/main/java/com/kunk/singbox/ui/screens/NodeDetailScreen.kt`
2. `C:/Users/33039/Desktop/KunBox/app/src/main/java/com/kunk/singbox/ui/screens/NodeProtocolFields.kt`
3. `C:/Users/33039/Desktop/KunBox/app/src/main/java/com/kunk/singbox/repository/ConfigRepository.kt`

Windows 端编辑页需要覆盖：

1. 节点名称、服务器、端口和协议认证参数。
2. 传输层、TLS、Reality、ECH 和 MUX 参数。
3. 节点级前置代理选择和高级标签输入。
4. 高价计费节点保护。
5. 参与自动探测与切换。
6. 保存、刷新和运行中修改提示。

## 数据模型

节点继续使用现有 sing-box outbound JSON，不新增独立配置实体。

节点级字段：

```json
{
  "detour": "profile-id::node-tag",
  "x_kunbox_metered_protected": false,
  "x_kunbox_auto_selection_eligible": true
}
```

同配置引用允许保存为节点标签，跨配置引用保存为 `profileId::节点标签`。运行配置会把引用转换成最终 outbound 标签。

## 编辑页

`NodeDetailModal.tsx` 负责编辑已存在节点，并复用现有字段组件和节点 JSON 结构。

页面行为：

1. 按协议显示对应参数。
2. 前置代理候选来自所有已启用配置。
3. 当前节点和高价保护节点不进入前置代理候选。
4. 已保存但当前无法解析的引用继续显示，避免静默丢失。
5. 高价保护开启时关闭自动探测选项。
6. 自动探测开启时关闭高价保护选项。
7. 保存调用 `node.update`，成功后刷新节点列表。
8. 已连接时提示重新连接后生效。

## 后端更新

新增 `node_update` 命令，更新活动配置中的目标节点。

保存校验：

1. 节点名称不能为空。
2. 同一配置内节点名称不能重复。
3. 前置代理节点必须存在于已启用配置。
4. 节点不能引用自身。
5. 前置代理链不能形成循环。
6. 高价保护节点不能作为前置代理。
7. 节点改名时同步改写同配置和跨配置引用。
8. 被其他节点引用时，禁止直接把该节点改成高价保护节点。

## 订阅刷新

订阅刷新通过节点身份匹配保留用户策略：

1. 保留 `detour`。
2. 保留高价计费保护。
3. 保留自动探测与切换资格。
4. 节点消失时不把策略迁移到无关节点。

## 运行配置

主配置生成：

1. 高价保护节点不进入普通运行候选。
2. 自动探测只包含允许参与的节点。
3. 当前配置内的前置链保留节点标签。
4. 跨配置依赖自动加入 outbounds，并转换为作用域标签。
5. 多级前置链递归收集。
6. 元数据字段在交给 sing-box 前删除。

临时测速：

1. 递归加载同配置和跨配置前置依赖。
2. 为临时节点分配独立标签并重写 `detour`。
3. 检测循环、失效引用和高价保护依赖。
4. XHTTP 测速沿用同一条节点级前置链。

XHTTP 主运行链路：

1. Xray 插件连接内部回环入口。
2. sing-box 路由把该入口固定送往节点的 `detour`。
3. 内部入口只监听 `127.0.0.1`。
4. 链式节点不生成会绕过前置代理的远端直连规则。

## 文件范围

前端：

1. `kunbox-electron/src/renderer/components/Nodes.tsx`
2. `kunbox-electron/src/renderer/components/Settings.tsx`
3. `kunbox-electron/src/renderer/components/ui/NodeDetailModal.tsx`
4. `kunbox-electron/src/renderer/components/ui/node-editor.ts`
5. `kunbox-electron/src/shared/tauri-api.ts`
6. `kunbox-electron/src/shared/types.ts`

后端：

1. `src-tauri/src/commands/profiles/catalog.rs`
2. `src-tauri/src/commands/profiles/subscription/links.rs`
3. `src-tauri/src/commands/profiles/latency/config.rs`
4. `src-tauri/src/commands/profiles/latency/runtime.rs`
5. `src-tauri/src/commands/singbox.rs`
6. `src-tauri/src/types.rs`

## 自动验证

Rust：

```powershell
cargo check
cargo test --lib
```

前端：

```powershell
npm run typecheck
npm run lint
npm run build
```

附加自检：

```powershell
node node-editor.test.mjs
node manual-node.test.mjs
git diff --check
```

## 验收标准

1. 每种支持协议都能在编辑页读取、修改并保存已有参数。
2. 节点级前置代理能选择同配置或跨配置节点。
3. 自引用、循环引用、失效引用和高价保护依赖会被拒绝。
4. 节点重命名后所有前置引用保持有效。
5. 两个节点策略开关互斥并持久化。
6. 订阅刷新不会覆盖用户保存的节点策略。
7. 普通节点、XHTTP 节点和临时测速均使用节点级前置链。
8. 设置页没有旧的全局前置代理入口。
9. Rust 与前端自动检查全部通过。
10. Release 产物写入 `src-tauri/target/release`。
