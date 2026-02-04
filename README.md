# KunBox for Windows

A modern, cross-platform sing-box proxy client built with Electron, React, and TypeScript.

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

- **Framework**: Electron + Vite
- **Frontend**: React 18 + TypeScript
- **Styling**: Tailwind CSS
- **UI Components**: Radix UI
- **State Management**: Zustand
- **Animations**: Framer Motion

## Project Structure

```
kunbox-electron/
├── src/
│   ├── main/           # Electron main process
│   │   ├── ipc/        # IPC handlers
│   │   └── utils/      # Utilities
│   ├── preload/        # Preload scripts
│   ├── renderer/       # React frontend
│   │   ├── components/ # UI components
│   │   ├── stores/     # Zustand stores
│   │   └── styles/     # Global styles
│   └── shared/         # Shared types and constants
├── electron-builder.yml
└── package.json
```

## Getting Started

### Prerequisites

- Node.js 18+
- npm or yarn

### Installation

```bash
cd kunbox-electron
npm install
```

### Development

```bash
npm run dev
```

### Build

```bash
# Build for production
npm run build

# Build Windows installer
npm run build:win
```

## Screenshots

*Coming soon*

## Routing Modes

| Mode | Description |
|------|-------------|
| Direct | Bypass proxy, connect directly |
| Proxy | Route through proxy server |
| Block | Block the connection |
| Node | Route to specific node |
| Profile | Route to specific profile |

## Configuration

Settings are stored locally and include:

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
