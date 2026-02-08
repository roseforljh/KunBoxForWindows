# KunBox-Windows 项目指南

## 环境说明

**重要**: 此项目在 Windows PowerShell 环境下运行。

### 命令执行规范

PowerShell 不支持 `&&` 命令连接符，请使用以下方式：

```powershell
# 正确方式 1: 使用 cmd /c 包装
cmd /c "cd /d C:\Users\33039\Desktop\KunBox-Windows\kunbox-electron && npm run build"

# 正确方式 2: 使用分号分隔（但前一条失败不会阻止后一条）
cd C:\path\to\project; npm run build

# 错误方式（PowerShell 不支持）
cd C:\path\to\project && npm run build
```

## 项目结构

```
KunBox-Windows/
├── kunbox-electron/    # 前端 - React + Vite
│   ├── src/
│   │   ├── renderer/   # React 渲染层
│   │   └── shared/     # 共享类型和 API
│   └── package.json
├── src-tauri/          # 后端 - Rust + Tauri
│   ├── src/
│   │   ├── commands/   # Tauri 命令
│   │   ├── state.rs    # 应用状态
│   │   ├── types.rs    # 类型定义
│   │   └── lib.rs      # 入口点
│   └── Cargo.toml
└── build.ps1           # 构建脚本
```

## 技术栈

- **后端**: Tauri 2.x + Rust
- **前端**: React 18 + TypeScript + Vite
- **样式**: Tailwind CSS
- **组件库**: Radix UI
- **状态管理**: Zustand
- **动画**: Framer Motion

## 常用命令

### 前端 (kunbox-electron 目录)

```powershell
npm install      # 安装依赖
npm run dev      # 开发模式
npm run build    # 构建
npm run typecheck # 类型检查
npm run lint     # 代码检查
```

### 后端 (src-tauri 目录)

```powershell
cargo check      # 检查编译
cargo test       # 运行测试
cargo tauri dev  # Tauri 开发模式
cargo tauri build # 构建发布版
```

### 完整开发流程

1. 在 `kunbox-electron/` 运行 `npm install` 安装前端依赖
2. 在 `kunbox-electron/` 运行 `npm run dev` 启动 Vite 开发服务器
3. 在 `src-tauri/` 运行 `cargo tauri dev` 启动 Tauri 开发模式

## 开发约定

1. 遵循现有代码风格
2. 使用 TypeScript 严格模式
3. 组件使用函数式组件 + Hooks
4. 样式优先使用 Tailwind CSS
5. 状态管理使用 Zustand
6. 前后端 API 变更需同步更新两侧
