use anyhow::{anyhow, bail, Context};
use config::keyassignment::SpawnCommand;
use config::TermConfig;
use mux::activity::Activity;
use mux::domain::SplitSource;
use mux::tab::SplitRequest;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_term::TerminalSize;

#[derive(Copy, Debug, Clone, Eq, PartialEq)]
pub enum SpawnWhere {
    NewWindow,
    NewTab,
    SplitPane(SplitRequest),
}

pub fn spawn_command_impl(
    spawn: &SpawnCommand,
    spawn_where: SpawnWhere,
    size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) {
    let spawn = spawn.clone();

    promise::spawn::spawn(async move {
        if let Err(err) =
            spawn_command_internal(spawn, spawn_where, size, src_window_id, term_config).await
        {
            log::error!("Failed to spawn: {:#}", err);
        }
    })
    .detach();
}

pub async fn spawn_command_internal(
    spawn: SpawnCommand,
    spawn_where: SpawnWhere,
    size: TerminalSize,
    src_window_id: Option<MuxWindowId>,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<()> {
    // 提权请求：转交一个新启动的管理员 GUI 实例处理。
    // ConPTY 句柄无法跨完整性级别复用，tab 内"原地提权"在架构上不可行；
    // 提权实例自己创建的 ConPTY+shell 天然继承管理员令牌，
    // 其所有 tab 都会被 is_elevated 检测命中并显示「(管理员)」标题前缀。
    if spawn.elevate {
        if cfg!(windows) {
            if spawn.domain != config::keyassignment::SpawnTabDomain::CurrentPaneDomain {
                anyhow::bail!("elevate 不支持与非本机域组合使用");
            }
            spawn_elevated_instance(&spawn)?;
            return Ok(());
        }
        log::warn!("elevate 仅在 Windows 上生效，已忽略");
    }

    let mux = Mux::get();
    let activity = Activity::new();

    let current_pane_id = match src_window_id {
        Some(window_id) => {
            if let Some(tab) = mux.get_active_tab_for_window(window_id) {
                tab.get_active_pane().map(|p| p.pane_id())
            } else {
                None
            }
        }
        None => None,
    };

    let cwd = if let Some(cwd) = spawn.cwd.as_ref() {
        Some(cwd.to_str().map(|s| s.to_owned()).ok_or_else(|| {
            anyhow!(
                "Domain::spawn requires that the cwd be unicode in {:?}",
                cwd
            )
        })?)
    } else {
        None
    };

    let cmd_builder = match (
        spawn.args.as_ref(),
        spawn.cwd.as_ref(),
        spawn.set_environment_variables.is_empty(),
    ) {
        (None, None, true) => None,
        _ => {
            let mut builder = spawn
                .args
                .as_ref()
                .map(|args| CommandBuilder::from_argv(args.iter().map(Into::into).collect()))
                .unwrap_or_else(CommandBuilder::new_default_prog);
            for (k, v) in spawn.set_environment_variables.iter() {
                builder.env(k, v);
            }
            if let Some(cwd) = &spawn.cwd {
                builder.cwd(cwd);
            }
            Some(builder)
        }
    };

    let workspace = mux.active_workspace().clone();

    match spawn_where {
        SpawnWhere::SplitPane(direction) => {
            let src_window_id = match src_window_id {
                Some(id) => id,
                None => anyhow::bail!("no src window when splitting a pane?"),
            };
            if let Some(tab) = mux.get_active_tab_for_window(src_window_id) {
                let pane = tab
                    .get_active_pane()
                    .ok_or_else(|| anyhow!("tab to have a pane"))?;

                log::trace!("doing split_pane");
                let (pane, _size) = mux
                    .split_pane(
                        // tab.tab_id(),
                        pane.pane_id(),
                        direction,
                        SplitSource::Spawn {
                            command: cmd_builder,
                            command_dir: cwd,
                        },
                        spawn.domain,
                    )
                    .await
                    .context("split_pane")?;
                pane.set_config(term_config);
            } else {
                bail!("there is no active tab while splitting pane!?");
            }
        }
        _ => {
            let (_tab, pane, window_id) = mux
                .spawn_tab_or_window(
                    match spawn_where {
                        SpawnWhere::NewWindow => None,
                        _ => src_window_id,
                    },
                    spawn.domain,
                    cmd_builder,
                    cwd,
                    size,
                    current_pane_id,
                    workspace,
                    spawn.position,
                )
                .await
                .context("spawn_tab_or_window")?;

            // If it was created in this window, it copies our handlers.
            // Otherwise, we'll pick them up when we later respond to
            // the new window being created.
            if Some(window_id) == src_window_id {
                pane.set_config(term_config);
            }
        }
    };

    drop(activity);

    Ok(())
}

/// 以管理员身份启动一个新的 OrcaTerm GUI 实例来处理提权 spawn 请求。
/// 用 `ShellExecuteW` 的 runas 动词触发 UAC；用户确认后新实例以管理员
/// 令牌运行，其 spawn 的 shell 天然继承管理员令牌。使用 --always-new-process 避免
/// 新实例把请求回投给本（未提权）实例。
#[cfg(windows)]
fn spawn_elevated_instance(spawn: &SpawnCommand) -> anyhow::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    use winapi::um::winuser::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    // 提权走 orca-term.exe 的 start 子命令（它会再拉起 GUI）；
    // --always-new-process 保证不回连本实例（它是 start 的选项，须置于 start 之后）。
    let cli = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("orca-term.exe"),
        Err(err) => anyhow::bail!("无法定位当前 exe：{err}"),
    };

    let mut params = String::from("start --always-new-process");
    if let Some(args) = &spawn.args {
        if !args.is_empty() {
            // 参数中可能含空格，统一加引号（Windows 命令行引号规则）
            let quoted: Vec<String> = args
                .iter()
                .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
                .collect();
            params.push_str(" -- ");
            params.push_str(&quoted.join(" "));
        }
    }

    let verb = wide("runas");
    let file = wide(cli.to_string_lossy().as_ref());
    let parameters = wide(&params);

    // ShellExecuteW 需要在有消息循环的线程外调用也没问题，但 UAC 提权
    // 要求调用进程具有可交互的窗口站；GUI 线程满足条件。
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 返回值 <=32 表示失败（含 SE_ERR_ACCESSDENIED=5：用户取消 UAC）
    if result as i32 <= 32 {
        let code = result as i32;
        if code == 5 {
            log::warn!("用户取消了管理员授权（UAC）");
        } else {
            log::error!("ShellExecuteW runas 失败，错误码 {code}");
        }
    }
    Ok(())
}

/// 执行一条完整的命令行（SFTP 外部工具等"快捷方式式"命令），如
/// `"D:\Program Files (x86)\WinSCP\WinSCP.exe" "会话名" /Desktop`。
/// 拆分规则与 CreateProcess 一致：带引号的 exe 取到配对引号，否则取首个
/// 空格前的段；其余为参数。用 ShellExecuteW「open」拉起（解析 PATH、
/// 无控制台窗口、目录式协议处理器也能走）。
#[cfg(windows)]
pub fn shell_execute_command_line(command: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    use winapi::um::winuser::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    let command = command.trim();
    let (file, parameters) = if let Some(rest) = command.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => (&rest[..end], rest[end + 1..].trim_start()),
            None => (command, ""),
        }
    } else {
        match command.split_once(' ') {
            Some((exe, rest)) => (exe, rest.trim_start()),
            None => (command, ""),
        }
    };
    if file.is_empty() {
        log::error!("SFTP 命令为空，无法执行：{command:?}");
        return;
    }

    let verb = wide("open");
    let file_w = wide(file);
    let params_w = wide(parameters);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file_w.as_ptr(),
            params_w.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as i32 <= 32 {
        log::error!(
            "SFTP 命令执行失败（ShellExecuteW 错误码 {}）：{command}",
            result as i32
        );
    }
}

#[cfg(not(windows))]
pub fn shell_execute_command_line(_command: &str) {
    log::error!("SFTP 外部工具命令仅支持 Windows");
}
