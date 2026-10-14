# OrcaTerm

**English** | [简体中文](README.zh-CN.md)

A portable, Windows-first terminal emulator forked from [wezterm](https://github.com/wez/wezterm), with a built-in GUI configuration app powered by [GPUI](https://github.com/zed-industries/zed) (the UI framework of the Zed editor).

## What is different from upstream wezterm

- **Portable distribution** — `build.ps1` produces a self-contained `dist/` directory (exe + `conpty.dll`/`OpenConsole.exe` side-load + fonts + icons). No installer, no registry writes; the app reads its `orca-config.lua` from its own directory instead of `$HOME`.
- **Built-in configuration UI** (`orca-term-config-ui.exe`, started from the gear button in the tab bar) — written in Rust on top of Zed's **GPUI** framework:
  - SSH connection manager (host / port / username / private key, validated and emitted to `orca-config.lua` as `ssh_domains`, integrated with the launch menu)
  - Per-connection "run command after connect" (e.g. `cd /data/project`), implemented via the upstream `SshDomain.default_prog` mechanism
  - Font, color scheme, padding and other common settings in a native GUI
- **Tab bar enhancements** — drag tabs with the mouse to reorder them. A clear-screen button at the end of the tab bar first saves the scrollback and viewport to `screen-shot/` next to the executable (the 5 most recent are kept), then clears the screen.
- **Starship prompt over SSH** — when connecting to a remote host, OrcaTerm detects the remote shell and architecture, downloads a pinned [Starship](https://starship.rs) prompt binary, and wraps the remote shell's startup so the Starship prompt is used. Supports bash, zsh and Fish remote shells.
- **Build-number versioning** — the tab bar shows a per-build identifier (`Ver 1.0 build<date.seq>`) for tracing binaries back to commits.
- **ConPTY side-loading** — ships a pinned `conpty.dll` + `OpenConsole.exe` so console behavior does not depend on the OS build (fixes garbled CJK output / stray cmd.exe on Chinese Windows).
- **Elevated (Administrator) terminal sessions** — the launch menu (right-click the `+` button) offers "elevated CMD / PowerShell" entries. Selecting one triggers the Windows UAC prompt and opens a **new OrcaTerm window running with an administrator token**; every tab inside it is elevated and its title gets a `(管理员)` prefix (based on real token detection via `TokenElevation`, not on the shell name).
  - No manual prerequisites: this uses the native Windows `runas` elevation. It does **not** use or require `sudo.exe` (Microsoft sudo's inline mode cannot attach to a ConPTY, so the elevated shell must be spawned by an elevated OrcaTerm instance of its own).
  - Why a new window: ConPTY handles cannot cross UAC integrity levels, so an elevated shell must live in a process tree that is elevated from the start — the same constraint that makes Windows Terminal open admin terminals in a separate window.

Everything else — the terminal core, multiplexer, PTY layer, Lua configuration — is upstream wezterm.

## Build

Requirements: Rust (MSVC toolchain), Visual Studio 2022 Build Tools, Strawberry Perl (for the vendored OpenSSL), Python 3.

```powershell
./build.ps1              # debug build
./build.ps1 -Profile release
```

Output goes to `dist/`: `orca-term-gui.exe`, `orca-term.exe` (CLI), `orca-term-config-ui.exe`.

### Cross-building from Linux

`build.sh` cross-compiles the Windows binaries from Linux using the `x86_64-pc-windows-gnu` target (a MinGW toolchain is required). It also assigns a build number (`YYYYMMDD.NN`, recorded in `build-id.txt`) and appends an entry to `Temp/build-history.log`.

```bash
./build.sh              # release build (default)
./build.sh debug        # debug build
```

Output also goes to `dist/`.

## Acknowledgements

This project would not exist without:

- **[wezterm](https://github.com/wez/wezterm)** by **Wez Furlong** — OrcaTerm is a direct fork of wezterm; all core terminal functionality comes from it. wezterm is licensed under the MIT License.
- **[Zed](https://github.com/zed-industries/zed)** by **Zed Industries** — the configuration UI is built on the `gpui` framework, which Zed distributes under the Apache-2.0 license.

## License

OrcaTerm is distributed under the [MIT License](LICENSE.md), inherited from wezterm.

Copyright (c) 2018-Present Wez Furlong (wezterm)
Modifications and OrcaTerm-specific code Copyright (c) 2024-Present [sianbox2024](https://github.com/sianbox2024)
