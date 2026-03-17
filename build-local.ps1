# SwiftShare 本地构建脚本
# 用于快速打包测试，无需等待 GitHub Actions

# 修复中文乱码：强制 UTF-8
$PSDefaultParameterValues['*:Encoding'] = 'utf8'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:PYTHONIOENCODING = "utf-8"
chcp 65001 > $null 2>&1

Write-Host "=== SwiftShare 本地构建 ===" -ForegroundColor Cyan

# 检查 pnpm
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Host "错误: 未找到 pnpm，请先安装 pnpm" -ForegroundColor Red
    exit 1
}

# 检查 Rust
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "错误: 未找到 Rust，请先安装 Rust" -ForegroundColor Red
    exit 1
}

# 清理旧的构建产物（可选）
$cleanBuild = $args -contains "--clean"
if ($cleanBuild) {
    Write-Host "清理旧的构建产物..." -ForegroundColor Yellow
    if (Test-Path "src-tauri\target\release") {
        Remove-Item -Recurse -Force "src-tauri\target\release\bundle" -ErrorAction SilentlyContinue
        Remove-Item -Force "src-tauri\target\release\SwiftShare.exe" -ErrorAction SilentlyContinue
    }
}

# 安装前端依赖
Write-Host "`n[1/3] 安装前端依赖..." -ForegroundColor Green
pnpm install
if ($LASTEXITCODE -ne 0) {
    Write-Host "前端依赖安装失败" -ForegroundColor Red
    exit 1
}

# 构建应用
Write-Host "`n[2/3] 构建应用..." -ForegroundColor Green
Write-Host "  (本地构建已跳过更新签名)" -ForegroundColor Gray

# 创建临时配置文件覆盖 createUpdaterArtifacts（避免 PowerShell 引号转义问题）
$overrideConf = Join-Path $PWD "src-tauri\.build-override.json"
'{"bundle":{"createUpdaterArtifacts":false}}' | Out-File -FilePath $overrideConf -Encoding ascii -NoNewline
try {
    npx tauri build --config $overrideConf
} finally {
    Remove-Item -Force $overrideConf -ErrorAction SilentlyContinue
}
if ($LASTEXITCODE -ne 0) {
    Write-Host "构建失败" -ForegroundColor Red
    exit 1
}

# 显示构建产物位置
Write-Host "`n[3/3] 构建完成!" -ForegroundColor Green
Write-Host "`n产物位置:" -ForegroundColor Cyan

$exePath = "src-tauri\target\release\SwiftShare.exe"
$nsisPath = "src-tauri\target\release\bundle\nsis"

if (Test-Path $exePath) {
    $exeSize = (Get-Item $exePath).Length / 1MB
    Write-Host "  便携版: $exePath" -ForegroundColor White
    Write-Host "          大小: $([math]::Round($exeSize, 2)) MB" -ForegroundColor Gray
}

if (Test-Path $nsisPath) {
    $installers = Get-ChildItem "$nsisPath\*.exe"
    foreach ($installer in $installers) {
        $installerSize = $installer.Length / 1MB
        Write-Host "  安装包: $($installer.FullName)" -ForegroundColor White
        Write-Host "          大小: $([math]::Round($installerSize, 2)) MB" -ForegroundColor Gray
    }
}

Write-Host "`n提示: 使用 --clean 参数可清理旧产物后重新构建" -ForegroundColor Yellow
Write-Host "示例: .\build-local.ps1 --clean" -ForegroundColor Gray
