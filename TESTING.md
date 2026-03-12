# SwiftShare 设备识别改进 - 测试指南

## 改进概述

这次改进解决了多张网卡导致同一设备在发现列表中出现多次的问题。

### 核心改进

1. **唯一机器标识 (Machine ID)**
   - 每个 SwiftShare 实例生成一个固定的 UUID
   - 重启后保持一致，便于设备追踪
   - 存储在用户本地数据目录（不同系统路径不同）

2. **改进的 mDNS 服务名**
   - 新格式：`uuid[hostname]`
   - 使得同一机器的多个网卡服务能被识别为同一设备

3. **智能 IP 选择**
   - 当一个设备有多个 IP 地址时，自动选择最佳的
   - 优先级：公网 > 私网 > 链接本地

4. **UI 改进**
   - 显示清晰的主机名（无 mDNS 后缀）
   - 使用 machine_id 作为唯一标识

## 测试场景

### 场景 1：单网卡设备（基础功能）
**预期结果**：正常工作，设备列表显示为一个设备

**测试步骤**：
1. 启动两个 SwiftShare 实例
2. 观察设备列表
3. 验证能否连接和传输文件

### 场景 2：多网卡设备（核心改进）
**预期结果**：同一设备只显示一次，自动选择最佳 IP

**测试步骤**：
1. 在一台有多个网络接口的电脑上启动 SwiftShare
2. 用另一台电脑的 SwiftShare 连接
3. 观察设备列表：
   - 应该只显示一个该设备的条目
   - IP 应该是最优的可连接地址
   - 设备名应该是清晰的主机名

### 场景 3：机器 ID 持久化
**预期结果**：重启应用后，同一机器的 ID 保持一致

**测试步骤**：
1. 启动 SwiftShare，记录设备列表中的 machine_id（可从日志或内部查看）
2. 停止应用
3. 重新启动应用
4. 验证 machine_id 是否相同

### 场景 4：虚拟网卡过滤
**预期结果**：虚拟网卡（Docker、VMware 等）被过滤，不产生重复设备

**测试步骤**：
1. 如果系统有虚拟网络适配器
2. 启动 SwiftShare
3. 检查控制台日志，确认虚拟网卡被跳过
4. 验证设备列表中没有因虚拟网卡产生的重复项

## 验证清单

- [ ] 代码编译通过（无错误）
- [ ] TypeScript 类型检查通过
- [ ] 单网卡场景正常运作
- [ ] 多网卡场景去重正常
- [ ] 设备名显示清晰（无 mDNS 后缀）
- [ ] 能正常连接并传输文件
- [ ] 重启后 machine_id 保持一致
- [ ] 虚拟网卡被正确过滤

## 关键代码位置

- 机器 ID 生成：`src-tauri/src/transport.rs` - `generate_machine_id()` 和 `get_machine_id_path()`
- mDNS 服务注册：`src-tauri/src/discovery.rs` - 第 42-103 行（`start()` 函数）
- 服务发现和去重：`src-tauri/src/discovery.rs` - `emit_devices()` 函数
- 名称解析：`src-tauri/src/discovery.rs` - `extract_machine_and_hostname()` 函数
- UI 更新：`src/App.tsx` - 第 468-489 行（设备列表部分）

## 故障排查

### 如果设备仍然显示多次
1. 检查 mDNS 是否在所有网卡上正确注册
2. 查看控制台日志，搜索 "Announcing" 相关信息
3. 验证 `extract_machine_and_hostname()` 是否正确解析了服务名

### 如果 machine_id 在重启后不一致
1. 检查存储路径是否可写
2. 查看是否有权限问题
3. 检查日志中的 "Failed to persist" 相关错误

### 如果看不到其他设备
1. 检查网络连接是否正常
2. 验证 mDNS 是否在网络上广播
3. 查看防火墙设置

## 日志关键词

- `[Discovery] Starting with machine_id:` - 应用启动时的 machine_id
- `[Discovery] Registering service:` - 服务注册信息
- `[Discovery] Resolved:` - 发现的服务信息
- `[Discovery] Emitting N devices` - 最终发送给 UI 的设备数量
