# OrcaTerm

[English](README.md) | **简体中文**

一个便携、Windows 优先的终端模拟器，分叉自 [wezterm](https://github.com/wez/wezterm)，并内置了一个基于 [GPUI](https://github.com/zed-industries/zed)（Zed 编辑器的 UI 框架）构建的图形化配置程序。

## 与上游 wezterm 的差异

- **便携化分发** — `build.ps1` 产出自包含的 `dist/` 目录（exe + 侧载的 `conpty.dll`/`OpenConsole.exe` + 字体 + 图标）。无安装器、不写注册表；程序从自身目录读取 `orca-config.lua`，而非 `$HOME`。
- **内置配置界面**（`orca-term-config-ui.exe`，从标签栏齿轮按钮启动）— 基于 Zed 的 **GPUI** 框架以 Rust 编写：
  - SSH 连接管理器（主机 / 端口 / 用户名 / 私钥，校验后以 `ssh_domains` 形式写入 `orca-config.lua`，并与启动菜单集成）
  - 每连接「连接后执行命令」（如 `cd /data/project`），基于上游 `SshDomain.default_prog` 机制实现
  - 字体、配色、内边距等常用设置的图形化编辑
- **流水构建号** — 标签栏显示每次构建的标识（`Ver 1.0 build<日期.序号>`），便于把二进制追溯回代码提交。
- **ConPTY 侧载** — 随包分发固定版本的 `conpty.dll` + `OpenConsole.exe`，使控制台行为不依赖系统版本（修复中文 Windows 上的乱码 / 退出落入 cmd 等问题）。

其余部分——终端核心、多路复用器、PTY 层、Lua 配置——均为上游 wezterm。

## 构建

依赖：Rust（MSVC 工具链）、Visual Studio 2022 Build Tools、Strawberry Perl（编译内置 OpenSSL 用）、Python 3。

```powershell
./build.ps1              # debug 构建
./build.ps1 -Profile release
```

产物输出到 `dist/`：`orca-term-gui.exe`、`orca-term.exe`（CLI）、`orca-term-config-ui.exe`。

## 致谢

没有以下项目就没有本仓库：

- **[wezterm](https://github.com/wez/wezterm)**，作者 **Wez Furlong** — OrcaTerm 是 wezterm 的直接分叉，全部核心终端能力来自它。wezterm 以 MIT 许可证发布。
- **[Zed](https://github.com/zed-industries/zed)**，作者 **Zed Industries** — 配置界面构建于其 `gpui` 框架之上，该框架以 Apache-2.0 许可证发布。

## 许可证

OrcaTerm 沿用 wezterm 的 [MIT 许可证](LICENSE.md) 分发。

Copyright (c) 2018-Present Wez Furlong（wezterm 原始代码）
修改与 OrcaTerm 专属代码 Copyright (c) 2024-Present [sianbox2024](https://github.com/sianbox2024)
