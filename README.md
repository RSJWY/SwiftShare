# SwiftShare

局域网零配置极速文件传输工具。基于 mDNS 自动发现局域网设备，通过自定义 TCP 二进制协议高效传输文件，无需任何服务器或手动配置。

## 功能特性

- **零配置设备发现** — 基于 mDNS/DNS-SD（`_swiftshare._tcp.local.`）自动发现局域网内的设备，无需手动输入 IP 地址
- **双向传输模式** — 支持 Push（直接发送）和 Pull（共享索引 + 按需拉取）两种文件传输方式
- **智能分片传输** — 小文件（<1MB）采用单包流式传输；大文件采用分块传输，支持断点续传
- **连接池复用** — TCP 连接按目标 IP 池化管理，跨传输任务复用，减少连接开销
- **自动重连** — 连接失败时自动重试（最多 5 次，线性退避），保障传输可靠性
- **文件完整性校验** — 传输过程中监控源文件变更，确保数据一致性
- **多文件与目录支持** — 递归遍历目录结构，支持批量文件传输
- **实时进度追踪** — 基于事件的进度推送，前端动画进度条实时展示传输状态
- **毛玻璃暗色界面** — Windows Acrylic 透明效果，无边框自定义窗口，现代化 Glassmorphism 设计风格

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri v2 |
| 前端 | React 19 + TypeScript 5.8 |
| 构建工具 | Vite 7 |
| 样式 | Tailwind CSS v4 |
| 动画 | Framer Motion |
| 后端 | Rust (Edition 2021) |
| 异步运行时 | Tokio |
| 设备发现 | mdns-sd (mDNS/DNS-SD) |
| 网络传输 | 自定义 TCP 二进制协议 |
| 包管理 | pnpm (前端) / Cargo (Rust) |

## 协议设计

SwiftShare 使用自定义的二进制协议进行文件传输，协议头固定 24 字节：

```
 0       4     6       10              18        24
 +-------+-----+-------+---------------+---------+
 | SWFT  | Type| Rsrvd |    Offset     | PayLen  |
 +-------+-----+-------+---------------+---------+
   Magic  u16BE  4bytes    u64 BE       6bytes BE
```

支持 8 种数据包类型：`FileMeta`、`FileChunk`、`SmallFileStream`、`ListRequest`、`ListResponse`、`PullRequest`、`PullStream`、`Error`

## 前置要求

- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [Tauri v2 依赖](https://v2.tauri.app/start/prerequisites/)

## 快速开始

```bash
# 克隆项目
git clone git@github.com:RSJWY/SwiftShare.git
cd SwiftShare

# 安装前端依赖
pnpm install

# 开发模式运行
pnpm tauri dev
```

### 多实例调试

项目提供了 PowerShell 脚本，支持在同一台机器上同时运行多个实例，方便测试点对点传输：

```powershell
.\run-dev-auto.ps1
```

该脚本会自动分配随机端口并隔离 Cargo 编译缓存，避免端口冲突。

### 构建生产版本

```bash
pnpm tauri build
```

## 项目结构

```
SwiftShare/
├── src/                    # 前端源码 (React + TypeScript)
│   ├── App.tsx             # 主界面组件
│   ├── App.css             # 样式 (Tailwind + 自定义毛玻璃样式)
│   ├── main.tsx            # React 入口
│   └── assets/             # 静态资源
├── src-tauri/              # Rust 后端 (Tauri)
│   ├── src/
│   │   ├── lib.rs          # Tauri 命令注册与应用初始化
│   │   ├── discovery.rs    # mDNS 设备发现
│   │   ├── transport.rs    # TCP 文件传输引擎
│   │   └── main.rs         # 二进制入口
│   ├── capabilities/       # Tauri v2 权限配置
│   ├── icons/              # 应用图标
│   ├── Cargo.toml          # Rust 依赖配置
│   └── tauri.conf.json     # Tauri 应用配置
├── public/                 # 静态公共资源
├── run-dev-auto.ps1        # 多实例调试脚本
├── package.json            # 前端依赖与脚本
├── vite.config.ts          # Vite 构建配置
└── tsconfig.json           # TypeScript 配置
```

## 推荐 IDE 配置

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 许可证

本项目基于 [Apache License 2.0](LICENSE) 开源。
