# KunBox for Windows

<p align="center">
  <img src="kunbox-electron/build/icon.png" alt="KunBox Logo" width="128" height="128">
</p>

<p align="center">
  A modern, cross-platform sing-box proxy client built with Tauri, React, and TypeScript.
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

- **Framework**: Tauri 2.x (Rust backend)
- **Frontend**: React 18 + TypeScript + Vite
- **Styling**: Tailwind CSS
- **UI Components**: Radix UI
- **State Management**: Zustand
- **Animations**: Framer Motion

## Project Structure

```
KunBox-Windows/
├── kunbox-electron/        # Frontend (React + Vite)
│   ├── src/
│   │   ├── renderer/       # React frontend
│   │   │   ├── components/ # UI components
│   │   │   ├── stores/     # Zustand stores
│   │   │   └── styles/     # Global styles
│   │   └── shared/         # Shared types
│   └── package.json
├── src-tauri/              # Backend (Rust + Tauri)
│   ├── src/
│   │   ├── commands/       # Tauri commands
│   │   ├── state.rs        # App state
│   │   ├── types.rs        # Type definitions
│   │   └── lib.rs          # Entry point
│   └── Cargo.toml
└── build.ps1               # Build script
```

## Getting Started

### Prerequisites

- Node.js 18+
- Rust 1.77.2+
- npm or yarn

### Installation

```bash
# Install frontend dependencies
cd kunbox-electron
npm install

# Check Rust backend
cd ../src-tauri
cargo check
```

### Development

```bash
# Terminal 1: Start frontend dev server
cd kunbox-electron
npm run dev

# Terminal 2: Start Tauri dev mode
cd src-tauri
cargo tauri dev
```

### Build

```powershell
# Build for production (from project root)
.\build.ps1
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

Settings are stored in `%APPDATA%\KunBox` and include:

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
- [Tauri](https://tauri.app/) - Build smaller, faster, and more secure desktop applications
