# Graph Report - .  (2026-05-28)

## Corpus Check
- 15 files ¡¤ ~32,507 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 429 nodes ¡¤ 903 edges ¡¤ 21 communities detected
- Extraction: 100% EXTRACTED ¡¤ 0% INFERRED ¡¤ 0% AMBIGUOUS
- Token cost: 0 input ¡¤ 0 output

## God Nodes (most connected - your core abstractions)
1. `singbox_start_impl()` - 24 edges
2. `generate_config_with_settings()` - 21 edges
3. `load_profiles_data()` - 18 edges
4. `generate_config()` - 15 edges
5. `unique_test_dir()` - 15 edges
6. `start_temp_singbox()` - 14 edges
7. `write_json_file()` - 14 edges
8. `read_generated_config()` - 14 edges
9. `make_test_state()` - 13 edges
10. `cleanup_temp_singbox()` - 12 edges

## Surprising Connections (you probably didn't know these)
- `test_latency_via_temp_backend()` --calls--> `start_temp_singbox()`  [EXTRACTED]
  src\commands\profiles.rs ¡ú src\commands\profiles.rs  _Bridges community 12 ¡ú community 14_
- `test_latency_via_temp_backend()` --calls--> `release_temp_singbox_test_slot()`  [EXTRACTED]
  src\commands\profiles.rs ¡ú src\commands\profiles.rs  _Bridges community 12 ¡ú community 13_
- `cleanup_temp_singbox()` --calls--> `cleanup_temp_singbox_process()`  [EXTRACTED]
  src\commands\profiles.rs ¡ú src\commands\profiles.rs  _Bridges community 1 ¡ú community 14_
- `release_temp_singbox_test_slot()` --calls--> `cleanup_temp_singbox()`  [EXTRACTED]
  src\commands\profiles.rs ¡ú src\commands\profiles.rs  _Bridges community 13 ¡ú community 14_
- `node_test_latency()` --calls--> `current_latency_test_cancel_token()`  [EXTRACTED]
  src\commands\profiles.rs ¡ú src\commands\profiles.rs  _Bridges community 15 ¡ú community 12_

## Communities

### Community 0 - "Community 0"
Cohesion: 0.08
Nodes (44): build_release_from_version(), builds_release_from_version(), clear_kernel_cache_targets(), clears_actual_cache_db_and_ruleset_cache(), download_kernel_archive_to_path(), extract_kernel_archive(), fetch_release_fallback_from_jsdelivr(), fetch_trusted_remote_releases() (+36 more)

### Community 1 - "Community 1"
Cohesion: 0.08
Nodes (37): cleanup_temp_singbox_process(), cleanup_temp_singbox_process_kills_and_clears_process(), cleanup_temp_singbox_process_succeeds_when_slot_is_empty(), extract_ech_dns_server(), extract_ech_name_and_dns_server(), extract_ech_public_name(), generate_temp_config_adds_dns_for_ech_nodes(), generate_temp_config_raw() (+29 more)

### Community 2 - "Community 2"
Cohesion: 0.06
Nodes (19): custom_rules_get(), custom_rules_save(), domain_rules_get(), domain_rules_save(), load_custom_rules(), save_custom_rules(), AppState, AppSettings (+11 more)

### Community 3 - "Community 3"
Cohesion: 0.08
Nodes (27): bounded_selector_probe_helper_caps_concurrency(), build_dns_server(), build_dns_server_with_resolver(), detect_foreign_wintun_aliases(), get_clash_api_port(), is_valid_ruleset_tag(), NetAdapterRecord, node_bootstrap_signature() (+19 more)

### Community 4 - "Community 4"
Cohesion: 0.09
Nodes (15): build_local_proxy_client(), download_and_verify(), extract_github_path(), get_default_rulesets(), is_valid_ruleset_tag(), load_rulesets(), local_proxy_client_uses_settings_port(), ruleset_cache_path() (+7 more)

### Community 5 - "Community 5"
Cohesion: 0.13
Nodes (25): build_xray_release_from_version(), builds_xray_release_from_version(), converts_exact_github_release_to_plugin_release(), download_archive_to_path(), extract_xray_archive(), fetch_xray_release_by_tag(), fetch_xray_releases(), find_xray_windows_asset() (+17 more)

### Community 6 - "Community 6"
Cohesion: 0.15
Nodes (25): export_node_to_link(), extract_hostname(), fetch_subscription(), is_valid_profile_id(), load_profile_nodes(), load_profile_nodes_raw(), load_profiles_data(), node_add() (+17 more)

### Community 7 - "Community 7"
Cohesion: 0.16
Nodes (18): decode_windows_output(), ensure_u16_in_range(), ensure_u32_in_range(), get_settings(), parse_excluded_tcp_port_ranges(), port_in_ranges(), set_settings(), set_settings_impl() (+10 more)

### Community 8 - "Community 8"
Cohesion: 0.12
Nodes (22): active_or_first_node(), apply_route_target(), collect_referenced_profile_selector_tags(), extract_ech_dns_server_override(), generate_config_with_settings(), is_proxy_type(), is_valid_profile_id(), is_xray_bridge_node() (+14 more)

### Community 9 - "Community 9"
Cohesion: 0.13
Nodes (21): append_startup_diagnostic(), build_foreign_wintun_warning(), clear_proxy_session_marker(), disable_system_proxy_for_state_on_crash(), extract_singbox_fatal_error(), format_startup_failure_message(), get_singbox_path(), is_running_as_admin() (+13 more)

### Community 10 - "Community 10"
Cohesion: 0.18
Nodes (19): clear_persisted_proxy_snapshot(), decode_windows_output(), delete_registry_value(), disable_system_proxy_for_state(), disable_system_proxy_internal(), enable_system_proxy_for_state(), enable_system_proxy_internal(), force_clear_system_proxy() (+11 more)

### Community 11 - "Community 11"
Cohesion: 0.35
Nodes (18): generate_config(), generate_config_bridges_xhttp_nodes_through_local_xray_plugin(), generate_config_forces_strict_tun_route(), generate_config_hijacks_dns_by_protocol_or_port(), generate_config_keeps_proxy_dns_for_non_ech_active_node(), generate_config_preserves_profile_scoped_node_identity_for_domain_rules(), generate_config_routes_xray_plugin_remote_direct_in_tun_mode(), generate_config_scopes_fakedns_to_tun_inbound_when_tun_is_enabled() (+10 more)

### Community 12 - "Community 12"
Cohesion: 0.18
Nodes (16): acquire_temp_singbox_test_slot(), append_latency_diagnostic(), check_clash_api_running(), current_latency_test_settings(), first_temp_singbox_tag(), is_latency_test_batch_cancelled(), map_latency_probe_result(), node_test_all() (+8 more)

### Community 13 - "Community 13"
Cohesion: 0.26
Nodes (13): can_start_temp_singbox(), make_test_state(), release_temp_singbox_test_slot(), stale_release_does_not_decrement_new_batch_temp_slot_owner(), standalone_temp_latency_rejects_concurrent_none_owner(), temp_start_allowed_when_error_and_no_main_process(), temp_start_allowed_when_idle_and_no_main_process(), temp_start_allowed_when_main_process_alive_if_explicitly_permitted() (+5 more)

### Community 14 - "Community 14"
Cohesion: 0.29
Nodes (11): append_temp_latency_logs(), cleanup_temp_singbox(), cleanup_temp_singbox_removes_dir_and_process(), clear_temp_singbox_tag_map(), get_kernel_path_with_fallback(), remove_temp_singbox_dir(), remove_temp_singbox_dir_succeeds_on_nonexistent_path(), removes_temp_singbox_directory_recursively() (+3 more)

### Community 15 - "Community 15"
Cohesion: 0.29
Nodes (10): begin_latency_test_batch(), cancel_and_reset_latency_test_token(), cancel_and_reset_latency_test_token_cancels_existing_waiters(), cancelled_latency_batch_is_tracked_by_run_id(), cancelling_old_batch_does_not_cancel_new_active_batch(), cancelling_old_batch_keeps_new_active_batch_id(), current_latency_test_cancel_token(), mark_latency_test_batch_cancelled() (+2 more)

### Community 16 - "Community 16"
Cohesion: 0.29
Nodes (8): allocate_clash_api_port(), find_available_tcp_port(), find_available_tcp_port_avoiding(), inbound_listen_addr(), reserve_available_tcp_port_avoiding(), reserve_tcp_port(), resolve_available_inbound_ports(), resolve_available_inbound_ports_replaces_unavailable_ports()

### Community 17 - "Community 17"
Cohesion: 0.29
Nodes (8): build_xray_plugin_config(), build_xray_plugin_config_hoists_legacy_xhttp_extra_encryption(), build_xray_plugin_config_preserves_vless_xhttp_transport(), make_xhttp_node(), plugin_bridge_path(), start_plugin_bridges(), vless_encryption(), xray_plugin_path()

### Community 18 - "Community 18"
Cohesion: 0.67
Nodes (6): append_startup_diagnostic(), get_data_dir(), read_settings_sync(), run(), setup_tray(), spawn_safe_exit()

### Community 19 - "Community 19"
Cohesion: 1.0
Nodes (0): 

### Community 20 - "Community 20"
Cohesion: 1.0
Nodes (0): 

## Knowledge Gaps
- **30 isolated node(s):** `ProxyState`, `TrafficStats`, `Profile`, `SingBoxOutbound`, `RuleSet` (+25 more)
  These have ¡Ü1 connection - possible missing edges or undocumented components.
- **Thin community `Community 19`** (2 nodes): `build.rs`, `main()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 20`** (2 nodes): `main.rs`, `main()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **What connects `ProxyState`, `TrafficStats`, `Profile` to the rest of the system?**
  _30 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 0` be split into smaller, more focused modules?**
  _Cohesion score 0.08 - nodes in this community are weakly interconnected._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.08 - nodes in this community are weakly interconnected._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.06 - nodes in this community are weakly interconnected._
- **Should `Community 3` be split into smaller, more focused modules?**
  _Cohesion score 0.08 - nodes in this community are weakly interconnected._
- **Should `Community 4` be split into smaller, more focused modules?**
  _Cohesion score 0.09 - nodes in this community are weakly interconnected._
- **Should `Community 5` be split into smaller, more focused modules?**
  _Cohesion score 0.13 - nodes in this community are weakly interconnected._