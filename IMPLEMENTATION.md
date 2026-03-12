# SwiftShare 设备识别改进 - 实现细节

## 问题陈述

当你的电脑上有多张网卡（物理网卡、虚拟网卡等）时，SwiftShare 在设备发现时会将同一个程序实例显示为多个设备。这导致：
1. 设备列表混乱，难以识别实际有多少台电脑
2. 无法清晰看到完整的设备名
3. 用户体验不佳

## 解决方案架构

### 1. 机器唯一标识（Machine ID）

**生成机制**：
```rust
// 在应用启动时执行一次
pub async fn generate_machine_id() -> String {
    // 尝试读取已存在的 ID
    if let Ok(content) = tokio::fs::read_to_string(&id_file).await {
        if !content.trim().is_empty() {
            return content.trim().to_string();
        }
    }

    // 生成新的 UUID
    let new_id = Uuid::new_v4().to_string();

    // 持久化保存
    let _ = tokio::fs::write(&id_file, &new_id).await;

    new_id
}
```

**存储位置**（按操作系统）：
- Windows: `%APPDATA%/SwiftShare/.machine_id`
- macOS: `~/Library/Application Support/SwiftShare/.machine_id`
- Linux: `~/.local/share/swiftshare/.machine_id`

**特点**：
- ✅ 重启后保持一致（通过持久化）
- ✅ 跨同一机器的多个实例共享（都读取同一个文件）
- ✅ 用户可以手动删除文件来重置

### 2. mDNS 服务名格式改进

**旧格式**（问题所在）：
```
hostname::port:interface_name
例：DESKTOP-ABC::7878:Ethernet
   DESKTOP-ABC::7878:WiFi
   DESKTOP-ABC::7878:VirtualBox
```
这样导致同一个程序有多个服务名。

**新格式**（解决方案）：
```
uuid[hostname]
例：a1b2c3d4-e5f6-4789-0123[DESKTOP-ABC]
```
所有网卡上注册相同的服务名，mDNS 会自动聚合它们的地址。

### 3. 服务注册逻辑

在 `discovery.rs` 的 `start()` 函数中：

```rust
// 为每个非虚拟网卡注册服务
for iface in &interfaces {
    if is_virtual_interface(&iface.name) {
        continue;  // 跳过虚拟网卡
    }

    // 所有网卡使用相同的服务名！
    let service_name = format!("{}[{}]", machine_id, hostname);

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &service_name,           // 关键：相同的名称
        &host_label,
        iface.ip,               // 不同的 IP
        transport.port,
        HashMap::new(),
    )?;
    daemon.register(service)?;
}
```

### 4. 发现和聚合逻辑

**内部数据结构**：
```rust
// 存储格式：machine_id:port -> (addresses, machine_id, port, hostname)
HashMap<String, (Vec<IpAddr>, String, u16, String)>
```

**聚合过程**：
1. 当 mDNS 返回 `ServiceResolved` 事件时，会包含所有注册的 IP 地址
2. 使用 `machine_id:port` 作为唯一键存储这些地址
3. 重复发现同一个服务时，直接更新地址列表（不会重复）

**IP 优先级选择**：
```rust
fn emit_devices(...) {
    for (_, (addrs, machine_id, port, hostname)) in services {
        let mut best_addr = None;

        // 1. 优先选择非本地的可路由地址（表示远程设备）
        for addr in addrs {
            if !local_ips.contains(addr) && is_routable(addr) {
                best_addr = Some(*addr);
                break;
            }
        }

        // 2. 其次选择本地可路由地址
        if best_addr.is_none() {
            for addr in addrs {
                if is_routable(addr) {
                    best_addr = Some(*addr);
                    break;
                }
            }
        }

        // 3. 最后选择任意地址（备选）
        if best_addr.is_none() && !addrs.is_empty() {
            best_addr = addrs.first().copied();
        }

        // 创建最终的 DeviceInfo
        if let Some(addr) = best_addr {
            devices.insert(machine_id.clone(), DeviceInfo {
                machine_id,
                name: hostname,
                ip: addr.to_string(),
                port,
            });
        }
    }
}
```

### 5. 虚拟网卡过滤

```rust
fn is_virtual_interface(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let patterns = [
        "virtual", "vmware", "hyper-v", "vbox", "virtualbox",
        "veth", "docker", "wsl", "bridge", "tap", "tun",
        "hamachi", "zerotier", "loopback", "npf",
    ];
    patterns.iter().any(|p| lowered.contains(p))
}
```

这确保虚拟网卡不会产生额外的设备条目。

### 6. DeviceInfo 结构更新

**旧结构**：
```rust
pub struct DeviceInfo {
    pub name: String,      // mDNS 服务名（包含后缀）
    pub ip: String,
    pub port: u16,
}
```

**新结构**：
```rust
pub struct DeviceInfo {
    pub machine_id: String, // 新增：唯一机器标识
    pub name: String,       // 改进：清晰的主机名
    pub ip: String,         // 最优选择的 IP
    pub port: u16,
}
```

### 7. UI 改进

在 React 组件中：

```typescript
// 使用 machine_id 而非 ip:port 作为唯一标识
const active = activeDevice?.machine_id === device.machine_id;

// 清晰显示主机名（不再需要字符串处理）
<p>{device.name}</p>
```

## 数据流示例

### 场景：用户有两台电脑，每台都有 WiFi 和以太网

**电脑 A**（启动 SwiftShare）：
1. 生成 machine_id: `uuid-a`
2. 注册两个服务（都是 `uuid-a[Computer-A]`，但在不同 IP）
   - 192.168.1.100（WiFi）
   - 192.168.1.101（Ethernet）

**电脑 B**（发现电脑 A）：
1. mDNS 浏览器收到 `ServiceResolved` 事件
2. 服务名: `uuid-a[Computer-A]`
3. 地址: `[192.168.1.100, 192.168.1.101]`
4. 内部存储：`uuid-a:7878 -> ([192.168.1.100, 192.168.1.101], "uuid-a", 7878, "Computer-A")`
5. 选择最佳 IP（假设两个都可路由，选择第一个）
6. 发送给 UI：
   ```json
   {
     "machine_id": "uuid-a",
     "name": "Computer-A",
     "ip": "192.168.1.100",
     "port": 7878
   }
   ```
7. UI 显示单个设备条目 "Computer-A" (192.168.1.100)

## 向后兼容性

如果发现老版本的服务（格式：`hostname:port:interface`），系统提供了降级处理：

```rust
fn extract_machine_and_hostname(fullname: &str) -> (String, String) {
    // 新格式：uuid[hostname]
    if let Some(bracket_pos) = instance.find('[') {
        // ... 新格式处理
    }

    // 旧格式降级处理
    (instance.to_string(), instance.to_string())
}
```

## 性能考虑

- **UUID 生成**：一次性操作，发生在应用启动时
- **持久化**：文件 I/O 仅在首次启动时发生
- **聚合**：O(1) 操作（HashMap 查找）
- **IP 选择**：O(n) 其中 n = 网卡数（通常很小）

## 安全考虑

- Machine ID 是随机 UUID，无法追踪用户身份
- 存储在本地文件系统，无网络传输
- 仅在 mDNS 服务名中暴露，不包含敏感信息

## 测试验证

见 `TESTING.md` 文件中的详细测试指南。

关键验证点：
1. ✅ 编译通过（无错误，无重要警告）
2. ✅ 类型检查通过（TypeScript）
3. ✅ 单网卡场景正常
4. ✅ 多网卡场景去重
5. ✅ 虚拟网卡被过滤
6. ✅ 重启后 machine_id 保持一致
