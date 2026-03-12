# SwiftShare

> 局域网零配置极速文件传输工具

SwiftShare 是一款专为局域网设计的点对点文件传输应用。无需服务器、无需注册、无需手动填写 IP——打开即用，拖入即传。基于 mDNS 自动发现同局域网内的设备，通过自研的 TCP 二进制协议高效传输文件，关闭即离线，隐私优先。

---

## 功能特性

### 零配置设备发现
基于 mDNS/DNS-SD（`_swiftshare._tcp.local.`）协议，启动后自动广播并发现局域网内的其他 SwiftShare 实例，无需手动输入 IP 地址。发现新设备时主动重新广播自身，加快双向发现速度。

### 快速下线感知
采用双重机制确保设备列表实时准确：
- **主动 TCP 探活**：每 5 秒对已知设备发起连接探测，约 10 秒内感知离线
- **Goodbye 包**：正常关闭程序时向所有已知设备发送离线通知，对方立即移除

### 双向传输模式
- **Push 模式**：直接将文件发送到目标设备
- **Pull 模式**：将文件添加到本地共享索引，对方按需拉取，支持拖出到文件夹

### 智能分片传输
- 小文件（< 1 MB）：单包流式传输，延迟极低
- 大文件：分块传输，支持**断点续传**，中断后可从上次进度继续

### 连接池复用
TCP 连接按 `ip:port` 池化管理，跨传输任务复用，避免频繁握手开销。

### 传输可靠性保障
- 连接失败自动重试（最多 5 次，线性退避）
- 源文件变更检测，拒绝传输已修改的文件，确保数据一致性

### 多文件与目录支持
递归遍历目录结构，支持整个文件夹拖入共享，批量传输。

### 实时进度追踪
基于事件的进度推送，前端实时展示传输进度。

### 毛玻璃暗色界面
Windows Acrylic 透明效果，无边框自定义窗口，现代化 Glassmorphism 设计风格。

---

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
| 设备发现 | mdns-sd 0.11（mDNS/DNS-SD） |
| 网络传输 | 自研 TCP 二进制协议 |
| 包管理 | pnpm（前端）/ Cargo（Rust） |

---

## 协议设计

SwiftShare 使用自研的二进制协议进行文件传输，协议头固定 24 字节：

```
 0       4     6       10              18        24
 +-------+-----+-------+---------------+---------+
 | SWFT  | Type| Rsrvd |    Offset     | PayLen  |
 +-------+-----+-------+---------------+---------+
   Magic  u16BE  4bytes    u64 BE       6bytes BE
```

### 数据包类型（共 9 种）

| 类型值 | 名称 | 说明 |
|--------|------|------|
| 1 | `FileMeta` | 文件元信息（路径、大小、续传偏移） |
| 2 | `FileChunk` | 大文件分块数据 |
| 3 | `SmallFileStream` | 小文件流式传输（含元信息） |
| 4 | `ListRequest` | 请求对端共享文件列表 |
| 5 | `ListResponse` | 返回共享文件列表（JSON） |
| 6 | `PullRequest` | 请求拉取指定文件 |
| 7 | `PullStream` | 文件流响应 |
| 8 | `Error` | 错误信息 |
| 9 | `Goodbye` | 离线通知，收到后立即断开连接 |

---

## 前置要求

- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install)（stable）
- [Tauri v2 依赖](https://v2.tauri.app/start/prerequisites/)

---

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

### 多实例本机调试

项目提供了两个 PowerShell 脚本，可在同一台机器上同时启动两个实例，方便测试点对点传输：

```powershell
# 终端 1
.un-dev-1420.ps1

# 终端 2（新开一个终端）
.un-dev-1421.ps1
```

两个实例使用独立的 Cargo 编译缓存目录（`target-1420` / `target-1421`），互不干扰。启动后两个窗口会自动发现对方并显示在「在线设备」列表中。

### 构建生产版本

```bash
pnpm tauri build
```

---

## 项目结构

```
SwiftShare/
├── src/                    # 前端源码（React + TypeScript）
│   ├── App.tsx             # 主界面组件
│   ├── App.css             # 样式（Tailwind + 自定义毛玻璃样式）
│   ├── main.tsx            # React 入口
│   └── assets/             # 静态资源
├── src-tauri/              # Rust 后端（Tauri）
│   ├── src/
│   │   ├── lib.rs          # Tauri 命令注册与应用初始化
│   │   ├── discovery.rs    # mDNS 设备发现与探活
│   │   ├── transport.rs    # TCP 文件传输引擎
│   │   └── main.rs         # 二进制入口
│   ├── capabilities/       # Tauri v2 权限配置
│   ├── icons/              # 应用图标
│   ├── Cargo.toml          # Rust 依赖配置
│   └── tauri.conf.json     # Tauri 应用配置
├── public/                 # 静态公共资源
├── run-dev-1420.ps1        # 多实例调试脚本（实例 1）
├── run-dev-1421.ps1        # 多实例调试脚本（实例 2）
├── package.json            # 前端依赖与脚本
├── vite.config.ts          # Vite 构建配置
└── tsconfig.json           # TypeScript 配置
```

---

## 推荐 IDE 配置

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

---

## 许可证

本项目基于 [Apache License 2.0](LICENSE) 开源。
