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
New-Item -ItemType Directory -Path "Temp" -Force | Out-Null
Add-Content -Path "Temp\build-history.log" -Value "$buildId`t$gitHash`t$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`t$Profile"
Write-Host "构建号：build$buildId"

# vendored OpenSSL（async_ossl 在 Windows 上启用 openssl/vendored）从源码编译时
# 需要 perl 跑 Configure 脚本；Git Bash 自带的 MSYS perl 缺 Locale::Maketext::Simple
# 等模块，必须用完整发行版（Strawberry Perl）。与上游 CI（gen_windows.yml）做法一致：
# 构建前把 Strawberry Perl 前置到 PATH。
$strawberry = "D:\Tools\StrawberryPerl\perl\bin"
if (Test-Path "$strawberry\perl.exe") {
    $env:PATH = "$strawberry;$env:PATH"
    Write-Host "使用 Strawberry Perl：$strawberry"
} else {
    Write-Host "警告：未找到 $strawberry，vendored OpenSSL 构建可能因 perl 缺模块而失败" -ForegroundColor Yellow
}

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

New-Item -ItemType Directory -Path "dist" -Force | Out-Null
Copy-Item -LiteralPath "$src\orca-term-gui.exe" -Destination "dist\orca-term-gui.exe" -Force
Copy-Item -LiteralPath "$src\orca-term.exe" -Destination "dist\orca-term.exe" -Force
# ConPTY 运行时必须随 exe 分发（见 pty/src/win/psuedocon.rs 的 sideload 逻辑与上游
# changelog）：缺失时回退系统 ConPTY，中文 Windows 上退出会出现 GBK 乱码并落入 cmd。
Copy-Item -LiteralPath "assets\windows\conhost\conpty.dll" -Destination "dist\conpty.dll" -Force
Copy-Item -LiteralPath "assets\windows\conhost\OpenConsole.exe" -Destination "dist\OpenConsole.exe" -Force
Copy-Item -LiteralPath "$configUiSrc\orca-term-config-ui.exe" -Destination "dist\orca-term-config-ui.exe" -Force
# 随包默认 orca-config.lua（assets\orca-config.lua：本机 PowerShell 经
# starship\ 子目录启用提示符的完整配置）。部署时覆盖到程序目录即为该
# 配置；用户后续手改的配置在重新部署时需自行备份。
if (Test-Path "assets\orca-config.lua") {
    Copy-Item "assets\orca-config.lua" "dist\orca-config.lua" -Force
}

# 附带 starship 二进制（与 build.sh 的 fetch_starship 保持一致）：
# - dist\starship\starship-<musl target>：SSH 提示符注入上传到远端用；
# - dist\starship.exe：本机 PowerShell 启动菜单的 starship 提示符用
#   （Issue/1.png：缺失时每次启动 PowerShell 都报「无法识别 starship.exe」）。
# 版本号以 wezterm-ssh/src/orca_prompt.rs 的 STARSHIP_VERSION 为单一事实源。
$ProgressPreference = 'SilentlyContinue'
$orcaPromptSrc = "wezterm-ssh\src\orca_prompt.rs"
$starshipVer = $null
if (Test-Path $orcaPromptSrc) {
    $m = Select-String -Path $orcaPromptSrc -Pattern 'STARSHIP_VERSION: &str = "([0-9.]+)"' | Select-Object -First 1
    if ($m) { $starshipVer = $m.Matches[0].Groups[1].Value }
}
if (-not $starshipVer) {
    Write-Host "警告：无法从 orca_prompt.rs 解析 STARSHIP_VERSION，跳过 starship 下载" -ForegroundColor Yellow
} else {
    # 版本变化时清掉旧版本产物，避免目录里残留过期二进制
    $verFile = "dist\starship\.version"
    if ((Test-Path $verFile) -and ((Get-Content $verFile -Raw).Trim() -ne $starshipVer)) {
        Remove-Item -Recurse -Force "dist\starship" -ErrorAction SilentlyContinue
    }
    New-Item -ItemType Directory -Path "dist\starship" -Force | Out-Null
    Set-Content -Path $verFile -Value $starshipVer -NoNewline

    # starship 相关产物统一放 dist\starship\（本机 PowerShell 与远端注入共用）：
    # - starship.toml：gruvbox powerline 预设，本机经 STARSHIP_CONFIG 引用，
    #   SSH 注入时也上传到远端；
    # - starship.exe：Windows 版，本机 PowerShell 启动菜单引用（Issue/1.png）。
    if (Test-Path "assets\starship.toml") {
        Copy-Item "assets\starship.toml" "dist\starship\starship.toml" -Force
    } else {
        Write-Host "警告：缺少 assets\starship.toml，starship 提示符将使用出厂默认样式" -ForegroundColor Yellow
    }

    $base = "https://github.com/starship/starship/releases/download/v$starshipVer"
    foreach ($target in @("x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl")) {
        $out = "dist\starship\starship-$target"
        if ((Test-Path $out) -and (Get-Item $out).Length -gt 0) { continue }
        Write-Host "==> 下载 starship v$starshipVer ($target)..."
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $tmp -Force | Out-Null
        try {
            Invoke-WebRequest "$base/starship-$target.tar.gz" -OutFile "$tmp\s.tar.gz"
            Invoke-WebRequest "$base/starship-$target.tar.gz.sha256" -OutFile "$tmp\s.sha256"
            $expected = (Get-Content "$tmp\s.sha256" -Raw).Trim().Split()[0]
            $actual = (Get-FileHash "$tmp\s.tar.gz" -Algorithm SHA256).Hash.ToLower()
            if ($expected -ne $actual) { throw "sha256 校验失败（期望 $expected，实际 $actual）" }
            tar -xzf "$tmp\s.tar.gz" -C $tmp starship
            Move-Item "$tmp\starship" $out -Force
            Write-Host "    已就位: $out" -ForegroundColor Green
        } catch {
            Write-Host "==> 警告：starship $target 获取失败：$_，运行时将回退为在线下载/builtin 提示符" -ForegroundColor Yellow
        } finally {
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }
    }

    # Windows 版给本机 PowerShell 用：便携包应开箱即用
    $out = "dist\starship\starship.exe"
    if (-not ((Test-Path $out) -and (Get-Item $out).Length -gt 0)) {
        Write-Host "==> 下载 starship v$starshipVer (x86_64-pc-windows-msvc)..."
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $tmp -Force | Out-Null
        try {
            Invoke-WebRequest "$base/starship-x86_64-pc-windows-msvc.zip" -OutFile "$tmp\s.zip"
            Invoke-WebRequest "$base/starship-x86_64-pc-windows-msvc.zip.sha256" -OutFile "$tmp\s.sha256"
            $expected = (Get-Content "$tmp\s.sha256" -Raw).Trim().Split()[0]
            $actual = (Get-FileHash "$tmp\s.zip" -Algorithm SHA256).Hash.ToLower()
            if ($expected -ne $actual) { throw "sha256 校验失败（期望 $expected，实际 $actual）" }
            Expand-Archive -Path "$tmp\s.zip" -DestinationPath $tmp -Force
            Move-Item "$tmp\starship.exe" $out -Force
            Write-Host "    已就位: $out" -ForegroundColor Green
        } catch {
            Write-Host "==> 警告：starship.exe（Windows）获取失败：$_，本机 PowerShell 的 starship 提示符将不可用" -ForegroundColor Yellow
        } finally {
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }
    }
}

Write-Host "已输出到 dist\：orca-term-gui.exe、orca-term.exe、orca-term-config-ui.exe" -ForegroundColor Green
