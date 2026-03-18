[English](./README.md) | [中文](./README.zh-CN.md)

# SwiftShare ⚡

> Zero-config LAN file transfer tool for high-speed P2P sharing

SwiftShare is a peer-to-peer file transfer application designed specifically for local area networks. No servers, no registration, and no manual IP entries required—just open and share. It automatically discovers devices on the same network via mDNS and transfers files efficiently through a custom TCP binary protocol.

---

## Feature Highlights ✨

- 📡 **Zero-config Device Discovery**: Uses mDNS/DNS-SD to find peers automatically.
- 🖱️ **Drag-and-Drop Sharing**: Simple interface for sending files and folders.
- 🔄 **Pull & Drag-out Mode**: Share files locally and let others pull them on demand.
- 📁 **Directory Browsing**: Navigate shared folders with breadcrumb support.
- 📈 **Transfer Progress & History**: Real-time tracking of active and past transfers.
- 🛡️ **Conflict Detection**: Prevents accidental overwrites by checking file existence.
- 🧩 **Resumable Transfers**: Supports resuming large file transfers from where they left off.
- 🌌 **Glassmorphism Dark UI**: Modern Windows Acrylic effect with a sleek dark theme.

---

## Tech Stack 🛠️

| Layer | Technology |
|-------|------------|
| Desktop | Tauri v2 |
| Frontend | React 19 + TypeScript 5.8 |
| Build | Vite 7 |
| Styling | Tailwind CSS v4 |
| Animation | Framer Motion |
| Backend | Rust (Edition 2021) |
| Runtime | Tokio |
| Discovery | mdns-sd (mDNS/DNS-SD) |

---

## Quick Start 🚀

### Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [Tauri v2 dependencies](https://v2.tauri.app/start/prerequisites/)

### Installation & Run

```bash
# Clone the repository
git clone git@github.com:RSJWY/SwiftShare.git
cd SwiftShare

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev
```

### Build Production Version

```bash
pnpm tauri build
```

---

## Documentation 📖

- 📖 [Usage Guide](./docs/usage.md) - How to use SwiftShare
- ⚙️ [Settings Reference](./docs/settings.md) - Configuration options
- 🔧 [Troubleshooting](./docs/troubleshooting.md) - Common issues & solutions

---

## Multi-instance Debugging 🧪

For local testing of P2P transfers on a single machine, use the provided PowerShell scripts to launch two independent instances:

```powershell
# Terminal 1
./run-dev-1420.ps1

# Terminal 2
./run-dev-1421.ps1
```

Each instance uses a dedicated Cargo target directory (`target-1420` / `target-1421`) to avoid build lock conflicts.

---

## Project Structure 🧭

```
SwiftShare/
├── docs/                   # Documentation (Usage, Settings, etc.)
├── src/                    # Frontend source (React + TypeScript)
│   ├── App.tsx             # Main UI components
│   ├── main.tsx            # Entry point
│   └── assets/             # Images and styles
├── src-tauri/              # Backend source (Rust + Tauri)
│   ├── src/
│   │   ├── discovery.rs    # mDNS & Peer management
│   │   └── transport.rs    # TCP Transfer engine
│   ├── Cargo.toml          # Rust dependencies
│   └── tauri.conf.json     # Tauri configuration
├── run-dev-1420.ps1        # Debug script for instance 1
├── run-dev-1421.ps1        # Debug script for instance 2
├── package.json            # Frontend dependencies
└── vite.config.ts          # Build configuration
```

---

## License 📄

This project is licensed under the [Apache License 2.0](LICENSE).
