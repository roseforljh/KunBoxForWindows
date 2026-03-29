# KunBox 代码审计报告

**审计日期**: 2026-03-29

---

## 1. DNS路由一致性 (Routing Correctness)

**状态**: ✅ 已正确实现

DNS规则按outbound_mode正确映射：direct->dns-local, proxy->dns-remote。

代码位置: singbox.rs lines 722-796, 800-836

---

## 2. 选择器Tag冲突 (Selector Uniqueness)

**状态**: ⚠️ 需添加防御性检查

PROXY和auto selector无existing_tags检查，可能与节点tag冲突。

代码位置: singbox.rs lines 1011-1034

---

## 3. 临时进程清理 (Temp Singbox Lifecycle)

**状态**: ⚠️ 建议显式清理

temp_test目录未删除，进程kill_on_drop依赖OwnedChild drop。

代码位置: profiles.rs lines 1265-1376

---

## 4. 延迟测试并发 (Over-concurrent Latency Tests)

**状态**: ✅ 已正确控制

chunk_size=5，并发可控。

---

## 5. 缓存清理路径错误 (Wrong Cache Clear Path)

**状态**: 🔴 需修复

kernel_clear_cache清理data_dir/cache，但sing-box缓存在data_dir/cache.db。

代码位置: kernel.rs lines 651-662, singbox.rs lines 916-919

---

## 6. 下载内存峰值 (Kernel Download Memory Spike)

**状态**: ⚠️ 建议流式处理

整个100MB文件加载到内存，应流式下载到临时文件。

代码位置: kernel.rs lines 530-553

---

## 7. 前端写放大 (Frontend Write Amplification)

**状态**: ⚠️ 建议debounce

testAllLatency每次更新都触发persist写入。

代码位置: nodesStore.ts lines 268-275

---

## 8. 缓存过期 (Stale Persisted Latency Cache)

**状态**: ⚠️ 需添加TTL检查

latencyCache.timestamp未用于过期判断。

代码位置: nodesStore.ts lines 24-31, 226-250

---

## 9. 日志堆积 (Log Burst Memory)

**状态**: ⚠️ 建议添加上限

pendingLogsRef无上限，可能在刷新延迟时堆积。

代码位置: Logs.tsx lines 47-55

---

## 10. 状态回滚 (Optimistic Settings State Drift)

**状态**: ⚠️ 需添加catch回滚

updateSetting在API失败时未回滚UI状态（requireAdmin除外）。

代码位置: Settings.tsx lines 42-73


**最优先修复**: #5 (缓存清理无效)
