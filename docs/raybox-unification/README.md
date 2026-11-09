# RayBox 统一迁移总计划

KunBoxForWindows 当前通过外部 `sing-box.exe` 进程运行核心，并用独立 `xray.exe` bridge 支持 XHTTP。目标是改为消费 `D:\RayBox-rx` 发布的 RayBox Windows 原生核心，原 bridge 仅作为迁移期间回滚路径。

## 必须建立

- RayBox Windows x64（后续按需求支持 arm64）不可变 release。
- PE 架构、版本、完整 commit 前缀、文件大小、SHA-256、依赖 DLL、签名和 manifest 校验。
- `kernel.rs` 的下载、安装、备份、回滚和本地版本读取适配 RayBox lock。
- `singbox.rs` 的配置生成优先使用 RayBox 原生 `type: "xray"` outbound；原有 plugin bridge 保留至原生路径验收完成。
- 前端节点编辑器与共享类型统一 `core`、XHTTP、Encryption、RX、OOB 字段和默认关闭策略。

## 不得丢失的现有行为

启动/停止/重启、状态/流量/日志、延迟测试、TUN/Wintun、DNS、系统代理快照恢复、控制面回环绑定、管理员检查、Wintun 冲突处理、敏感日志脱敏、临时凭据清理和孤儿进程清理。

## 硬阻塞

当前资源目录没有 RayBox Windows EXE、下载 URL、稳定 ABI 或依赖 DLL。未建立并验收原生工件链前，不宣称 Windows 已完成统一迁移。

详细进度见 `STATUS.md`。
