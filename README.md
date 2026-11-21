# KunBox for Windows

<p align="center">
  <img src="kunbox-electron/build/icon.png" alt="KunBox Logo" width="128" height="128">
</p>

<p align="center">
  A modern, cross-platform sing-box proxy client built with Electron, React, and TypeScript.
</p>

## Features

- **Profile Management** - Import and manage subscription profiles with auto-update support
- **Node Management** - View, filter, and test proxy nodes with latency testing
- **Rule Sets** - Drag-and-drop rule management with built-in rule hub (geosite/geoip)
- **Domain Routing** - Smart domain-based traffic routing with automatic type detection
- **Process Routing** - Application-specific routing rules (requires TUN mode)
- **TUN Mode** - System-wide traffic capture for complete proxy coverage
- **Theme Support** - Dark/Light theme with smooth transition animations
- **Real-time Logs** - Live connection logging with filtering

## Tech Stack

- **Framework**: Electron 45.x + Node.js 22+
- **Packaging**: electron-builder 26.x
- **Frontend**: React 19 + TypeScript 6 + Vite 8 + electron-vite 6
- **Styling**: Tailwind CSS 4 (@tailwindcss/vite)
- **UI Components**: Radix UI
- **State Management**: Zustand 5
- **Animations**: Framer Motion 14

## Project Structure

```
KunBox-Windows/
├── kunbox-electron/        # Core App (Electron + React)
│   ├── src/
│   │   ├── main/           # Electron main process (sing-box, IPC, system proxy, kernel)
│   │   ├── preload/        # Preload script (window.api bridge)
│   │   ├── renderer/       # React frontend
│   │   │   ├── components/ # UI components
│   │   │   ├── stores/     # Zustand stores
│   │   │   └── styles/     # Global styles
│   │   └── shared/         # Shared types and constants
│   ├── resources/          # Kernel resources and lock files
│   ├── electron.vite.config.ts
│   ├── electron-builder.yml
│   └── package.json
├── scripts/                # Build and kernel helper scripts
├── build.bat               # Windows batch build script
└── build.ps1               # PowerShell build script
```

## Getting Started

### Prerequisites

- Node.js 20+
- npm

### Installation

```bash
cd kunbox-electron
npm install
```

### Development

```bash
cd kunbox-electron
npm run dev
```

### Build

```powershell
# Build for production (from project root)
.\build.ps1
```

Or inside `kunbox-electron/`:

```powershell
npm run build           # Build main, preload, and renderer
npm run build:win       # Package Windows NSIS installer and portable build
npm run build:portable  # Package Windows portable build
npm run typecheck       # TypeScript type check
npm run lint            # ESLint check
```

## Routing Modes

| Mode | Description |
|------|-------------|
| Direct | Bypass proxy, connect directly |
| Proxy | Route through proxy server |
| Block | Block the connection |
| Node | Route to specific node |
| Profile | Route to specific profile |

## Configuration

Settings and runtime data are stored in `D:\KunBoxData` (fallback `%APPDATA%\KunBox` when drive `D:` is unavailable) and include:

- **Proxy Settings**: HTTP/SOCKS port, LAN access
- **TUN Settings**: Enable TUN mode, network stack selection
- **DNS Settings**: Local/Remote DNS, FakeDNS
- **System Settings**: Auto-start, minimize to tray, theme

## License

MIT License

## Acknowledgments

- [sing-box](https://github.com/SagerNet/sing-box) - The universal proxy platform
- [sing-geosite](https://github.com/SagerNet/sing-geosite) - GeoSite rule sets
- [sing-geoip](https://github.com/SagerNet/sing-geoip) - GeoIP rule sets
- [Electron](https://www.electronjs.org/) - Cross-platform desktop applications with web technologies
