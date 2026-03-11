$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()

Write-Host "Using dev port $port"

$env:TAURI_DEV_PORT = $port
$env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot ("src-tauri\target-$port")

$configPath = Join-Path $PSScriptRoot "src-tauri\tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$config.build.devUrl = "http://localhost:$port"

$tempConfig = Join-Path $env:TEMP "tauri.conf.$port.json"
$config | ConvertTo-Json -Depth 20 | Set-Content $tempConfig -Encoding UTF8

pnpm tauri dev --config "$tempConfig"
