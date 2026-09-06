# 构建 orca-term 主程序并输出到 portable 分发目录。
# 用法：pwsh ./build.ps1   （可选参数：-Profile release 用 --release 构建）
param([string]$Profile = "debug")

# 构建号流水：日期.当日序号（build20260905.01），每次构建 +1，跨日重置为 01。
# 记录在 build-id.txt（当前号，wezterm-gui/build.rs 编译期读取）；
# Temp\build-history.log 追加一行「构建号 + git 短哈希 + 时间」用于把构建号对回代码版本。
$today = Get-Date -Format "yyyyMMdd"
$last = ""
if (Test-Path "build-id.txt") { $last = (Get-Content "build-id.txt" -Raw).Trim() }
$seq = 1
if ($last -match "^$today\.(\d+)$") { $seq = [int]$Matches[1] + 1 }
$buildId = "{0}.{1:d2}" -f $today, $seq
Set-Content -Path "build-id.txt" -Value $buildId -NoNewline
$gitHash = (git rev-parse --short=7 HEAD) 2>$null
if (-not $gitHash) { $gitHash = "nogit" }
Add-Content -Path "Temp\build-history.log" -Value "$buildId`t$gitHash`t$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`t$Profile"
Write-Host "构建号：build$buildId"

if ($Profile -eq "release") {
    cargo build --release
    $src = "target\release"
} else {
    cargo build
    $src = "target\debug"
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "cargo build 失败（exit=$LASTEXITCODE）" -ForegroundColor Red
    exit $LASTEXITCODE
}

# config-ui 是独立 workspace（根 cargo build 不会构建它）。GUI 配置按钮点击时
# 会拉起 exe 同目录的 orca-term-config-ui.exe（见 wezterm-gui/src/termwindow/
# mouseevent.rs 的 ConfigUIButton 分支），缺失时静默无反应，必须随包分发。
Push-Location config-ui
if ($Profile -eq "release") {
    cargo build --release
    $configUiSrc = "config-ui\target\release"
} else {
    cargo build
    $configUiSrc = "config-ui\target\debug"
}
Pop-Location
if ($LASTEXITCODE -ne 0) {
    Write-Host "config-ui cargo build 失败（exit=$LASTEXITCODE）" -ForegroundColor Red
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Path "portable" -Force | Out-Null
Copy-Item -LiteralPath "$src\orca-term-gui.exe" -Destination "portable\orca-term-gui.exe" -Force
Copy-Item -LiteralPath "$src\orca-term.exe" -Destination "portable\orca-term.exe" -Force
# ConPTY 运行时必须随 exe 分发（见 pty/src/win/psuedocon.rs 的 sideload 逻辑与上游
# changelog）：缺失时回退系统 ConPTY，中文 Windows 上退出会出现 GBK 乱码并落入 cmd。
Copy-Item -LiteralPath "assets\windows\conhost\conpty.dll" -Destination "portable\conpty.dll" -Force
Copy-Item -LiteralPath "assets\windows\conhost\OpenConsole.exe" -Destination "portable\OpenConsole.exe" -Force
Copy-Item -LiteralPath "$configUiSrc\orca-term-config-ui.exe" -Destination "portable\orca-term-config-ui.exe" -Force

Write-Host "已输出到 portable\：orca-term-gui.exe、orca-term.exe、orca-term-config-ui.exe" -ForegroundColor Green
