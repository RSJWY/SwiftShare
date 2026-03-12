#!/bin/bash

# SwiftShare 测试验证脚本

echo "=== SwiftShare 编译验证 ==="

cd "D:/Other/SwiftShare/src-tauri"
echo "检查 Rust 编译..."
cargo check > /tmp/rust_check.log 2>&1
if [ $? -eq 0 ]; then
  echo "✅ Rust 编译通过"
else
  echo "❌ Rust 编译失败"
  cat /tmp/rust_check.log | tail -20
  exit 1
fi

cd "D:/Other/SwiftShare"
echo "检查 TypeScript..."
npx tsc --noEmit > /tmp/ts_check.log 2>&1
if [ $? -eq 0 ]; then
  echo "✅ TypeScript 检查通过"
else
  echo "❌ TypeScript 检查失败"
  cat /tmp/ts_check.log | tail -20
  exit 1
fi

echo ""
echo "=== 所有检查通过! ==="
echo ""
echo "修复内容:"
echo "1. ✅ 拖放共享文件 - 改为直接刷新完整列表"
echo "2. ✅ 自设备过滤 - 添加条件判断避免空值过滤"
echo "3. ✅ 初始化竞速 - 优先从持久化文件读取 machine_id"
echo ""
echo "下一步:"
echo "1. 运行 'npm run tauri dev' 启动开发版本"
echo "2. 启动两个实例测试设备发现"
echo "3. 拖入文件测试共享功能"
