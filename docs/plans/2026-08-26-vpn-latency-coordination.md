# VPN 与测速并发协调计划

## 问题结论

用户在离线测速临时 sing-box 运行期间开启 VPN，当前实现会产生生命周期冲突。

现有生命周期锁只保护“启动内核”这一步，没有覆盖测速请求本身。VPN 启动会杀掉临时 sing-box，但没有取消活动测速批次，旧请求会继续返回失败并写入延迟缓存。

当前时序：

```text
测速请求持有临时内核
        │
        ├──────── 用户开启 VPN
        │                  │
        │                  ├─ 启动流程清理临时内核
        │                  └─ 没有取消旧测速请求
        │
        └──────── 旧请求继续返回失败，覆盖缓存
```

## 目标

1. VPN 启动和停止拥有高于测速的生命周期优先级。
2. VPN 操作开始时，活动测速立即收到取消信号。
3. 只有所有测速调用退出后，才清理临时 sing-box。
4. 单节点测速、批量测速、仪表盘自动测速、托盘测速全部归入同一个取消边界。
5. 被取消的旧请求不能写入延迟缓存，也不能把节点标记为失败。
6. VPN 启动、停止、重启和自动连接走同一套后端协调规则。
7. 主内核和临时内核不并行运行，不改变已经连接状态下的主 API 测速。
8. 不新增依赖，不改变用户可见的节点协议和配置格式。

## 不在范围内

1. 不把 `ProxyState` 扩展成包含测速状态的 UI 状态枚举，避免破坏现有前端协议。
2. 不重写测速探测协议、超时参数和并发数量。
3. 不改变延迟缓存的正常成功结果持久化策略。
4. 不增加浏览器 E2E 框架，当前并发边界适合 Rust 和前端 Store 回归检查。
5. 不处理外部代理软件自身的冲突，当前范围只覆盖 KunBox 内部主内核和临时内核。

## 方案总览

采用后端“生命周期闸门 + 取消令牌 + 读写门控”，前端增加同一取消入口和结果代际检查。

```text
                    ┌─────────────────────────────┐
                    │ Latency lifecycle gate       │
                    │ 读：测速请求                 │
                    │ 写：VPN 启动/停止/重启       │
                    └──────────────┬──────────────┘
                                   │
        ┌──────────────────────────┼──────────────────────────┐
        │                          │                          │
   节点批量测速                单节点测速                 VPN 操作
        │                          │                          │
        └────────── CancellationToken / RunId ────────────────┘
                                   │
                       清理临时内核后再启动主内核
```

### 生命周期规则

```text
Idle
 ├─ LatencyRun：允许离线测速
 ├─ Connecting：拒绝新测速，取消旧测速
 ├─ Connected：只走主 Clash API
 ├─ Disconnecting：拒绝新测速，取消旧测速
 └─ Error：允许重新启动或离线测速
```

`ProxyState` 保持不变。生命周期闸门负责跨操作协调，避免把内部并发状态暴露给 UI。

## 后端设计

### 1. AppState 增加测速生命周期闸门

在 `AppState` 增加：

- `latency_gate: Arc<tokio::sync::RwLock<()>>`
- `latency_blocked: Arc<AtomicBool>`

测速命令持有读锁直到请求完整结束。VPN 启停命令先设置 `latency_blocked=true`，取消全局测速令牌，再等待写锁。写锁拿到后，所有旧测速已经退出，且新测速不能进入。

顺序：

```text
VPN 操作
  1. blocked = true
  2. cancel token
  3. 等待读锁全部释放
  4. 获取写锁
  5. 清理临时内核
  6. 执行主内核启停
  7. 释放写锁并 blocked = false
```

使用 RAII 守卫，保证启动或停止任何提前返回时都能解除 blocked，避免应用永久拒绝测速。

### 2. 统一取消入口

抽出后端内部函数 `cancel_latency_tests_and_wait`：

1. 取消当前 `CancellationToken`。
2. 清空当前活动批次 ID，旧 Run 失效。
3. 获取生命周期写锁，等待所有读锁释放。
4. 只有写锁拿到后才允许清理临时 sing-box。

现有 `node_cancel_latency_tests` 复用这条路径。

`node_begin_latency_tests` 先取消并等待旧 Run，再清理旧临时内核，最后创建新 Run。

### 3. VPN 启停统一抢占测速

`singbox_start_impl`、`singbox_stop_impl` 在拿 `lifecycle_lock` 之前先调用后端协调器。

这样避免这个死锁顺序：

```text
错误顺序：VPN 先拿 lifecycle_lock → 等测速退出
          测速拿不到 lifecycle_lock → 等 VPN 释放
```

正确顺序：

```text
VPN 先阻止新测速并取消旧测速 → 等旧测速退出 → 再拿 lifecycle_lock
```

临时内核启动等待 `lifecycle_lock` 时，同时监听取消令牌。VPN 抢占期间排队的旧测速会直接退出，不会排在 VPN 后面重新启动临时内核。

### 4. RunId 与结果代际隔离

批量测速继续使用 `runId`，并强化后端检查：

- 请求开始时必须属于当前 Run。
- 每次返回结果前检查 token 和 RunId。
- 取消、VPN 启停、开启新批次后，旧结果全部丢弃。

单节点测速使用当前取消令牌，同时前端使用 `latencyCancelGeneration`：

```text
开始单节点测速：capture generation = 10
VPN 启动取消：generation = 11
旧请求返回：10 != 11，丢弃结果
```

取消结果不转成 `local_test_failed` 写缓存。

### 5. 临时内核所有权

临时 sing-box 仍由现有温度进程槽位管理，但只能在生命周期写锁内清理。清理函数不再被并发测速调用无条件重置活动状态。

释放流程：

```text
LatencyRun 完成或取消
       ↓
读锁释放
       ↓
VPN/取消命令拿写锁
       ↓
清理 temp sing-box、Xray bridge、临时目录和映射
```

## 前端设计

### 1. 连接操作先取消测速

`connectionStore.connect` 和 `disconnect` 在调用 Tauri 启停命令前，调用 `useNodesStore.getState().cancelTestAllLatency()`。

效果：

- 节点卡片立即停止转圈。
- 批量进度立即归零。
- 已有成功延迟保留。
- 后端随后再次取消，形成兜底。

托盘和自动连接都复用 `connectionStore`，不再单独实现取消。

### 2. 单节点结果不覆盖新状态

`testNodeLatency` 捕获取消代际。请求完成后发现代际变化，直接返回，不更新节点和缓存。

批量测速继续使用已有 `AbortController + runId`，取消动作统一调用后端 `run_id=None`，确保单节点和批量都被取消。

### 3. 连接后的自动测速

VPN 成功返回并释放生命周期写锁后，现有 Dashboard 自动测速继续执行。此时后端状态为 `Connected`，选择主 Clash API，不启动临时内核。

## 代码改动范围

1. `src-tauri/src/state.rs`
   - 增加测速读写闸门和阻断标记。
2. `src-tauri/src/types.rs` and `kunbox-electron/src/shared/types.ts`
   - 增加明确的 `cancelled` 测速结果状态，取消不再伪装成节点故障。
3. `src-tauri/src/commands/profiles/latency.rs`
   - 增加生命周期守卫、取消等待、读锁接入。
   - 统一单节点、批量和取消命令。
4. `src-tauri/src/commands/profiles/latency/runtime.rs`
   - 临时内核等待生命周期锁时响应取消。
5. `src-tauri/src/commands/profiles/latency/tests.rs`
   - 增加 VPN 抢占测速、取消等待、旧 Run 隔离测试。
6. `src-tauri/src/commands/singbox.rs`
   - 启停命令接入协调器，确保先取消测速再操作内核。
7. `kunbox-electron/src/renderer/stores/nodesStore.ts`
   - 增加取消代际，取消动作覆盖单节点和批量。
8. `kunbox-electron/src/renderer/stores/connectionStore.ts`
   - 连接和断开前统一触发测速取消。
9. `kunbox-electron/src/renderer/stores/nodesStore.concurrent.test.ts`
   - 新增前端旧结果丢弃检查。

共 10 个代码文件，不新增依赖，不新增独立服务。

## 错误处理

1. VPN 启动前取消失败时，后端仍通过生命周期写锁阻止新测速。
2. 临时探测超时会被取消令牌打断，读锁随后释放。
3. VPN 启动任意错误路径通过 RAII 守卫解除测速阻断。
4. 旧测速结果只被丢弃，不写入失败缓存。
5. 新测速在 `Connecting` 或 `Disconnecting` 阶段返回可识别的取消结果，前端不显示节点故障。
6. 清理临时进程失败只记录日志，主 VPN 启动错误按现有返回语义处理。

## 测试计划

### Rust 单元测试

1. `cancel_latency_tests_and_wait` 会取消令牌并等待已有读锁释放。
2. VPN 写锁拿到后，新测速请求被阻断。
3. `start_temp_singbox` 在取消令牌触发时不会继续等待生命周期锁。
4. Connecting 和 Disconnecting 阶段不会启动临时内核。
5. 旧 Run 返回结果时，后端不接受结果。
6. 新 Run 可以在旧 Run 取消完成后正常开始。
7. 单节点和批量取消都能清掉当前活动批次。
8. VPN 启动失败时，生命周期闸门最终恢复可用。

### 前端纯检查

1. 连接前取消测速会清除 `isTesting` 和进度。
2. 取消代际变化后，旧单节点结果不会更新节点或缓存。
3. 旧批次返回结果时，`runId` 不匹配不会更新节点。
4. 成功延迟在取消后保持不变。

### 用户路径

```text
代码路径                                      用户路径
[+] 批量测速 + VPN 启动                        [+] 节点页点击测速，再点击 VPN
  ├─ cancel token                              ├─ [★★★] 转圈立即结束
  ├─ 读锁释放                                  ├─ [★★★] 旧延迟不被失败覆盖
  └─ 写锁启动 VPN                              └─ [★★★] VPN 正常连接

[+] 单节点测速 + 托盘启动                      [+] 节点测速中点托盘“启动 VPN”
  ├─ generation fencing                        ├─ [★★★] 旧结果不回写
  └─ main API after connected                  └─ [★★★] 连接后测速走主内核

[+] VPN 启动失败                               [+] 内核启动失败后再次点击测速
  ├─ RAII unblock                              ├─ [★★★] UI 不永久卡住
  └─ state error                               └─ [★★★] 可以再次测速或重试
```

不增加 E2E，测试对象都是纯状态和后端并发边界。

## 生产失败模式

| 路径 | 失败方式 | 测试 | 处理 | 用户结果 |
|------|----------|------|------|----------|
| VPN 抢占 | 旧请求卡在临时 API | 取消等待测试 | token + 写锁 | VPN 等待后正常启动 |
| 新测速进入 | transition 期间绕过取消 | 写锁阻断测试 | blocked + RwLock | 不启动第二个临时内核 |
| 旧结果返回 | 晚到结果覆盖新延迟 | RunId/generation 测试 | 直接丢弃 | 原延迟不变 |
| 启动失败 | 闸门永久阻断 | RAII 测试 | 守卫 Drop 解锁 | 可重新测速 |
| 停止 VPN | 主 API 探测被中断 | stop 抢占测试 | 统一取消 | 不写失败缓存 |
| 单节点测速 | 独立 token 未取消 | 前端取消代际测试 | 统一取消入口 | 不显示假故障 |

没有无测试、无错误处理且静默失败的关键路径。

## 既有能力复用

1. 复用现有 `lifecycle_lock`，它继续负责进程启动顺序。
2. 复用现有 `CancellationToken`、`runId` 和 `AbortController`。
3. 复用现有 `cleanup_temp_singbox`、临时进程槽位和映射表。
4. 复用 `connectionStore` 作为所有前端 VPN 入口的统一调用点。
5. 复用已有延迟后端选择规则，Connected 后仍优先主 API。

## 并行化

顺序实施，没有并行化机会。后端闸门、取消语义和前端取消入口互相依赖，拆开会产生竞态和合并冲突。

## 实施顺序

1. 增加 AppState 闸门和后端生命周期守卫。
2. 接入测速命令读锁、取消等待和 RunId 检查。
3. 接入临时内核取消等待和 VPN 启停抢占。
4. 接入前端连接前取消和结果代际检查。
5. 补 Rust 和前端并发回归检查。
6. 运行定向测试、类型检查和前端构建。
7. 对照验收项检查未提交改动边界。

## 验收标准

1. 批量测速期间启动 VPN，临时内核被安全清理，VPN 正常连接。
2. 单节点测速期间启动、停止、重启 VPN，旧请求不覆盖延迟缓存。
3. VPN 连接期间不启动新的临时测速内核。
4. 取消后已有成功延迟保持不变。
5. VPN 启动失败后，测速功能仍可再次使用。
6. 仪表盘、托盘、自动连接行为一致。
7. 全部新增 Rust 和前端回归检查通过。

## 自审记录

- Step 0：范围接受原方案，10 个代码文件，0 个新依赖，0 个独立服务。
- 架构审查：发现 1 个竞态缺口，已补充“阻断标记 + 读锁后二次检查”，防止 VPN 抢占窗口放进新测速。
- 代码质量审查：发现 1 个清理缺口，已补充批量测速临时内核清理守卫，覆盖取消、异常和提前返回。
- 测试审查：新增 2 个必须覆盖的场景，分别是闸门抢占窗口和批量提前返回后的临时进程清理。
- 性能审查：无阻塞问题。VPN 等待时间由取消令牌缩短，正常情况不增加测速路径网络开销。
- 既有能力复用：已写明，未引入并行工作区。
- 生产失败模式：0 个无测试、无错误处理且静默失败的关键缺口。
- Outside voice：当前运行在 Codex 内，跳过嵌套 Codex 复审。
- 审查结论：通过，可以实施。

## What already exists

1. `lifecycle_lock` 已保护主内核和临时内核的启动顺序，本计划只补全请求生命周期。
2. 现有 CancellationToken、批次 RunId 和前端 AbortController 直接复用。
3. `cleanup_temp_singbox` 已覆盖临时 sing-box、Xray bridge、临时目录和映射表。
4. `connectionStore` 已被仪表盘、托盘和自动连接共同使用，取消接入后无需复制入口逻辑。

## NOT in scope

1. 不重写 ProxyState，原因是新增内部协调状态不应扩大前后端协议。
2. 不增加浏览器 E2E，原因是问题边界是 Rust 并发与 Store 代际检查。
3. 不改变测速协议和超时策略，原因是本次根因是生命周期抢占，不是探测算法。
4. 不处理第三方代理软件冲突，原因是当前证据只指向 KunBox 内部临时内核。

## Test coverage diagram

```text
CODE PATHS                                      USER FLOWS
[+] lifecycle gate                              [+] 批量测速中启动 VPN
  ├─ blocked=true                                ├─ [★★★] 转圈立即结束
  ├─ cancel token                                ├─ [★★★] 延迟缓存不被失败覆盖
  ├─ await readers                               └─ [★★★] VPN 正常连接
  └─ RAII unblock                                

[+] node_test_latency / node_test_all           [+] 单节点测速中托盘启动 VPN
  ├─ read lock                                   ├─ [★★★] 旧请求结果被丢弃
  ├─ blocked recheck                             └─ [★★★] 连接后走主 API
  ├─ cancelled result                            
  └─ release read lock                           [+] VPN 启动失败
                                                   ├─ [★★★] 闸门恢复
[+] singbox_start/stop                           └─ [★★★] 可以再次测速
  ├─ cancel + wait
  ├─ lifecycle lock                               [+] 用户取消批量测速
  ├─ temp cleanup guard                           ├─ [★★★] 临时进程退出
  └─ main process                                 └─ [★★★] 旧结果不写缓存
```

计划新增测试覆盖全部新增分支。全部为单元或 Store 纯检查，不需要 E2E。

## Failure mode review

| 失败模式 | 覆盖 | 处理 | 用户结果 |
|---|---|---|---|
| 新请求在 VPN 抢占窗口进入 | 闸门二次检查测试 | blocked + RwLock | 不会启动第二套测速内核 |
| 旧请求等待生命周期锁 | 取消等待测试 | token select | 快速退出 |
| 批量中途取消 | 清理守卫测试 | finally/Drop 清理 | 无残留临时进程 |
| 启动失败后闸门不恢复 | RAII 测试 | 守卫 Drop | 可再次测速 |
| 旧结果晚到 | generation/RunId 测试 | 丢弃结果 | 延迟不被污染 |

## Implementation tasks

- [x] **T1（P1）**：增加 AppState 测速生命周期闸门和阻断标记。
- [x] **T2（P1）**：把单节点、批量、开始和取消命令接入闸门与统一取消等待。
- [x] **T3（P1）**：让临时内核启动和批量路径响应取消并保证清理。
- [x] **T4（P1）**：让 VPN 启停先取消测速，再操作主内核。
- [x] **T5（P1）**：让前端连接入口取消测速并丢弃旧代际结果。
- [x] **T6（P1）**：补 Rust/TypeScript 回归检查并完成定向验证。

## 落地结果

1. AppState 已增加测速读写闸门和生命周期阻断标记。
2. 单节点、批量、开始、取消命令已接入统一闸门。
3. VPN 启动和停止已先取消测速，再获取主生命周期锁。
4. 临时内核等待生命周期锁时会响应取消令牌。
5. 取消结果新增 `cancelled` 状态，不再写入失败延迟。
6. 前端连接和断开会立即清除测速 UI，并用代际检查丢弃旧结果。
7. Rust 全量单元测试 162 个通过。
8. TypeScript 并发检查、节点编辑检查和类型检查通过。
9. 前端生产构建通过。
10. `git diff --check` 通过。

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | 未运行 | 根因明确的并发缺陷修复 |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | 跳过 | 当前运行在 Codex 内，禁止嵌套复审 |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | CLEAR | 2 个缺口已吸收，0 个关键缺口 |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | 未运行 | 只清理状态，不改变视觉设计 |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | 未运行 | 不改变构建和开发流程 |

**VERDICT:** ENG CLEARED，允许开始实施

NO UNRESOLVED DECISIONS
