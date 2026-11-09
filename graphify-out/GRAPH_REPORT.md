# Graph Report - KunBoxForWindows  (2026-11-08)

## Corpus Check
- 106 files · ~103,553 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1699 nodes · 4331 edges · 81 communities (71 shown, 10 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 145 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `9653dce6`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- AppState
- lib.rs
- kernel.rs
- .new
- singbox.rs
- git_proxy.rs
- types.ts
- NodeDetailModal.tsx
- String
- RuleSets.tsx
- types.rs
- latency.rs
- rules.rs
- latency/tests.rs
- rulesets.rs
- tauri.conf.json
- VPN 与测速并发协调计划
- manual-node.ts
- DomainRules.tsx
- settings.rs
- Issue #1 规则集仓库下载修复计划
- App.tsx
- subscription/tests.rs
- Health Failover Monitor Implementation Plan
- Dashboard.tsx
- 节点运行态污染修复计划
- Value
- compilerOptions
- LatencyProbeError
- Option
- nodesStore.ts
- singbox_start_impl
- Profiles.tsx
- dependencies
- export_node_to_link
- devDependencies
- Settings.tsx
- start_temp_singbox
- subscription.rs
- parse_node_link
- begin_latency_lifecycle
- generate_temp_config_with_native_xray_raw
- 阶段与依赖
- parse_vless_link
- fetch-raybox.ps1
- KunBox for Windows
- resolve_available_inbound_ports
- parse_hysteria2_link
- KunBox 代码审计报告
- 节点编辑与前置代理实施计划
- prepare_temp_nodes_with_detours
- KunBox-Windows 项目指南
- compilerOptions
- 工件与配置共同契约
- ErrorBoundary
- default.json
- bounded_selector_probe_stream
- 迁移现场基线（2026-11-05）
- package.json
- scripts
- global.d.ts
- constants.ts
- RayBox 统一迁移总计划
- 迁移过程状态
- eslint
- @radix-ui/react-tooltip
- test.mjs
- lucide-react
- typescript
- vite
- vite.config.ts
- kunbox
- react

## God Nodes (most connected - your core abstractions)
1. `AppState` - 167 edges
2. `unique_test_dir()` - 39 edges
3. `singbox_start_impl()` - 38 edges
4. `generate_config_with_settings_and_native()` - 36 edges
5. `write_json_file()` - 36 edges
6. `generate_config()` - 33 edges
7. `read_generated_config()` - 30 edges
8. `load_profiles_data()` - 27 edges
9. `run_health_monitor_once()` - 26 edges
10. `node_test_all()` - 23 edges

## Surprising Connections (you probably didn't know these)
- `proxy_failure_maps_to_node_failure_status()` --calls--> `map_latency_probe_result()`  [INFERRED]
  src-tauri/src/commands/profiles/latency/tests.rs → src-tauri/src/commands/profiles/latency.rs
- `generate_temp_config_adds_dns_for_ech_nodes()` --calls--> `generate_temp_config_raw()`  [INFERRED]
  src-tauri/src/commands/profiles/latency/tests.rs → src-tauri/src/commands/profiles/latency/config.rs
- `generate_temp_config_does_not_add_domain_resolver_for_ip_dns_server()` --calls--> `generate_temp_config_raw()`  [INFERRED]
  src-tauri/src/commands/profiles/latency/tests.rs → src-tauri/src/commands/profiles/latency/config.rs
- `generate_temp_config_removes_naive_unsupported_fields_and_uses_remote_dns()` --calls--> `generate_temp_config_raw()`  [INFERRED]
  src-tauri/src/commands/profiles/latency/tests.rs → src-tauri/src/commands/profiles/latency/config.rs
- `generate_temp_config_skips_naive_when_runtime_unavailable()` --calls--> `generate_temp_config_raw()`  [INFERRED]
  src-tauri/src/commands/profiles/latency/tests.rs → src-tauri/src/commands/profiles/latency/config.rs

## Import Cycles
- None detected.

## Communities (81 total, 10 thin omitted)

### Community 0 - "AppState"
Cohesion: 0.08
Nodes (95): Child, RwLock, add_runtime_metadata(), apply_profile_update(), assign_unique_node_tag(), collect_custom_profile_nodes(), custom_profile_accepts_only_new_node_links(), custom_profile_nodes_follow_selection_order_and_normalize_duplicate_tags() (+87 more)

### Community 1 - "lib.rs"
Cohesion: 0.26
Nodes (15): App, Box, append_startup_diagnostic(), configure_desktop_webview(), get_data_dir(), read_settings_sync(), AppHandle, AppSettings (+7 more)

### Community 2 - "kernel.rs"
Cohesion: 0.11
Nodes (69): Response, clear_kernel_cache_targets(), clears_actual_cache_db_and_ruleset_cache(), download_kernel_archive_to_path(), failed_replacement_restores_previous_kernel_and_backup(), get_bundle_kernel_dir(), get_data_kernel_dir(), get_dir_size() (+61 more)

### Community 3 - ".new"
Cohesion: 0.15
Nodes (49): Serialize, cancel_health_monitor_cancels_existing_token(), collect_health_targets_separates_selectors_and_fixed_nodes(), collect_health_targets_uses_renamed_main_selector_when_proxy_tag_collides(), explicit_scoped_metered_node_remains_available_for_single_node_routing(), generate_config(), generate_config_ech_repair_preserves_local_nodes(), generate_config_hijacks_dns_by_protocol_or_port() (+41 more)

### Community 4 - "singbox.rs"
Cohesion: 0.08
Nodes (33): backup_probe_candidates_rotate_past_previously_checked_prefix(), build_native_xray_outbound(), build_xray_plugin_config(), build_xray_plugin_config_hoists_legacy_xhttp_extra_encryption(), build_xray_plugin_config_preserves_vless_xhttp_transport(), build_xray_plugin_config_routes_remote_outbound_through_front_proxy(), config_file_has_outbound_type(), config_value_has_outbound_type() (+25 more)

### Community 5 - "git_proxy.rs"
Cohesion: 0.15
Nodes (30): Command, Output, RefCell, clear_git_proxy_snapshot(), connection_preserves_remote_git_proxy(), connection_sync_is_restored_after_disconnect(), disconnect_preserves_git_proxy_changed_by_user(), git_cli_backend_reads_writes_and_clears_isolated_global_proxy() (+22 more)

### Community 6 - "types.ts"
Cohesion: 0.09
Nodes (24): App(), HEALTH_EVENT_TOAST_TYPES, initApp(), ConnectionState, HealthToastType, defaultSettings, mockApi, mockNodes (+16 more)

### Community 7 - "NodeDetailModal.tsx"
Cohesion: 0.08
Nodes (39): applyNodePolicies(), DEFAULT_MULTIPLEX, durationMilliseconds(), enableMultiplex(), makeNodeReference(), NODE_POLICY_KEYS, NODE_RUNTIME_KEYS, nodePolicyState() (+31 more)

### Community 8 - "String"
Cohesion: 0.12
Nodes (52): allocate_clash_api_port(), allowed_xray_keys(), apply_system_proxy(), clear_persisted_proxy_snapshot(), clear_proxy_session_marker(), decode_windows_output(), delete_registry_value(), detect_foreign_wintun_aliases() (+44 more)

### Community 9 - "RuleSets.tsx"
Cohesion: 0.13
Nodes (19): builtInHubRules, defaultRuleSets, fastTransition, buildHubRuleSets(), HubRuleSet, HubTreeEntry, isHubFormatAvailable(), officialRawUrl() (+11 more)

### Community 10 - "types.rs"
Cohesion: 0.09
Nodes (24): Default, Into, AppSettings, CustomRules, DomainRule, HealthEvent, HealthEventKind, HealthStatus (+16 more)

### Community 11 - "latency.rs"
Cohesion: 0.14
Nodes (40): NodeLatencyResult, NodeLatencyStatus, RwLockReadGuard, acquire_temp_singbox_test_slot(), append_latency_diagnostic(), append_temp_latency_logs(), current_latency_test_settings(), dns_server_uses_domain_address() (+32 more)

### Community 12 - "rules.rs"
Cohesion: 0.12
Nodes (33): custom_rules_get(), custom_rules_save(), domain_rules_get(), domain_rules_save(), load_custom_rules(), CustomRules, DomainRule, Result (+25 more)

### Community 15 - "latency/tests.rs"
Cohesion: 0.09
Nodes (29): cleanup_temp_singbox_process(), release_temp_singbox_test_slot(), remove_temp_singbox_dir(), Path, can_start_temp_singbox(), cleanup_temp_singbox_process_kills_and_clears_process(), cleanup_temp_singbox_process_succeeds_when_slot_is_empty(), generate_temp_config_adds_dns_for_ech_nodes() (+21 more)

### Community 16 - "rulesets.rs"
Cohesion: 0.15
Nodes (34): atomic_cache_write_leaves_no_temp_file(), build_direct_client(), build_local_proxy_client(), download_and_verify(), download_ruleset(), extract_github_path(), forced_update_replaces_cache_and_failures_preserve_last_good_copy(), get_default_rulesets() (+26 more)

### Community 17 - "tauri.conf.json"
Cohesion: 0.05
Nodes (37): https://github.com/roseforljh/KunBoxForWindows/releases/latest/download/latest.json, icons/128x128@2x.png, icons/128x128.png, icons/32x32.png, icons/icon.icns, icons/icon.ico, msi, nsis (+29 more)

### Community 18 - "VPN 与测速并发协调计划"
Cohesion: 0.06
Nodes (35): 1. AppState 增加测速生命周期闸门, 1. 连接操作先取消测速, 2. 单节点结果不覆盖新状态, 2. 统一取消入口, 3. VPN 启停统一抢占测速, 3. 连接后的自动测速, 4. RunId 与结果代际隔离, 5. 临时内核所有权 (+27 more)

### Community 19 - "manual-node.ts"
Cohesion: 0.08
Nodes (34): AddProfileModal(), DNS_SERVERS, IMPORT_TYPES, ImportType, ProfileSettings, buildManualNodeLink(), createManualNodeDraft(), credentials() (+26 more)

### Community 20 - "DomainRules.tsx"
Cohesion: 0.16
Nodes (15): DOMAIN_TYPE_OPTIONS, DomainRules(), isBuiltInDomainRule(), OUTBOUND_MODE_OPTIONS, RoutingModeNotice(), RuleSets(), getRuleNodeValue(), NodeRule (+7 more)

### Community 21 - "settings.rs"
Cohesion: 0.14
Nodes (29): decode_windows_output(), ensure_u16_in_range(), ensure_u32_in_range(), get_settings(), parse_excluded_tcp_port_ranges(), port_in_ranges(), AppSettings, F (+21 more)

### Community 22 - "Issue #1 规则集仓库下载修复计划"
Cohesion: 0.06
Nodes (33): 1. 真实格式驱动, 2. 官方地址优先，备用地址受控, 3. TUN 和系统代理分流, 4. Source 旧地址明确拒绝, 5. 错误透传, 6. 仓库弹窗体验, 7. 下载文件安全写入, Failure mode review (+25 more)

### Community 23 - "App.tsx"
Cohesion: 0.07
Nodes (31): AboutIcon, AnimatedIconProps, AppContent(), defaultTransition, FileTextIcon, FolderIcon, footerItems, GaugeIcon (+23 more)

### Community 24 - "subscription/tests.rs"
Cohesion: 0.08
Nodes (31): clash_hysteria2_preserves_port_hopping_and_intervals(), export_fixture(), exports_http_transport_host_lists_and_grpc_service_names(), exports_hysteria2_port_hopping_without_emitting_port_zero(), exports_nodes_without_persisted_origin_metadata(), exports_ss_sip002_plugin_and_aead2022_password(), exports_trojan_password_sni_and_ws_with_reserved_characters(), exports_vless_reality_public_key_and_transport_in_standard_fields() (+23 more)

### Community 25 - "Health Failover Monitor Implementation Plan"
Cohesion: 0.06
Nodes (32): Health Failover Monitor Implementation Plan, Task 1: 增加健康监控类型和设置, Task 2: 增加 Health Monitor 状态容器, Task 3: 抽取 selector 和固定节点监控目标, Task 4: 实现低开销探针引擎, Task 5: 实现 selector 备用池和切换决策, Task 6: 接入 sing-box 生命周期, Task 7: 添加前端事件和提示 (+24 more)

### Community 26 - "Dashboard.tsx"
Cohesion: 0.10
Nodes (24): About(), DownloadProgress, UpdateInfo, Dashboard(), formatHealthStatus(), formatSelectorNodeName(), HEALTH_STATUS_LABELS, HealthToastType (+16 more)

### Community 27 - "节点运行态污染修复计划"
Cohesion: 0.06
Nodes (31): 1. 建立唯一的运行态字段表, 2. 后端读取边界清洗, 3. 后端保存边界清洗, 4. 启动迁移历史数据, 5. 前端编辑器隔离, GSTACK REVIEW REPORT, Rust, TODO 处理 (+23 more)

### Community 28 - "Value"
Cohesion: 0.08
Nodes (53): active_or_first_node(), allocate_unique_outbound_tag(), apply_outbound_domain_resolver(), apply_route_target(), build_dns_bootstrap_server(), build_dns_server(), build_dns_server_with_resolver(), collect_health_eligible_node_tags() (+45 more)

### Community 29 - "compilerOptions"
Cohesion: 0.07
Nodes (29): compilerOptions, allowImportingTsExtensions, allowSyntheticDefaultImports, baseUrl, esModuleInterop, isolatedModules, jsx, lib (+21 more)

### Community 30 - "LatencyProbeError"
Cohesion: 0.50
Nodes (7): LatencyProbeError, CancellationToken, Result, test_latency_via_clash_api(), test_latency_via_clash_api_cancellable(), test_latency_via_http_proxy(), test_latency_via_http_proxy_cancellable()

### Community 31 - "Option"
Cohesion: 0.09
Nodes (42): HealthEvent, HealthStatus, decide_health_action(), execute_health_action(), fetch_selector_health(), fixed_node_failure_action_builds_fixed_node_event(), fixed_node_failure_is_not_reported_again_before_next_probe(), fixed_node_failure_never_returns_switch_action() (+34 more)

### Community 32 - "nodesStore.ts"
Cohesion: 0.11
Nodes (26): NodeItem, SORT_OPTIONS, AddNodeModal(), AddNodeModalProps, AddNodeTarget, SUPPORTED_PROTOCOLS, AddProfileModalProps, NodeDetailModalProps (+18 more)

### Community 33 - "singbox_start_impl"
Cohesion: 0.13
Nodes (32): append_startup_diagnostic(), build_foreign_wintun_warning(), cancel_health_monitor(), close_stale_selector_connections(), extract_singbox_fatal_error(), get_clash_api_port(), is_running_as_admin(), local_clash_api_client() (+24 more)

### Community 34 - "Profiles.tsx"
Cohesion: 0.17
Nodes (12): Profiles(), ConfirmModal(), ConfirmModalProps, DNS_SERVERS, EditProfileModal(), EditProfileModalProps, ProfileSettings, Modal (+4 more)

### Community 35 - "dependencies"
Cohesion: 0.08
Nodes (25): class-variance-authority, clsx, framer-motion, dependencies, class-variance-authority, clsx, framer-motion, @radix-ui/react-dialog (+17 more)

### Community 36 - "export_node_to_link"
Cohesion: 0.24
Nodes (24): add(), add_field(), check_preserved(), export_node_to_link(), finish_export(), required(), Option, Result (+16 more)

### Community 37 - "devDependencies"
Cohesion: 0.08
Nodes (25): autoprefixer, eslint-plugin-react-hooks, devDependencies, autoprefixer, eslint-plugin-react-hooks, postcss, prettier, react-dom (+17 more)

### Community 38 - "Settings.tsx"
Cohesion: 0.10
Nodes (14): KernelCapabilities, KernelSettings(), KernelVersion, RemoteRelease, FIXED_NODE_HEALTH_NOTE, healthSettingsPatch, HEALTH_SETTING_KEYS, Settings() (+6 more)

### Community 39 - "start_temp_singbox"
Cohesion: 0.15
Nodes (20): cleanup_temp_singbox(), clear_temp_singbox_proxy_port_map(), clear_temp_singbox_tag_map(), PathBuf, check_clash_api_running(), get_kernel_path_with_fallback(), AppHandle, CancellationToken (+12 more)

### Community 40 - "subscription.rs"
Cohesion: 0.27
Nodes (17): decode_base64_compat(), fetch_subscription(), normalize_duplicate_node_tags(), parse_clash_proxies(), parse_host_port(), parse_port_value(), parse_singbox_outbounds(), parse_subscription_content() (+9 more)

### Community 41 - "parse_node_link"
Cohesion: 0.33
Nodes (18): apply_tls_params(), decode_link_fragment(), insert_proxy_credentials(), map_clash_type(), parse_anytls_link(), parse_http_proxy_link(), parse_naive_link(), parse_node_link() (+10 more)

### Community 42 - "begin_latency_lifecycle"
Cohesion: 0.16
Nodes (17): Drop, RwLockWriteGuard, begin_latency_lifecycle(), begin_latency_test_batch(), cancel_active_latency_batch(), cancel_and_reset_latency_test_token(), current_latency_test_cancel_token(), LatencyLifecycleGuard (+9 more)

### Community 43 - "generate_temp_config_with_native_xray_raw"
Cohesion: 0.21
Nodes (15): apply_temp_latency_domain_resolver(), generate_temp_config_raw(), generate_temp_config_with_dependencies_raw(), generate_temp_config_with_native_xray_raw(), HashMap, Result, String, Value (+7 more)

### Community 44 - "阶段与依赖"
Cohesion: 0.13
Nodes (14): Context, Phase 0：基线、工作树隔离与项目记录, Phase 1：RayBox 核心基线与补丁移植, Phase 2：统一产物、manifest 与发布门禁, Phase 3：KunBox Android 接入, Phase 4：SimpleNet 统一接入, Phase 5：KunBoxForWindows 接入, Phase 6：三客户端配置契约与功能矩阵统一 (+6 more)

### Community 45 - "parse_vless_link"
Cohesion: 0.22
Nodes (14): parse_vless_link_does_not_treat_non_url_suffix_as_ech_resolver(), parse_vless_link_enables_ech_without_invalid_config_for_name_resolver(), parse_vless_link_serializes_pem_ech_config_as_array(), parse_vless_link_skips_empty_ech_value(), parse_vless_link_xhttp(), extract_ech_dns_server(), extract_ech_name_and_dns_server(), extract_ech_public_name() (+6 more)

### Community 47 - "KunBox for Windows"
Cohesion: 0.14
Nodes (13): Acknowledgments, Build, Configuration, Development, Features, Getting Started, Installation, KunBox for Windows (+5 more)

### Community 48 - "resolve_available_inbound_ports"
Cohesion: 0.33
Nodes (9): find_available_tcp_port_avoiding(), inbound_listen_addr(), reserve_available_tcp_port_avoiding(), reserve_tcp_port(), resolve_available_inbound_ports(), resolve_available_inbound_ports_replaces_unavailable_ports(), Error, singbox_random_available_ports() (+1 more)

### Community 49 - "parse_hysteria2_link"
Cohesion: 0.24
Nodes (12): parse_bool_param(), parse_hysteria2_link(), parse_hysteria2_server_ports(), parse_hysteria_link(), parse_tuic_link(), Option, SingBoxOutbound, Value (+4 more)

### Community 50 - "KunBox 代码审计报告"
Cohesion: 0.17
Nodes (11): 10. 状态回滚 (Optimistic Settings State Drift), 1. DNS路由一致性 (Routing Correctness), 2. 选择器Tag冲突 (Selector Uniqueness), 3. 临时进程清理 (Temp Singbox Lifecycle), 4. 延迟测试并发 (Over-concurrent Latency Tests), 5. 缓存清理路径错误 (Wrong Cache Clear Path), 6. 下载内存峰值 (Kernel Download Memory Spike), 7. 前端写放大 (Frontend Write Amplification) (+3 more)

### Community 51 - "节点编辑与前置代理实施计划"
Cohesion: 0.17
Nodes (11): 后端更新, 安卓端对照, 数据模型, 文件范围, 目标, 编辑页, 自动验证, 节点编辑与前置代理实施计划 (+3 more)

### Community 52 - "prepare_temp_nodes_with_detours"
Cohesion: 0.27
Nodes (12): prepare_temp_nodes_with_detours(), resolve_temp_detour_dependency(), HashMap, HashSet, Result, String, Value, Vec (+4 more)

### Community 53 - "KunBox-Windows 项目指南"
Cohesion: 0.18
Nodes (10): KunBox-Windows 项目指南, 前端 (kunbox-electron 目录), 后端 (src-tauri 目录), 命令执行规范, 完整开发流程, 常用命令, 开发约定, 技术栈 (+2 more)

### Community 54 - "compilerOptions"
Cohesion: 0.20
Nodes (9): compilerOptions, allowSyntheticDefaultImports, composite, module, moduleResolution, skipLibCheck, strict, include (+1 more)

### Community 55 - "工件与配置共同契约"
Cohesion: 0.25
Nodes (7): Android, Windows, 发布, 工件与配置共同契约, 服务端, 来源与本地候选, 节点与配置

### Community 56 - "ErrorBoundary"
Cohesion: 0.25
Nodes (3): ErrorBoundary, ErrorBoundaryProps, ErrorBoundaryState

### Community 57 - "default.json"
Cohesion: 0.25
Nodes (7): core:default, main, description, identifier, permissions, $schema, windows

### Community 58 - "bounded_selector_probe_stream"
Cohesion: 0.25
Nodes (8): AtomicUsize, I, Item, bounded_selector_probe_stream(), bounded_selector_probe_stream_yields_before_all_probes_finish(), F, update_max(), Stream

### Community 59 - "迁移现场基线（2026-11-05）"
Cohesion: 0.29
Nodes (6): 写入协作, 初始验证, 接口复核, 旧 RayBox AAR 实检, 来源错配（主会话实证）, 迁移现场基线（2026-11-05）

### Community 60 - "package.json"
Cohesion: 0.29
Nodes (6): author, description, license, name, type, version

### Community 61 - "scripts"
Cohesion: 0.25
Nodes (8): scripts, build, dev, format, lint, lint:fix, test, typecheck

### Community 62 - "global.d.ts"
Cohesion: 0.33
Nodes (5): API, *.jpg, *.png, *.svg, Window

### Community 63 - "constants.ts"
Cohesion: 0.33
Nodes (5): APP_NAME, APP_VERSION, CLASH_API_URL, IPC_CHANNELS, SINGBOX_DOWNLOAD_URL

### Community 64 - "RayBox 统一迁移总计划"
Cohesion: 0.40
Nodes (4): RayBox 统一迁移总计划, 不得丢失的现有行为, 必须建立, 硬阻塞

### Community 65 - "迁移过程状态"
Cohesion: 0.25
Nodes (7): 2026-11-05, 2026-11-05 的历史阻塞, 2026-11-08 HK 全协议真机巡检（Windows）, 2026-11-08 Windows UI 与 RayBox 语义版本更新, 下一阶段, 最新验证（2026-11-05）, 迁移过程状态

## Knowledge Gaps
- **407 isolated node(s):** `name`, `version`, `type`, `description`, `author` (+402 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **10 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AppState` connect `AppState` to `singbox_start_impl`, `kernel.rs`, `.new`, `singbox.rs`, `git_proxy.rs`, `start_temp_singbox`, `String`, `begin_latency_lifecycle`, `latency.rs`, `rules.rs`, `latency/tests.rs`, `rulesets.rs`, `resolve_available_inbound_ports`, `settings.rs`, `Value`, `Option`?**
  _High betweenness centrality (0.178) - this node is a cross-community bridge._
- **Why does `parse_node_link()` connect `parse_node_link` to `AppState`, `export_node_to_link`, `subscription.rs`, `parse_vless_link`, `parse_hysteria2_link`, `subscription/tests.rs`?**
  _High betweenness centrality (0.042) - this node is a cross-community bridge._
- **Why does `start_temp_singbox()` connect `start_temp_singbox` to `AppState`, `types.rs`, `generate_temp_config_with_native_xray_raw`, `latency.rs`, `latency/tests.rs`, `prepare_temp_nodes_with_detours`?**
  _High betweenness centrality (0.028) - this node is a cross-community bridge._
- **What connects `name`, `version`, `type` to the rest of the system?**
  _407 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `AppState` be split into smaller, more focused modules?**
  _Cohesion score 0.07972367438386856 - nodes in this community are weakly interconnected._
- **Should `kernel.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.10524256651017215 - nodes in this community are weakly interconnected._
- **Should `.new` be split into smaller, more focused modules?**
  _Cohesion score 0.14857142857142858 - nodes in this community are weakly interconnected._