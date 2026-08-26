# Issue #1 规则集仓库下载修复计划

## Issue 事实

Issue #1：`[Bug] 从仓库下载远程规则集失败`。

- 环境：Windows 11、KunBox v0.0.35、TUN 已开启。
- 用户反馈：系统可以访问 GitHub，KunBox 规则集仓库下载失败。
- Issue 状态：Open。
- 评论和日志：没有提供。

已经核对 v0.0.35 代码和当前上游仓库：

1. `sing-geosite/rule-set` 当前有 1872 个 `.srs` 文件，0 个 `.json` 文件。
2. 前端仓库列表却为每个 `.srs` 文件拼接 `.json` Source 地址。
3. `https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.json` 返回 404。
4. 内置地址大量使用 `ghp.ci`，该域名当前 TLS 连接失败。
5. 后端还会串行尝试 `mirror.ghproxy.com`、`raw.gitmirror.com`、`raw.fastgit.org`，这些地址当前也存在 TLS 或服务失效。

## 根因

```text
GitHub API 返回实际 .srs 文件
          │
          ▼
前端强行生成 .srs 和 .json 两个地址
          │
          ├─ Binary → .srs，地址真实存在
          └─ Source → .json，地址不存在，返回 404

下载端再串行尝试多个失效镜像
          │
          ▼
失败时间变长，前端只显示“下载失败”
```

TUN 不是这条 Issue 的唯一根因。TUN 只决定 Rust 请求是否能走代理路径，无法让不存在的 `.json` 文件变存在。

## 目标

1. 仓库页面只展示上游真实存在的格式。
2. 官方 `rule-set` 仓库默认只展示 Binary。
3. Source 只有在 API 明确返回真实源文件时才显示。
4. 下载优先走官方地址，备用地址数量受控。
5. TUN、系统代理、未连接 VPN 三种网络状态都能得到清晰行为。
6. 下载失败向用户返回 HTTP、TLS、超时、代理不可用等真实原因。
7. 旧配置中的失效 Source 地址不会继续静默失败。
8. 内置仓库加载失败时仍能使用内置列表。
9. 不改变 sing-box 规则集缓存格式和规则匹配逻辑。

## 方案

### 1. 真实格式驱动

把 `HubRuleSet` 的地址改成可选字段：

```text
sourceUrl?: string
binaryUrl?: string
```

前端读取 GitHub tree 时按实际扩展名分组：

```text
同一个规则名
 ├─ 存在 .srs → binaryUrl
 └─ 存在 .json → sourceUrl
```

当前官方仓库只会得到 `binaryUrl`，Source 按钮不渲染。

内置仓库列表也只保留真实 `.srs` 地址，去掉当前失效的 `.json` 和 `ghp.ci` 地址。

### 2. 官方地址优先，备用地址受控

后端将下载地址策略固定为：

```text
原始 URL
   │
   ├─ 代理客户端：127.0.0.1:本地端口
   │
   ├─ 直连客户端：明确 no_proxy，交给 TUN 或系统路由
   │
   └─ GitHub 官方 URL 的 CDN 备用地址
```

只保留两类官方或稳定地址：

- `raw.githubusercontent.com`
- `cdn.jsdelivr.net/gh/...`

移除死镜像列表，不再对四个不稳定地址做 30 秒级串行等待。

### 3. TUN 和系统代理分流

```text
TUN 开启
  → 直连客户端优先
  → TUN 接管操作系统流量
  → 本地代理作为备用

TUN 关闭且系统代理开启
  → 本地 KunBox 代理优先
  → 直连作为备用

VPN 未连接
  → 直连尝试
  → 失败后返回明确网络错误
```

直连客户端使用 `no_proxy()`，避免 Rust 进程意外继承失效系统代理环境变量。

### 4. Source 旧地址明确拒绝

后端收到 `format=source` 且地址指向官方 `rule-set/*.json` 时，返回明确错误：

```text
官方规则集仓库当前只提供 Binary 格式，请选择 Binary 下载。
```

不再浪费时间请求必然 404 的地址。

对非官方、自定义 URL 仍保留 Source 下载能力，只要用户自己提供的地址真实存在。

### 5. 错误透传

后端记录每次尝试的：

- URL
- 访问方式：代理或直连
- HTTP 状态码
- 错误分类

最终错误返回给前端。前端显示具体错误，不再统一显示空泛的“下载失败”。

### 6. 仓库弹窗体验

打开仓库弹窗时先显示内置列表，后台刷新官方列表：

```text
打开弹窗
  → 内置列表立即可见
  → 后台请求 GitHub tree
  → 成功：替换为实时列表
  → 失败：保留内置列表并显示错误
```

刷新按钮继续可用，失败后不会卡在空白加载页。

### 7. 下载文件安全写入

规则集下载成功后先写临时文件，再原子替换缓存文件：

```text
下载完整内容
  → 校验大小和内容头
  → 写 tag.srs.tmp
  → rename 为 tag.srs
```

防止网络中断时留下半个规则集，导致下次被误认为已缓存。

## 文件范围

1. `src-tauri/src/commands/rulesets.rs`
   - 稳定地址策略。
   - TUN 和代理客户端分流。
   - Source 官方地址拒绝。
   - 详细错误分类。
   - 临时文件原子替换。
   - Rust 单元测试。
2. `kunbox-electron/src/renderer/components/rulesets-model.ts`
   - Hub 项目格式分组和可用按钮判断纯函数。
3. `kunbox-electron/src/renderer/components/RuleSets.tsx`
   - HubRuleSet 地址改为可选。
   - 按 API 实际文件格式生成项目。
   - 只渲染可用格式按钮。
   - 内置列表去除失效 Source 地址。
   - 下载失败显示后端错误。
   - 仓库刷新失败保留内置列表。
4. `kunbox-electron/src/renderer/components/rulesets-model.test.ts`
   - 增加格式分组、地址生成和错误展示辅助函数测试。

共 4 个代码文件，不新增依赖，不新增服务。

## 数据流

```text
GitHub tree API
      │
      ▼
按规则名分组并识别扩展名
      │
      ├─ .srs → Binary 项目
      └─ .json → Source 项目
      │
      ▼
用户选择真实格式
      │
      ▼
后端选择代理或直连
      │
      ├─ 成功 → 校验 → 临时文件 → 原子替换缓存
      └─ 失败 → 分类错误 → 前端显示具体原因
```

## 错误处理

1. GitHub tree API 超时：保留内置列表并显示刷新失败。
2. GitHub tree 返回非 2xx：显示 HTTP 状态码。
3. 官方 Source 地址：立即返回格式不支持，不发网络请求。
4. 代理连接失败：记录代理错误，继续直连。
5. 直连失败：尝试一个稳定 CDN 备用地址。
6. CDN 失败：返回所有尝试的简短摘要。
7. HTTP 404：显示资源不存在。
8. HTTP 403：显示访问受限。
9. TLS 错误：显示域名连接失败。
10. 文件太小、HTML、JSON 错误页：拒绝写入缓存。
11. 临时文件写入失败：保留旧缓存，不覆盖有效文件。

## 测试计划

### Rust

1. 官方 `rule-set` tree 只含 `.srs` 时，生成项目没有 `sourceUrl`。
2. `.srs` 和 `.json` 同名时，两个地址都正确生成。
3. `.json` 不存在时，Source 官方请求被拒绝并返回明确错误。
4. GitHub 原始 URL 保持不被错误改写。
5. `ghp.ci`、旧镜像地址可以转换到官方 raw 路径。
6. 代理失败后会尝试直连。
7. 直连失败后只尝试受控备用地址。
8. HTML、JSON、过小文件不会写入缓存。
9. 下载中断不会破坏已有缓存。
10. 有效下载会完成临时文件到正式缓存的替换。

### TypeScript

1. 只有 Binary 的 Hub 项目只显示 Binary 按钮。
2. 只有 Source 的 Hub 项目只显示 Source 按钮。
3. 两种格式都存在时显示两个按钮。
4. Hub 项目没有地址时不会生成可点击按钮。
5. 下载异常文本会显示具体后端错误。
6. 仓库请求失败后保留内置列表。

### 覆盖图

```text
代码路径                                      用户路径
[+] GitHub tree 分组                          [+] 打开规则集仓库
  ├─ .srs                                     ├─ [★★★] 内置列表立即显示
  ├─ .json                                    ├─ [★★★] 实时列表格式正确
  └─ 其他扩展名忽略                            └─ [★★★] 请求失败仍可操作

[+] ruleset_download                           [+] 选择 Binary
  ├─ cached                                    ├─ [★★★] 成功后启用
  ├─ source 拒绝                               ├─ [★★★] 失败显示具体原因
  ├─ proxy                                      └─ [★★★] 不生成坏缓存
  ├─ direct
  ├─ CDN fallback                               [+] 选择 Source
  └─ content validation                         ├─ [★★★] 官方无 Source 时按钮隐藏
                                                └─ [★★★] 自定义真实 Source 仍可用
```

## 性能边界

1. 仓库 tree 只请求一次，前端按内存 Map 分组。
2. 下载最多三条路径：代理官方、直连官方、CDN 备用。
3. 每条路径使用现有超时机制，缩短失效镜像导致的最长等待。
4. 不读取和缓存整个 GitHub 仓库，只下载用户选中的规则集。

## 既有能力复用

1. 复用现有 `ruleset_fetch_hub` GitHub tree API。
2. 复用现有 `extract_github_path` 和规则集内容校验。
3. 复用现有本地代理客户端和 TUN 网络路径。
4. 复用现有规则集缓存目录和 `ruleset_is_cached`。
5. 复用现有内置规则集列表和前端下载流程。

## 不在范围内

1. 不改 sing-box 规则集格式本身。
2. 不新增规则集托管服务。
3. 不自动把不存在的 Source 转换成其他数据格式。
4. 不删除用户已有规则集配置，只在下载时给出可操作错误。
5. 不改 Issue 以外的节点、TUN、测速逻辑。

## 并行化

顺序实施。前端格式模型依赖后端返回语义，后端下载策略和前端错误展示共享同一验收路径，拆并行分支会增加冲突。

## 实施任务

- [x] **T1（P1）**：重构规则集地址和格式识别，只展示真实可用格式。
- [x] **T2（P1）**：重构后端下载路径和代理/TUN 分流。
- [x] **T3（P1）**：增加 Source 拒绝、错误分类和原子缓存写入。
- [x] **T4（P1）**：完善前端仓库弹窗、错误透传和旧地址提示。
- [x] **T5（P1）**：补 Rust 与 TypeScript 回归测试并完成构建验证。

## 验收标准

1. 官方仓库页面不再显示不可用的 Source 按钮。
2. Binary 规则集可以通过官方 raw 地址下载。
3. Source 地址不存在时不会发起无意义的网络请求。
4. TUN 开启时下载路径不依赖失效的系统代理。
5. 代理或直连失败时用户能看到具体原因。
6. 下载失败不会生成半截缓存。
7. 已有内置规则集在仓库刷新失败时仍可操作。
8. Rust 和 TypeScript 回归检查通过。

## 自审记录

- Step 0：范围接受，4 个代码文件，0 个新依赖，0 个新服务；使用纯函数抽离格式模型，避免在 React 组件内堆积不可测逻辑。
- 架构审查：发现 2 个问题，已补入计划：Source JSON 响应不能被通用二进制校验误杀；缓存写入必须按规则集格式校验。
- 代码质量审查：发现 1 个问题，已补入计划：旧镜像列表和地址转换必须集中在后端，避免前后端各自维护不同 URL。
- 测试审查：发现 3 个缺口，已补入真实格式分组、Source 内容校验和临时缓存原子替换测试。
- 性能审查：发现 1 个问题，已补入最多三条请求路径和单次 tree 请求约束。
- 既有能力复用：GitHub tree、现有代理客户端、内容大小校验和缓存目录全部复用。
- 生产失败模式：0 个无测试、无错误处理且静默失败的关键缺口。
- Outside voice：当前运行在 Codex 内，跳过嵌套 Codex 复审。
- 审查结论：通过，可以实施。

## What already exists

1. `ruleset_fetch_hub` 已能获取 GitHub tree，只需把结果格式化方式修正。
2. `extract_github_path` 已能识别 raw、镜像和 jsDelivr 地址，本次集中扩展为官方和 CDN 地址生成。
3. `build_local_proxy_client` 已能使用 KunBox 本地端口，本次只补明确直连客户端。
4. `download_and_verify` 已有大小、HTML 和 JSON 头校验，本次改为按 `RuleSet.format` 校验。
5. `RuleSets.tsx` 已有内置列表、仓库弹窗、下载和 toast 流程，本次只替换数据模型和错误展示。

## NOT in scope

1. 不修改 sing-box 规则集解析逻辑，当前问题发生在下载地址和下载链路。
2. 不新增外部规则集代理服务，避免引入新的可用性和隐私依赖。
3. 不自动把 Source JSON 转 Binary，转换属于构建工具职责，不在客户端做。
4. 不删除旧规则集配置，只标记不可用并提供重新选择 Binary 的路径。
5. 不修改 TUN、节点、测速和代理抢占逻辑。

## Test coverage diagram

```text
CODE PATHS                                      USER FLOWS
[+] treeToHubRuleSets()                         [+] 打开规则集仓库
  ├─ only .srs                                  ├─ [★★★] 立即看到内置列表
  ├─ only .json                                 ├─ [★★★] 实时列表显示真实格式
  ├─ both formats                               └─ [★★★] 刷新失败仍可操作
  └─ ignore other files

[+] ruleset_download                            [+] 选择 Binary
  ├─ cached                                     ├─ [★★★] 下载成功并启用
  ├─ source format allows JSON                   ├─ [★★★] 404 显示具体原因
  ├─ official Source rejection                   └─ [★★★] 下载中断不破坏缓存
  ├─ proxy/direct/CDN order
  └─ atomic cache write                         [+] 选择 Source
                                                ├─ [★★★] 官方无 Source 时不显示
[+] content validation                          └─ [★★★] 自定义 Source 可下载
  ├─ binary rejects HTML/JSON
  ├─ source accepts valid JSON
  └─ both reject undersized content
```

全部新增纯函数、错误分支和文件写入分支都有对应 Rust 或 TypeScript 检查，不增加 E2E。

## Failure mode review

| 失败模式 | 覆盖 | 处理 | 用户结果 |
|---|---|---|---|
| API 返回只有 `.srs` | 分组测试 | 不生成 Source 地址 | Source 按钮隐藏 |
| Source JSON 被误判错误 | 内容校验测试 | 按 format 允许 JSON | 自定义 Source 可用 |
| 镜像 TLS 失败 | 地址顺序测试 | 只保留官方和 CDN | 失败更快且可解释 |
| 下载中断 | 临时文件测试 | 原子替换 | 不产生坏缓存 |
| 代理不可用 | 客户端回退测试 | 直连继续尝试 | TUN 场景可恢复 |
| 后端返回 404/403 | 错误分类测试 | 透传状态 | 用户知道具体原因 |

## Implementation Tasks

- [x] **T1（P1）**：抽取真实格式分组纯函数，隐藏不存在的 Source 选项。
- [x] **T2（P1）**：重构规则集下载 URL、代理/TUN 回退和超时策略。
- [x] **T3（P1）**：按 format 校验内容并用临时文件原子替换缓存。
- [x] **T4（P1）**：透传具体错误并标记历史失效 Source 地址。
- [x] **T5（P1）**：补 Rust 与 TypeScript 回归检查，运行全量验证。

## 落地结果

1. 官方仓库 tree 按真实 `.srs`、`.json` 文件分组，只有存在的格式才显示按钮。
2. 官方 Source 地址会被识别并拒绝，避免无意义 404 请求。
3. 下载顺序改为本地代理官方地址、明确直连官方地址、jsDelivr 备用地址。
4. 直连客户端使用 `no_proxy`，避免错误继承失效系统代理。
5. Source 按 JSON 校验，Binary 拒绝 HTML/JSON 错误响应。
6. 下载使用临时文件后原子替换，失败不会留下半截缓存。
7. 后端错误包含 HTTP、超时、连接失败和内容格式原因。
8. 内置失效 Source 地址会被清理，仓库刷新失败保留内置列表。
9. Rust 全量单元测试 166 个通过。
10. 规则集 Rust 定向测试 8 个通过。
11. TypeScript 格式模型检查和类型检查通过。
12. 前端生产构建通过。
13. `git diff --check` 通过。

知识图谱刷新已执行，但现有图谱扫描遇到仓库内非 UTF-8 文件而失败，不影响代码和测试结果。

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | 未运行 | Issue 根因明确，未改变产品方向 |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | 跳过 | 当前运行在 Codex 内，禁止嵌套复审 |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | CLEAR | 4 个缺口已吸收，0 个关键缺口 |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | 未运行 | 只调整可用格式和错误文案 |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | 未运行 | 不改变构建流程 |

**VERDICT:** ENG CLEARED，允许开始实施

NO UNRESOLVED DECISIONS
