$env:TAURI_DEV_PORT = "1420"
$env:TAURI_DEV_HOST = "127.0.0.1"
$port = 1420

Write-Host "Using dev port $port"

$env:TAURI_DEV_PORT = $port
$env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot ("src-tauri\target-$port")
$env:VITE_PORT = $port

$configPath = Join-Path $PSScriptRoot "src-tauri\tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$config.build.devUrl = "http://localhost:$port"

$tempConfig = Join-Path $env:TEMP "tauri.conf.$port.json"
$config | ConvertTo-Json -Depth 20 | Set-Content $tempConfig -Encoding UTF8

# 启动 Vite 开发服务器（后台运行）
Write-Host "Starting Vite dev server on port $port..."
Start-Process -NoNewWindow -FilePath "pnpm" -ArgumentList "dev" -EnvironmentVariables @{VITE_PORT=$port}

# 等待 Vite 启动
Start-Sleep -Seconds 3

pnpm tauri dev --config "$tempConfig"
