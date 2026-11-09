# 迁移过程状态

## 当前交付状态（2026-11-08）

- Windows 已统一使用可信 RayBox 0.0.1，移除插件入口与旧插件下载 IPC；HK 完整协议集经桌面客户端实测 15/15 通过。
- 最终 `build.ps1 -Mode release` 已成功生成 MSI/NSIS；尚未安装这次包含 HK 转换修复的最终安装包，由用户从 GitHub 构建后自行安装。
- 提交前补验：原生 Xray 两项回归及 RayBox 六项身份/校验/回滚测试全部通过（8/8）。此前前端检查与实际 UI 验收已通过。
- RayBox-rx 当前为公开仓库；GitHub 构建下载使用 `RAYBOX_READ_TOKEN`，未配置时使用工作流自带 token，不需要额外创建 secret。
- 本地调试实例已停止；本机 requireAdmin/TUN 和节点配置仍是测试状态，原始配置保存在 `%APPDATA%\KunBox\*.hkbench-backup`，尚未恢复。下面按日期保留历史记录，历史阻塞不代表当前状态。

## 2026-11-05

- 完成四仓库只读盘点。
- 写入统一总计划：`C:\Users\33039\.claude\plans\parsed-squishing-pnueli.md`。
- 在本项目创建 `docs/raybox-unification/README.md`。
- 确认当前核心是外部 `sing-box.exe`，XHTTP 通过独立 `xray.exe` bridge；资源目录没有 RayBox Windows 原生工件。
- Rust/前端代码已接入 RayBox Windows 原生候选验证；RayBox 仓独立建立 Windows amd64 原生 EXE、manifest 和构建流程。
- 接入继续复用 `version/check/run` 与回环 Clash API，不引入 DLL/FFI，不重写控制面；正式连接和临时测速均支持原生 Xray 出站。
- 已移除启动时按映像名全局 `taskkill /IM sing-box.exe`；停止流程只处理 `state.singbox_process`、受管 bridge 和测速进程。
- 内核安装在缺少正式 RayBox lock 时明确拒绝版本列表和下载，不再回退到官方 sing-box/JSDelivr 下载；安装替换失败会恢复旧文件。
- 已接入 `type=xray` 原生配置、RayBox manifest/PE/SHA 校验和 Windows candidate runtime 验证；现有 Xray bridge 仅保留为显式迁移回滚路径。
- 2026-11-05 续接：正式配置路径与临时测速配置均已改为在有效 RayBox manifest 且核心可用时生成原生 `type: xray` outbound；不满足时保留旧 bridge 路径。
- 原生转换支持 VLESS/VMess/Trojan 的 protocol/settings/streamSettings，保留 TLS/Reality/WS/XHTTP 与 VLESS Encryption；VMess alter_id 非零和不支持字段不静默丢弃，转换失败通过 Result 返回节点级错误，不生成 block 伪装成功。
- 内核校验新增 RayBox manifest 结构、正式 dirty=false、repository/40位 commit、Windows amd64、PE machine=0x8664、size、SHA-256、双核心版本和工具链校验；当前 `D:\RayBox-rx\artifacts\windows-candidate` manifest 为 dirty=true，因此不会被正式消费。
- Phase6 fixture 已由真实 Rust 转换器测试生成：`docs/raybox-unification/fixtures/generated-xhttp.json`；测试 `generate_contract_xhttp_fixture` 通过（1 passed/0 failed），fixture 包含 VLESS/XHTTP/TLS SNI/ALPN/fingerprint、tag、detour 外层拨号字段。
- Windows Rust 全量验证通过：`cargo fmt --check`、`cargo test`（204 passed、1 ignored、0 failed）和 `cargo check`；仅保留未使用字段/函数/常量警告。
- Windows 前端验证通过：`npm run typecheck` 与 `npm run build`，Vite 转换 2103 个模块。
- Windows candidate runtime 通过双核心 version、`check -c`、`run -c`、回环 Clash API 和 selector 两次切换；只停止自有测试进程，未触碰既有用户进程、系统代理或 TUN。

## 最新验证（2026-11-05）

- Windows Rust 全量测试：206 passed、1 ignored、0 failed；`cargo check` 与 `cargo fmt --check` 通过。
- Windows 前端 `typecheck` 和 `build` 通过，Vite 转换 2103 个模块。
- Windows 原生 RayBox candidate 完成双版本、`check -c`、`run -c`、回环 Clash API 和 selector 两次切换；只停止自有测试进程。
- Windows manifest 现在校验完整 commit、fullHead、tag/dirty、sourceDigest、来源 URL/许可证、精确工具链、buildTags、RX/OOB 默认值、PE 架构以及全部声明资产的路径、大小和 SHA-256；正式下载改为可信 lock 驱动，旧 sing-box/JSDelivr 下载链已移除。
- 正式 native 配置和 latency 配置均通过 `Result` 传播转换错误；WS、XHTTP/SplitHTTP、TLS/Reality、detour/dial fields 与受支持字段按契约处理，不支持组合明确报错，不再生成 block 伪装成功。
- 三客户端真实配置契约检查：`clients=3`、`equal=true`、`nativeCoreCheck=true`、`rxDefault=false`、`oobDefault=false`。

## 2026-11-05 的历史阻塞

- 当前 Windows candidate 为 `dirty=true`、`tag=null`，只能用于本地 debug 验证；正式消费仍需 clean commit、真实匹配 tag、不可变 lock 和用户授权。
- Android 当前无 adb 设备，手机 TUN/Root/Proxy-only 真实验收未完成；Windows 真实 TUN、生产代理线路和正式测速未完成。
- 现有 Xray bridge 仅保留为明确的迁移回滚路径。

## 下一阶段

仅需完成真实设备/线路验收或建立 clean RayBox release 身份；不得把 dirty candidate 标记为正式工件，也不执行提交、推送、部署或发布。

## 2026-11-08 HK 全协议真机巡检（Windows）

- 用 HK 测试服务器的完整协议订阅（15 节点：VLESS+REALITY、XHTTP+REALITY+加密、XHTTP+加密、WS+TLS+加密、XHTTP+TLS/H1/H3/fixed-udp、TUIC native/quic、AnyTLS TLS/REALITY、SS2022 aes-128/aes-256/chacha20）导入 KunBox，经运行中内核的 Clash 选择器逐节点热切换。
- 修了两个转换器缺陷后实测 **15/15 通过**，全部经真实代理请求验证 HK 出口（216.236.7.40 / HKG）：
  - `packet_encoding`（xudp/空）不再导致整体启动失败，未知值仍拒绝；
  - `tls.certificate`（自签测试 CA）映射为 Xray `tlsSettings.certificates`（verify），Reality 与自定义证书冲突时明确报错。
- 原订阅（归一 10 节点）巡检：8 通过、2 失败（failure 节点在应用内本来就显示“节点失败”，属订阅死节点，非内核问题）。
- 巡检期间为方便调试临时关闭了 requireAdmin/TUN（系统代理模式），结束后恢复；调试实例以非提权方式运行。



- RayBox 已发布 `windows-v0.0.1`（commit `a7271fd63a1d5946b7563f15827528e2d516f6b4`，version 0.0.1，dirty=false）；EXE 实测 `RayBox version 0.0.1`。`android-v0.0.1` 为同一 commit，`server-v0.0.1` 为 `1880528af97c4ac3bc44a6c304321995539bccd3`（修复发布任务 `downloaded/` 目录被来源校验误判 dirty 后重建）。
- 可信 lock 已更新为 0.0.1：manifest 原始 SHA256 `1808bc1185e662dbb86cc91dd18f6c316925df3a95672e0c9e3a9d48eb1e8478`，sourceDigest `99e78a7f1ae5b5b28f3021e711d28f87d83f5a7a22ecccd40c01132f9d8d5c3a`；全部资产经 gh 下载校验 size/SHA256。
- `scripts/fetch-raybox.ps1` 用新锁真实下载并校验通过；内核契约测试 9 项全部通过（含真实 pinned 制品与锁匹配）。
- 无插件版完整 `build.ps1 -Mode release` 成功生成 MSI/NSIS，MSI 内已含 0.0.1 内核。用户关闭旧客户端后，新版本经 NSIS 静默安装（保留用户数据）并实际启动验证：
  - 侧栏只剩 仪表盘/节点/订阅/规则集/域名分流/设置/日志/关于，无“插件”；
  - 设置 → 内核显示 RayBox、本地版本 0.0.1、可信目标版本 0.0.1、“已是可信目标版本”，无 stable/alpha 分支与 sing-box/Xray 文案；
  - 运行中的内核进程为 `raybox-windows-amd64`（0.0.1），经本地代理端口 127.0.0.1:1252 实测 Cloudflare trace：出口 RU（95.163.84.18）、TLS 1.3。

## 2026-11-08 Windows UI 与 RayBox 语义版本更新

- 左侧“插件”入口、旧 Xray 插件页面、前端 plugin IPC API 和 Rust 插件下载 IPC 已移除；用户已有磁盘文件和 profile 未删除。
- 设置 → 内核已收敛为单一 RayBox 页面，移除 stable/alpha 假分支、sing-box/Xray 下载文案，显示可信目标版本、能力和下载进度；mock 也改为 RayBox 0.0.1。
- 正式启动和测速不再启动外部 xray.exe 插件桥；没有有效 RayBox manifest 时明确拒绝启动/测速。内部 `core=xray` 与 `type=xray` 仍作为协议兼容字段保留。XHTTP 无插件回归已通过 8 项。
- RayBox `VERSION` 改为 0.0.1，产品运行版本不再追加 commit SHA；clean 平台标签改为 `android-v0.0.1`、`windows-v0.0.1`、`server-v0.0.1`，完整 commit/fullHead/sourceDigest 和资产校验仍保留。旧 Windows `windows-SHA + 1.0.0-SHA12` lock/资产仍兼容校验，未伪造为 0.0.1。
- RayBox 版本测试通过，发布工作流 fake-GitHub 测试和 Android 来源校验通过；未触发 GitHub 发布。
- Windows 前端 typecheck、lint、9 个测试文件、Vite build 通过；Rust 定向内核测试 9 项通过，XHTTP 定向测试 8 项通过。完整 Rust 测试 205 项通过、1 项失败：`exports_local_node_fixtures` 是显式 ignored 本地夹具测试，未设置 `KUNBOX_TEST_NODE_EXPORT_DIR`，不是代码失败；此前带自建夹具执行时为 208 通过、0 失败、0 忽略。


- 已固定正式 clean 提交 `6a9e396e41a2e9d2b1b5b465139507f6deb0c84e`，tag 为 `windows-6a9e396e41a2e9d2b1b5b465139507f6deb0c84e`，版本为 `1.0.0-6a9e396e41a2`。Windows sourceDigest 为 `3e8117542808782a7467f72c3014fdface76fff235d62341f1c51d2ff95d4bfc`，与 Android 分别使用各自正式发布值。
- 可信 manifest 原始 SHA256 为 `febdd1e490bb35599493e8b5b3fa8a828f05c449eed290679484919e828630c6`；`raybox-windows-amd64.exe` 为 80365568 字节，SHA256 为 `6bd7b97b4bb2d10b8e4bea64421bc3ee41213034808d737d8c9220dfc41aa727`。正式来源、工具链、许可证、哈希、PE amd64 与真实 version 命令验证通过。
- Rust 全量 `cargo test -- --include-ignored`：208 项通过、0 失败、0 忽略。原 opt-in 导出测试使用自建四协议夹具执行；测试打印的 re-imported 文案不代表该函数进行了再次解析。真实 RayBox 配置校验依赖已配置。
- 前端 9 个测试文件全部执行通过，typecheck、lint、build 通过；Rust release 构建通过。仍有已有 dead_code、Browserslist 与 chunk 大小提示。
- `scripts/fetch-raybox.ps1` 的已有资源校验在 PowerShell 5/7 均通过；新空目录真实下载通过，包括 tag 解引用、可信 manifest 原始哈希和每个资产 size/hash。子脚本正常返回调用者，构建入口不再误读陈旧退出码。
- `build.ps1 -Mode release` 完整执行成功，生成 `src-tauri/target/release/bundle/msi/KunBox_0.0.24_x64_en-US.msi` 和 `src-tauri/target/release/bundle/nsis/KunBox_0.0.24_x64-setup.exe`。本机未配置更新签名密钥，本次未生成自动更新签名文件。
- 当时 CI 下载步骤已本地同步到固定 RayBox 下载脚本，但尚无 `RAYBOX_READ_TOKEN` secret；当时 Windows 真实 TUN、生产代理线路和桌面 UI 验收仍未执行。此项为历史记录，当前状态见文首。
