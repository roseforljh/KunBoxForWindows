# Health Failover Monitor Implementation Plan

> **For Claude:** Use `${SUPERPOWERS_SKILLS_ROOT}/skills/collaboration/executing-plans/SKILL.md` to implement this plan task-by-task.

**Goal:** 为 KunBox 增加低开销的节点健康探测和故障处理能力，让可切换的 selector 自动切到健康备用节点，固定指定的单节点只告警不擅自切换。

**Architecture:** 后端在 sing-box 连接成功后启动一个常驻 Health Monitor，断开时用 cancellation token 停止。监控分为轻量候选探针、真实路径探针、健康状态机和切换执行器四层；主节点和配置分流走 selector 自动故障切换，规则里显式指定单个 node 的分流只上报失败。前端只接收健康事件并展示状态，不参与判定。

**Tech Stack:** Tauri 2.x, Rust, Tokio, reqwest, sing-box Clash API, React 18, TypeScript, Zustand.

---

## 核心原则

1. 固定单节点永不自动切换。
2. selector 才允许自动切换。
3. Clash API delay 只做轻量心跳，不作为唯一可用性依据。
4. 真实路径探针只测当前路径和少量代表域名，不做高频全量扫描。
5. 失败和恢复都必须连续确认，避免一次抖动触发切换。
6. 备用节点提前维护，故障时直接切，不临时全量找节点。
7. 默认低开销，所有全量操作错峰、限并发、带退避。

## 场景分类

### 允许自动切换

- 主代理 `PROXY` selector。
- 配置分流 selector，当前格式是 `P:<profile-id>`。
- 未来如果新增自动选择策略，也必须以 selector 表达。

### 只告警，不自动切换

- 域名分流中 `outboundMode = node`。
- 规则集中 `outboundMode = node`。
- 任何用户显式绑定到单个节点的规则。

告警文案：

```text
分流节点不可用：<节点名>。该规则绑定了固定节点，KunBox 未自动更换，请手动调整规则或节点。
```

## 探针分层

### 轻量候选探针

用途：提前维护备用池。

方式：

- 调用 sing-box Clash API：`GET /proxies/{node}/delay`。
- 使用 `settings.latencyTestUrl`。
- 候选节点低频错峰扫描。

限制：

- 不能单独判定真实业务可用。
- 不能用于固定节点自动切换。

### 真实路径探针

用途：验证当前实际路由路径是否真的可用。

方式：

- 通过本地代理端口发起 HTTP 请求。
- 对主路径使用 `settings.localPort`。
- 对分流路径使用能命中该规则的代表域名。
- 只测当前正在使用的 selector 或固定节点规则，不扫描所有节点。

代表 URL 来源：

- 默认：`settings.latencyTestUrl`。
- 域名规则：使用规则值拼出 `https://<domain>/` 或配置内置样本。
- 规则集：使用少量内置样本，如 `https://www.gstatic.com/generate_204`、`https://www.cloudflare.com/cdn-cgi/trace`。

### 失败信号

计入失败：

- Clash API delay timeout。
- 真实路径请求 timeout。
- HTTP 502、503、504。
- TLS 握手失败。
- 连续连接失败。

不立即计入失败：

- 单次 DNS 抖动。
- 单个测试 URL 403。
- 网络从断开到恢复的短暂窗口。

## 状态机

节点状态：

```rust
enum HealthStatus {
    Unknown,
    Healthy,
    Suspect,
    Failed,
    Recovering,
}
```

状态字段：

```rust
struct NodeHealth {
    tag: String,
    status: HealthStatus,
    last_latency_ms: Option<u32>,
    success_streak: u8,
    failure_streak: u8,
    last_checked_at: i64,
    next_probe_after: i64,
    cooldown_until: Option<i64>,
    last_error: Option<String>,
}
```

判定规则：

- `success_streak >= 2` 进入 `Healthy`。
- `failure_streak == 1` 进入 `Suspect`。
- `failure_streak >= 3` 进入 `Failed`。
- `Failed` 后探测间隔指数退避：30s、60s、120s，最大 5min。
- 切换后 selector 冷却 60s，冷却期不再次自动切换。

## 备用节点池

每个 selector 维护一个备用池：

```rust
struct SelectorHealth {
    selector_tag: String,
    current_node: Option<String>,
    backup_nodes: Vec<String>,
    last_switch_at: Option<i64>,
    switch_cooldown_until: Option<i64>,
}
```

备用池构建：

- 启动后 3 秒开始预热。
- 每轮最多探测 3 个候选节点。
- 并发限制 2 到 4。
- 按健康状态、延迟、最近成功时间排序。
- 保留前 3 个备用节点。

## 切换策略

### 主节点 `PROXY`

切换条件：

- `PROXY` 当前节点连续真实路径失败 3 次。
- 至少存在一个 `Healthy` 备用节点。
- 不在冷却期。
- 主节点故障自动切换设置开启。

执行：

- 优先调用现有 `singbox_switch_node`。
- 如果目标节点与当前节点的 bootstrap signature 不一致，不自动重启；改为告警，避免后台重启造成明显中断。
- 如果 signature 一致，通过 Clash API 热切。

事件：

```json
{
  "kind": "selector_failed_over",
  "selector": "PROXY",
  "from": "old-node",
  "to": "backup-node",
  "reason": "current node failed 3 probes"
}
```

### 配置分流 `P:<profile-id>`

切换条件：

- 当前 selector 节点连续失败 3 次。
- 有健康备用节点。
- 不在冷却期。

执行：

- 使用 Clash API：`PUT /proxies/{selector}`。
- 不改用户配置文件。
- 不重启 sing-box。

事件：

```json
{
  "kind": "selector_failed_over",
  "selector": "P:profile-a",
  "from": "node-a",
  "to": "node-b",
  "reason": "profile selector node failed"
}
```

### 固定单节点规则

切换条件：

- 无。

执行：

- 不切换。
- 不改规则。
- 发告警事件。

事件：

```json
{
  "kind": "fixed_node_failed",
  "node": "fixed-node",
  "rule": "example.com",
  "message": "分流节点不可用：fixed-node。该规则绑定了固定节点，KunBox 未自动更换，请手动调整规则或节点。"
}
```

## 任务拆分

### Task 1: 增加健康监控类型和设置

**Files:**

- Modify: `src-tauri/src/types.rs`
- Modify: `kunbox-electron/src/shared/types.ts`

**Step 1: 写 Rust 类型**

在 `src-tauri/src/types.rs` 添加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Suspect,
    Failed,
    Recovering,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthEventKind {
    SelectorFailedOver,
    SelectorNoBackup,
    FixedNodeFailed,
    MainNodeNeedsManualSwitch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthEvent {
    pub kind: HealthEventKind,
    pub selector: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub node: Option<String>,
    pub rule: Option<String>,
    pub message: String,
}
```

给 `AppSettings` 增加字段：

```rust
#[serde(rename = "healthMonitorEnabled", default = "default_health_monitor_enabled")]
pub health_monitor_enabled: bool,
#[serde(rename = "mainNodeAutoFailover", default)]
pub main_node_auto_failover: bool,
#[serde(rename = "healthProbeIntervalSec", default = "default_health_probe_interval_sec")]
pub health_probe_interval_sec: u64,
```

默认值：

```rust
fn default_health_monitor_enabled() -> bool { true }
fn default_health_probe_interval_sec() -> u64 { 15 }
```

**Step 2: 写 TS 类型**

在 `kunbox-electron/src/shared/types.ts` 添加同名字段和事件类型。

**Step 3: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test types
```

Expected: 编译通过。

### Task 2: 增加 Health Monitor 状态容器

**Files:**

- Modify: `src-tauri/src/state.rs`

**Step 1: 增加状态字段**

在 `AppState` 中添加：

```rust
pub health_cancel: Arc<Mutex<Option<CancellationToken>>>,
pub node_health: Arc<Mutex<std::collections::HashMap<String, NodeHealth>>>,
```

如果 `NodeHealth` 放在 `singbox.rs` 内部，则只保留取消 token，健康快照先不跨模块暴露。

**Step 2: 初始化字段**

在 `AppState::new` 中初始化。

**Step 3: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test
```

Expected: 全部通过。

### Task 3: 抽取 selector 和固定节点监控目标

**Files:**

- Modify: `src-tauri/src/commands/singbox.rs`

**Step 1: 写失败测试**

在 `mod tests` 中新增：

```rust
#[tokio::test]
async fn collect_health_targets_separates_selectors_and_fixed_nodes() {
    // 构造 ruleset/profile/domain rule：
    // 1. profile 模式生成 P:profile-b selector
    // 2. node 模式生成 fixed target
    // 3. PROXY 总是 selector target
    // 断言 fixed node target 的 auto_failover=false
}
```

**Step 2: 实现目标类型**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum HealthTargetKind {
    Selector,
    FixedNode,
}

#[derive(Debug, Clone)]
struct HealthTarget {
    kind: HealthTargetKind,
    selector_tag: Option<String>,
    node_tag: Option<String>,
    rule_label: Option<String>,
    auto_failover: bool,
}
```

**Step 3: 实现收集函数**

```rust
async fn collect_health_targets(state: &AppState) -> Vec<HealthTarget>
```

收集规则：

- `PROXY` 作为 selector target。
- `collect_referenced_profile_selector_tags` 返回的 `P:<profile-id>` 作为 selector target。
- `custom_rules.domain_rules` 中 `outbound_mode == "node"` 作为 fixed node target。
- `rulesets` 中 `outbound_mode == "node" || "节点"` 作为 fixed node target。

**Step 4: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test collect_health_targets_separates_selectors_and_fixed_nodes
```

Expected: 新测试通过。

### Task 4: 实现低开销探针引擎

**Files:**

- Modify: `src-tauri/src/commands/singbox.rs`

**Step 1: 写状态机单测**

新增测试：

```rust
#[test]
fn health_state_fails_after_three_consecutive_failures() {}

#[test]
fn health_state_recovers_after_two_consecutive_successes() {}

#[test]
fn failed_node_uses_backoff_before_next_probe() {}
```

**Step 2: 实现 `NodeHealth`**

```rust
#[derive(Debug, Clone)]
struct NodeHealth {
    tag: String,
    status: HealthStatus,
    last_latency_ms: Option<u32>,
    success_streak: u8,
    failure_streak: u8,
    last_checked_at: i64,
    next_probe_after: i64,
    cooldown_until: Option<i64>,
    last_error: Option<String>,
}
```

**Step 3: 实现状态更新函数**

```rust
fn record_probe_success(health: &mut NodeHealth, latency_ms: u32, now_ms: i64)
fn record_probe_failure(health: &mut NodeHealth, error: String, now_ms: i64)
fn should_probe(health: &NodeHealth, now_ms: i64) -> bool
```

**Step 4: 实现轻量探针**

复用现有：

```rust
probe_selector_node_latency(client, clash_api_port, tag, test_url)
```

调整为可传 timeout，避免写死 5000ms。

**Step 5: 实现真实路径探针**

```rust
async fn probe_real_proxy_path(local_port: u16, url: &str, timeout_ms: u64) -> Result<u32, String>
```

要求：

- 使用 `reqwest::Proxy::all(format!("http://127.0.0.1:{local_port}"))`。
- timeout 默认 5 秒。
- 2xx、3xx、204 算成功。
- 502、503、504 算失败。

**Step 6: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test health_state
```

Expected: 新状态机测试通过。

### Task 5: 实现 selector 备用池和切换决策

**Files:**

- Modify: `src-tauri/src/commands/singbox.rs`

**Step 1: 写决策单测**

新增测试：

```rust
#[test]
fn selector_failover_selects_lowest_latency_healthy_backup() {}

#[test]
fn selector_failover_does_not_switch_during_cooldown() {}

#[test]
fn fixed_node_failure_never_returns_switch_action() {}
```

**Step 2: 定义动作**

```rust
enum HealthAction {
    None,
    SwitchSelector { selector: String, from: String, to: String },
    NotifyFixedNodeFailed { node: String, rule: Option<String> },
    NotifyNoBackup { selector: String },
}
```

**Step 3: 实现决策函数**

```rust
fn decide_health_action(
    target: &HealthTarget,
    selector: Option<&SelectorHealth>,
    node_health: &std::collections::HashMap<String, NodeHealth>,
    now_ms: i64,
) -> HealthAction
```

规则：

- `FixedNode` 永远不返回 `SwitchSelector`。
- `Selector` 当前节点 failed 且有 healthy backup 时返回切换。
- 没有 backup 时返回 `NotifyNoBackup`。

**Step 4: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test selector_failover fixed_node_failure
```

Expected: 新测试通过。

### Task 6: 接入 sing-box 生命周期

**Files:**

- Modify: `src-tauri/src/commands/singbox.rs`
- Modify: `src-tauri/src/state.rs`

**Step 1: 启动监控任务**

在 `singbox_start_impl` 连接成功、traffic polling 启动附近加入：

```rust
if effective_settings.health_monitor_enabled {
    start_health_monitor(app.clone(), state, effective_settings.clone()).await;
}
```

**Step 2: 停止监控任务**

在 `singbox_stop_impl` 开始处取消：

```rust
if let Some(cancel) = state.health_cancel.lock().await.take() {
    cancel.cancel();
}
```

崩溃路径也取消。

**Step 3: 实现监控循环**

```rust
async fn start_health_monitor(app: AppHandle, state: &AppState, settings: AppSettings)
async fn run_health_monitor(app: AppHandle, state: AppState, cancel: CancellationToken, settings: AppSettings)
```

循环规则：

- 初次延迟 3 秒。
- 每 15 秒检查当前节点。
- 每轮最多探测 3 个备用候选。
- selector 切换后 60 秒冷却。
- 连接状态不是 `Connected` 时退出。

**Step 4: 执行动作**

- `SwitchSelector` 调用 Clash API `PUT /proxies/{selector}`。
- `PROXY` 如果 `main_node_auto_failover == true` 才切。
- `PROXY` 切换前检查 bootstrap signature，一致才热切，不一致发 `MainNodeNeedsManualSwitch`。
- `FixedNodeFailed` 只发事件。

**Step 5: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test commands::singbox::tests::
```

Expected: singbox 模块测试全部通过。

### Task 7: 添加前端事件和提示

**Files:**

- Modify: `kunbox-electron/src/shared/types.ts`
- Modify: `kunbox-electron/src/shared/tauri-api.ts`
- Modify: `kunbox-electron/src/renderer/components/Dashboard.tsx`
- Modify: `kunbox-electron/src/renderer/stores/nodesStore.ts`

**Step 1: 添加 TS 类型**

```ts
export type HealthEventKind =
  | 'selector_failed_over'
  | 'selector_no_backup'
  | 'fixed_node_failed'
  | 'main_node_needs_manual_switch'

export interface HealthEvent {
  kind: HealthEventKind
  selector?: string | null
  from?: string | null
  to?: string | null
  node?: string | null
  rule?: string | null
  message: string
}
```

**Step 2: 添加 API listen**

在 `kunbox-electron/src/shared/tauri-api.ts` 添加：

```ts
onHealthEvent: (callback: (data: HealthEvent) => void) => {
  const unlisten = listen<HealthEvent>('singbox:health', (event) => callback(event.payload))
  return bindUnlisten(unlisten)
}
```

**Step 3: Dashboard 接事件**

在 `Dashboard.tsx` 中监听：

- `selector_failed_over`：toast success。
- `fixed_node_failed`：toast error。
- `selector_no_backup`：toast warning。
- `main_node_needs_manual_switch`：toast warning。

**Step 4: 更新节点状态**

在 `nodesStore.ts` 添加健康状态字段，前端只展示，不参与切换判断。

**Step 5: 跑前端类型检查**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\kunbox-electron
npm run typecheck
```

Expected: 通过。

### Task 8: 增加设置项

**Files:**

- Modify: `kunbox-electron/src/renderer/components/Settings.tsx`
- Modify: `src-tauri/src/commands/settings.rs`

**Step 1: 设置页添加开关**

放到代理或系统设置区域：

- `健康监控`
- `主节点故障自动切换`

默认：

- 健康监控：开。
- 主节点故障自动切换：关。

说明：

```text
固定节点分流失败时只提示，不会自动更换。
```

**Step 2: 后端设置持久化**

确保 `set_settings` 能读取新增字段。

**Step 3: 跑测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test commands::settings::tests::
```

Expected: settings 测试通过。

### Task 9: 集成验证

**Files:**

- No file changes.

**Step 1: 后端全量测试**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\src-tauri
cargo test
```

Expected: 全部通过。

**Step 2: 前端类型检查**

Run:

```powershell
cd C:\Users\33039\Desktop\KunBoxForWindows\kunbox-electron
npm run typecheck
```

Expected: 通过。

**Step 3: 手工验证固定节点**

流程：

1. 新增域名分流规则，outbound 选择固定 node。
2. 启动代理。
3. 让该节点失效。
4. 观察 UI 只提示固定节点不可用。
5. 确认该规则没有被改成其它节点。

Expected:

- 无自动切换。
- 有 `fixed_node_failed` 事件。

**Step 4: 手工验证配置分流 selector**

流程：

1. 新增域名分流规则，outbound 选择 profile。
2. 让当前 selector 节点失效。
3. 保留一个健康备用节点。

Expected:

- 后端发 `selector_failed_over`。
- Clash API 当前 selector 切到备用节点。
- 不重启 sing-box。

**Step 5: 手工验证主节点**

流程：

1. 开启 `主节点故障自动切换`。
2. 主节点失效。
3. 备用节点健康且 bootstrap signature 一致。

Expected:

- `PROXY` selector 自动切换。
- UI 提示主节点已自动切换。

## 风险和规避

### 风险：探针造成额外流量

规避：

- 默认 15 秒检查当前路径。
- 候选节点每轮最多 3 个。
- 失败节点指数退避。
- 不做全量秒级扫描。

### 风险：误判导致频繁切换

规避：

- 连续失败 3 次才失败。
- 连续成功 2 次才恢复。
- 切换冷却 60 秒。

### 风险：主节点切换需要重建配置

规避：

- 自动切换只允许 bootstrap signature 一致的节点。
- 不一致时提示手动切换。

### 风险：固定节点被擅自替换

规避：

- 固定节点 target 的 `auto_failover=false`。
- 决策函数单测保证 fixed node 永不返回切换动作。

## 交付顺序

1. 状态机和目标收集。
2. 探针引擎。
3. selector 自动切换。
4. 固定节点告警。
5. 前端事件展示。
6. 设置项。
7. 集成验证。

## 提交建议

每个阶段一个提交：

```powershell
git add src-tauri/src/types.rs kunbox-electron/src/shared/types.ts
git commit -m "feat: add health monitor types"

git add src-tauri/src/commands/singbox.rs src-tauri/src/state.rs
git commit -m "feat: add node health monitor"

git add kunbox-electron/src/shared/tauri-api.ts kunbox-electron/src/renderer/components/Dashboard.tsx kunbox-electron/src/renderer/stores/nodesStore.ts
git commit -m "feat: show node health events"

git add kunbox-electron/src/renderer/components/Settings.tsx src-tauri/src/commands/settings.rs
git commit -m "feat: add health monitor settings"
```
