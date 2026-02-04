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
├── kunbox-electron/    # 主项目 - KunBox sing-box 代理客户端
```

## 主项目: kunbox-electron

KunBox 是一个跨平台的 sing-box 代理客户端。

### 技术栈
- Electron + Vite
- React 18 + TypeScript
- Tailwind CSS
- Radix UI 组件库
- Zustand 状态管理
- Framer Motion 动画

### 目录结构
```
src/
├── main/       # Electron 主进程
├── preload/    # 预加载脚本
├── renderer/   # React 渲染进程
└── shared/     # 共享代码
```

### 常用命令

在 kunbox-electron 目录下执行：

```powershell
# 使用 cmd /c 包装以支持 && 语法
cmd /c "cd /d C:\Users\33039\Desktop\KunBox-Windows\kunbox-electron && npm run dev"
cmd /c "cd /d C:\Users\33039\Desktop\KunBox-Windows\kunbox-electron && npm run build"
cmd /c "cd /d C:\Users\33039\Desktop\KunBox-Windows\kunbox-electron && npm run typecheck"
cmd /c "cd /d C:\Users\33039\Desktop\KunBox-Windows\kunbox-electron && npm run lint"
```

可用脚本：
- `npm run dev` - 开发模式
- `npm run build` - 构建
- `npm run build:win` - 构建 Windows 版本
- `npm run lint` - 代码检查
- `npm run typecheck` - 类型检查

## 参考项目: LinJun

**用途**: 前端 UI/UX 设计参考，该项目的前端界面设计很好看。

当需要设计新的 UI 组件或页面时，可以参考 LinJun 项目的实现：
- 组件样式: `LinJun/src/renderer/components/`
- 页面布局: `LinJun/src/renderer/`
- Tailwind 配置: `LinJun/tailwind.config.js`

## 开发约定

1. 遵循现有代码风格
2. 使用 TypeScript 严格模式
3. 组件使用函数式组件 + Hooks
4. 样式优先使用 Tailwind CSS
5. 状态管理使用 Zustand
