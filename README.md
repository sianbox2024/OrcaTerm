# OrcaTerm

**English** | [简体中文](README.zh-CN.md)

A portable, Windows-first terminal emulator forked from [wezterm](https://github.com/wez/wezterm), with a built-in GUI configuration app powered by [GPUI](https://github.com/zed-industries/zed) (the UI framework of the Zed editor).

## What is different from upstream wezterm

- **Portable distribution** — `build.ps1` produces a self-contained `portable/` directory (exe + `conpty.dll`/`OpenConsole.exe` side-load + fonts + icons). No installer, no registry writes; the app reads its `orca-config.lua` from its own directory instead of `$HOME`.
- **Built-in configuration UI** (`orca-term-config-ui.exe`, started from the gear button in the tab bar) — written in Rust on top of Zed's **GPUI** framework:
  - SSH connection manager (host / port / username / private key, validated and emitted to `orca-config.lua` as `ssh_domains`, integrated with the launch menu)
  - Per-connection "run command after connect" (e.g. `cd /data/project`), implemented via the upstream `SshDomain.default_prog` mechanism
  - Font, color scheme, padding and other common settings in a native GUI
- **Build-number versioning** — the tab bar shows a per-build identifier (`Ver 1.0 build<date.seq>`) for tracing binaries back to commits.
- **ConPTY side-loading** — ships a pinned `conpty.dll` + `OpenConsole.exe` so console behavior does not depend on the OS build (fixes garbled CJK output / stray cmd.exe on Chinese Windows).

Everything else — the terminal core, multiplexer, PTY layer, Lua configuration — is upstream wezterm.

## Build

Requirements: Rust (MSVC toolchain), Visual Studio 2022 Build Tools, Strawberry Perl (for the vendored OpenSSL), Python 3.

```powershell
./build.ps1              # debug build
./build.ps1 -Profile release
```

Output goes to `portable/`: `orca-term-gui.exe`, `orca-term.exe` (CLI), `orca-term-config-ui.exe`.

## Acknowledgements

This project would not exist without:

- **[wezterm](https://github.com/wez/wezterm)** by **Wez Furlong** — OrcaTerm is a direct fork of wezterm; all core terminal functionality comes from it. wezterm is licensed under the MIT License.
- **[Zed](https://github.com/zed-industries/zed)** by **Zed Industries** — the configuration UI is built on the `gpui` framework, which Zed distributes under the Apache-2.0 license.

## License

OrcaTerm is distributed under the [MIT License](LICENSE.md), inherited from wezterm.

Copyright (c) 2018-Present Wez Furlong (wezterm)
Modifications and OrcaTerm-specific code Copyright (c) 2024-Present [sianbox2024](https://github.com/sianbox2024)
