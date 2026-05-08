# Graph Report - .  (2026-04-29)

## Corpus Check
- 51 files ， ~57,197 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 483 nodes ， 886 edges ， 18 communities detected
- Extraction: 100% EXTRACTED ， 0% INFERRED ， 0% AMBIGUOUS
- Token cost: 0 input ， 0 output

## God Nodes (most connected - your core abstractions)
1. `generate_config()` - 21 edges
2. `load_profiles_data()` - 18 edges
3. `singbox_start_impl()` - 17 edges
4. `start_temp_singbox()` - 13 edges
5. `kernel_download()` - 11 edges
6. `cleanup_temp_singbox()` - 11 edges
7. `save_profiles_data()` - 11 edges
8. `node_test_latency()` - 11 edges
9. `node_test_all()` - 11 edges
10. `make_test_state()` - 11 edges

## Surprising Connections (you probably didn't know these)
- None detected - all connections are within the same source files.

## Communities

### Community 0 - "Community 0"
Cohesion: 0.04
Nodes (108): acquire_temp_singbox_test_slot(), begin_latency_test_batch(), can_start_temp_singbox(), cancel_and_reset_latency_test_token(), cancel_and_reset_latency_test_token_cancels_existing_waiters(), cancelled_latency_batch_is_tracked_by_run_id(), cancelling_old_batch_does_not_cancel_new_active_batch(), cancelling_old_batch_keeps_new_active_batch_id() (+100 more)

### Community 1 - "Community 1"
Cohesion: 0.02
Nodes (21): handleAdd(), handleClose(), applyTheme(), handler(), restartIfConnected(), sleep(), createAreaPath(), createPath() (+13 more)

### Community 2 - "Community 2"
Cohesion: 0.05
Nodes (84): active_or_first_node(), allocate_clash_api_port(), append_startup_diagnostic(), apply_route_target(), bounded_selector_probe_helper_caps_concurrency(), build_dns_server(), build_dns_server_with_resolver(), clear_persisted_proxy_snapshot() (+76 more)

### Community 3 - "Community 3"
Cohesion: 0.08
Nodes (44): build_release_from_version(), builds_release_from_version(), clear_kernel_cache_targets(), clears_actual_cache_db_and_ruleset_cache(), download_kernel_archive_to_path(), extract_kernel_archive(), fetch_release_fallback_from_jsdelivr(), fetch_trusted_remote_releases() (+36 more)

### Community 4 - "Community 4"
Cohesion: 0.09
Nodes (16): build_local_proxy_client(), download_and_verify(), extract_github_path(), get_default_rulesets(), getModeLabel(), getOutboundValueDisplay(), is_valid_ruleset_tag(), load_rulesets() (+8 more)

### Community 5 - "Community 5"
Cohesion: 0.08
Nodes (8): custom_rules_get(), custom_rules_save(), domain_rules_get(), domain_rules_save(), load_custom_rules(), save_custom_rules(), AppState, UpdateInfo

### Community 6 - "Community 6"
Cohesion: 0.14
Nodes (10): decode_windows_output(), ensure_u16_in_range(), ensure_u32_in_range(), set_settings(), set_settings_impl(), set_settings_keeps_startup_side_effect_when_persist_succeeds(), set_settings_rolls_back_startup_side_effect_when_persist_fails(), set_windows_startup() (+2 more)

### Community 7 - "Community 7"
Cohesion: 0.25
Nodes (1): ErrorBoundary

### Community 8 - "Community 8"
Cohesion: 0.62
Nodes (6): build_icon_nodes(), build_shared_doc_graph(), edge(), load_uncached_files(), main(), node()

### Community 9 - "Community 9"
Cohesion: 0.67
Nodes (6): append_startup_diagnostic(), get_data_dir(), read_settings_sync(), run(), setup_tray(), spawn_safe_exit()

### Community 10 - "Community 10"
Cohesion: 0.5
Nodes (2): handleClose(), handleImport()

### Community 11 - "Community 11"
Cohesion: 1.0
Nodes (0): 

### Community 12 - "Community 12"
Cohesion: 1.0
Nodes (0): 

### Community 13 - "Community 13"
Cohesion: 1.0
Nodes (0): 

### Community 14 - "Community 14"
Cohesion: 1.0
Nodes (0): 

### Community 15 - "Community 15"
Cohesion: 1.0
Nodes (0): 

### Community 16 - "Community 16"
Cohesion: 1.0
Nodes (0): 

### Community 17 - "Community 17"
Cohesion: 1.0
Nodes (0): 

## Knowledge Gaps
- **23 isolated node(s):** `ProxyState`, `TrafficStats`, `Profile`, `SingBoxOutbound`, `RuleSet` (+18 more)
  These have ＋1 connection - possible missing edges or undocumented components.
- **Thin community `Community 11`** (2 nodes): `build.rs`, `main()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 12`** (1 nodes): `ast_extract.py`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 13`** (1 nodes): `postcss.config.js`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 14`** (1 nodes): `tailwind.config.js`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 15`** (1 nodes): `vite.config.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 16`** (1 nodes): `global.d.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 17`** (1 nodes): `constants.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **What connects `ProxyState`, `TrafficStats`, `Profile` to the rest of the system?**
  _23 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 0` be split into smaller, more focused modules?**
  _Cohesion score 0.04 - nodes in this community are weakly interconnected._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.02 - nodes in this community are weakly interconnected._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.05 - nodes in this community are weakly interconnected._
- **Should `Community 3` be split into smaller, more focused modules?**
  _Cohesion score 0.08 - nodes in this community are weakly interconnected._
- **Should `Community 4` be split into smaller, more focused modules?**
  _Cohesion score 0.09 - nodes in this community are weakly interconnected._
- **Should `Community 5` be split into smaller, more focused modules?**
  _Cohesion score 0.08 - nodes in this community are weakly interconnected._