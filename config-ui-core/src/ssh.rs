//! SSH 连接模型：快照 `ssh_domains` ↔ SshConnection 列表、Lua 发射、基础校验。
//! 发射固定 `multiplexing='None'`（等价 wezterm ssh 直连，不要求远端装有 wezterm）；
//! 私钥走 `ssh_option.identityfile`（libssh 认识的键，缺省时用 agent/默认密钥）。

use serde_json::Value;

/// PowerShell 探测结果:Some(绝对路径)=pwsh 7 可用,所有「PowerShell」语义的
/// 发射项都指向它;None=回退 powershell.exe(PS 5.1,系统自带,裸名安全)。

/// 解析 pwsh 7 安装位,返回绝对路径。绿色版 pwsh(无 MSI/Store 注册痕迹,如
/// D:\Tools\PowerShell\7)在 PATH 与 %ProgramFiles% 里都找不到,因此除常规
/// 位置外还接受调用方传入的 default_prog 作最终证据——配置里的默认程序
/// 指向 pwsh.exe(任意路径)即说明用户在用 pwsh 7,且该路径就是其安装位。
/// 返回绝对路径而非裸名:裸名 `pwsh.exe` 依赖 PATH,而保存后的菜单会在
/// 提权/最小环境等 PATH 不同的上下文里执行(绿色版不在 PATH 时裸名直接
/// 启动失败,表现为选菜单项即报 didn't exit cleanly)。
fn resolve_pwsh(default_prog: Option<&[String]>) -> Option<String> {
    #[cfg(windows)]
    {
        let from_path = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .find(|dir| dir.join("pwsh.exe").is_file())
                .map(|dir| dir.join("pwsh.exe"))
        });
        let candidates = [
            from_path,
            std::env::var_os("ProgramFiles").map(|pf| {
                std::path::PathBuf::from(pf)
                    .join("PowerShell")
                    .join("7")
                    .join("pwsh.exe")
            }),
            std::env::var_os("LocalAppData").map(|la| {
                std::path::PathBuf::from(la)
                    .join("Microsoft")
                    .join("PowerShell")
                    .join("pwsh.exe")
            }),
            Some(std::path::PathBuf::from(r"D:\Tools\PowerShell\7\pwsh.exe")),
        ];
        for c in candidates.into_iter().flatten() {
            if c.is_file() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
        // 配置快照的 default_prog 兜底:首启模板/用户配置写明 pwsh 完整路径
        // (无论装在哪个盘)就是最直接的证据,路径本身即安装位。
        default_prog.and_then(|prog| pwsh_path_from_prog(prog))
    }
    #[cfg(not(windows))]
    {
        let _ = default_prog;
        None
    }
}

/// 文件名判定层(纯函数):default_prog 第一元素文件名为 pwsh(.exe) 时
/// 返回该路径——绿色版安装唯一的可靠线索。
fn pwsh_path_from_prog(prog: &[String]) -> Option<String> {
    let exe = prog.first()?;
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    if file.eq_ignore_ascii_case("pwsh.exe") || file.eq_ignore_ascii_case("pwsh") {
        Some(exe.clone())
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SshConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// 私钥文件路径；空 = 跟随 ssh-agent / 默认密钥位置
    pub key_path: String,
    /// 连接后执行的命令（如 `cd /data/project`）；空 = 直接进默认 shell。
    /// 发射时包装为 default_prog={'sh','-c','<命令>; exec $SHELL'}。
    pub initial_command: String,
}

/// 从生效配置快照解析用户 ssh_domains。端口内嵌在 remote_address（host:port）里。
pub fn parse_ssh_domains(snap: &Value) -> Vec<SshConnection> {
    snap.get("ssh_domains")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let name = d.get("name").and_then(|x| x.as_str())?.to_string();
                    let addr = d
                        .get("remote_address")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    let (host, port) = split_addr(addr);
                    Some(SshConnection {
                        name,
                        host: host.to_string(),
                        port,
                        username: d
                            .get("username")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        key_path: d
                            .get("ssh_option")
                            .and_then(|o| o.get("identityfile"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        initial_command: d
                            .get("default_prog")
                            .and_then(|x| x.as_array())
                            .map(|prog| {
                                unwrap_default_prog(
                                    &prog.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>(),
                                )
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// host:port 拆分；无端口按 22。带端口的裸 IPv6 是罕见场景，v1 不处理。
fn split_addr(addr: &str) -> (&str, u16) {
    match addr.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => (
                h.trim_start_matches('[').trim_end_matches(']'),
                port,
            ),
            Err(_) => (addr, 22),
        },
        None => (addr, 22),
    }
}

fn addr_of(c: &SshConnection) -> String {
    if c.port == 22 {
        c.host.clone()
    } else {
        format!("{}:{}", c.host, c.port)
    }
}

/// default_prog 包装后缀：命令执行完（无论成败，`;` 分隔）exec 回用户默认 shell，
/// 避免连接建立即退出。$SHELL 由远端 sshd 环境提供。
const PROG_SUFFIX: &str = "; exec $SHELL";

/// 把用户填的连接后命令包装成 SshDomain.default_prog 的 argv；空命令返回 None。
/// 用户命令尾部多余的 `;` 先剥掉，避免拼出 `;;` 造成远端 sh 语法错误。
pub fn wrap_default_prog(cmd: &str) -> Option<Vec<String>> {
    let cmd = cmd.trim().trim_end_matches(';').trim_end();
    if cmd.is_empty() {
        return None;
    }
    Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("{cmd}{PROG_SUFFIX}"),
    ])
}

/// 从 default_prog 解出用户填的命令；非本工具的包装格式（如用户手写 Lua）
/// 按空格拼接近似还原，保证已保存内容不静默丢失。
pub fn unwrap_default_prog(prog: &[String]) -> String {
    if let [backend, flag, inner] = prog {
        if backend == "sh" && flag == "-c" {
            if let Some(cmd) = inner.strip_suffix(PROG_SUFFIX) {
                return cmd.trim_end_matches(';').trim_end().to_string();
            }
        }
    }
    prog.join(" ")
}

/// Lua 单引号字面量（与 form::quote 同规则：先反斜杠再引号）
fn quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// 发射 `config.ssh_domains = {...}`；空列表返回空串（由调用方决定是否清除）。
pub fn emit_ssh_domains(conns: &[SshConnection]) -> String {
    if conns.is_empty() {
        return String::new();
    }
    let mut out = String::from("config.ssh_domains = {\n");
    for c in conns {
        out.push_str(&format!(
            "  {{ name={}, remote_address={}, multiplexing='None'",
            quote(c.name.trim()),
            quote(addr_of(c).trim())
        ));
        let user = c.username.trim();
        if !user.is_empty() {
            out.push_str(&format!(", username={}", quote(user)));
        }
        let key = c.key_path.trim();
        if !key.is_empty() {
            out.push_str(&format!(
                ", ssh_option={{ identityfile={} }}",
                quote(key)
            ));
        }
        if let Some(prog) = wrap_default_prog(&c.initial_command) {
            let items: Vec<String> = prog.iter().map(|v| quote(v)).collect();
            out.push_str(&format!(", default_prog={{{}}}", items.join(",")));
        }
        out.push_str(" },\n");
    }
    out.push_str("}\n");
    out
}

/// 发射 `config.launch_menu`：四项本地 shell（CMD / PowerShell / 管理员CMD / 管理员PowerShell，
/// 后两项用 elevate=true 由提权的新 GUI 实例运行，UAC 授权后生效），再为每个 SSH 连接
/// 生成对应条目。保存时无条件重写，保证菜单与连接列表同步。
/// `default_prog` 为当前配置快照的默认程序（探测 pwsh 的最终证据，见 resolve_pwsh）。
pub fn emit_launch_menu(conns: &[SshConnection], default_prog: Option<&[String]>) -> String {
    // PowerShell 菜单项与默认 shell 保持一致：探测到 pwsh 7 时用其绝对路径
    // （含 -NoLogo），否则回退 powershell.exe（PS 5.1）。必须是绝对路径——
    // 此前发射裸名 pwsh.exe，绿色版安装（不在 PATH）下选菜单项即启动失败。
    let (ps_label, ps_args, ps_admin_label) = match resolve_pwsh(default_prog) {
        Some(pwsh) => (
            "新PowerShell 7窗口",
            format!("{{{}, '-NoLogo'}}", quote(&pwsh)),
            "新管理员PowerShell 7窗口",
        ),
        None => (
            "新PowerShell窗口",
            "{'powershell.exe'}".to_string(),
            "新管理员PowerShell窗口",
        ),
    };
    let mut out = String::from("config.launch_menu = {\n");
    out.push_str("  { label='新CMD窗口', args={'cmd.exe'} },\n");
    out.push_str(&format!(
        "  {{ label='{}', args={} }},\n",
        ps_label, ps_args
    ));
    out.push_str("  { label='新管理员CMD窗口', args={'cmd.exe'}, elevate=true },\n");
    out.push_str(&format!(
        "  {{ label='{}', args={}, elevate=true }},\n",
        ps_admin_label, ps_args
    ));
    for c in conns {
        let name = c.name.trim();
        if name.is_empty() {
            continue;
        }
        // 菜单标签带「SSH连接」前缀，DomainName 仍用原名匹配 ssh_domains
        out.push_str(&format!(
            "  {{ label={}, domain={{ DomainName={} }} }},\n",
            quote(&format!("SSH连接（{name}）")),
            quote(name)
        ));
    }
    out.push_str("}\n");
    // tab 标题「(管理员)」标记：前台进程提权时加前缀（PaneInformation.is_elevated，
    // 由 procinfo::LocalProcessInfo::is_elevated 提供，仅 Windows）
    out.push_str(
        "wezterm.on('format-tab-title', function(tab, tabs, panes, config, hover, tab_max_width)\n\
         \x20 local title = tab.tab_title\n\
         \x20 if #title == 0 then\n\
         \x20   title = tab.active_pane.title\n\
         \x20 end\n\
         \x20 local pane = tab.active_pane\n\
         \x20 if pane and pane.is_elevated then\n\
         \x20   title = '(管理员)' .. title\n\
         \x20 end\n\
         \x20 return {\n\
         \x20   { Text = title },\n\
         \x20 }\n\
         end)\n",
    );
    out
}

/// 保存前校验：名称/主机非空、名称唯一、名称不含引号与反斜杠（Lua 注入面收口）。
/// 返回 Err(消息) 时调用方应中止保存。
pub fn validate(conns: &[SshConnection]) -> Result<(), String> {
    let mut seen: Vec<&str> = vec![];
    for c in conns {
        let name = c.name.trim();
        if name.is_empty() {
            return Err(format!("SSH 连接「{}」名称为空", addr_of(c)));
        }
        if c.host.trim().is_empty() {
            return Err(format!("SSH 连接「{name}」主机为空"));
        }
        if name.contains(['\'', '"', '\\']) {
            return Err(format!("SSH 连接名称不能包含引号或反斜杠：{name}"));
        }
        if seen.contains(&name) {
            return Err(format!("SSH 连接名称重复：{name}"));
        }
        seen.push(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_reads_domains_with_key_and_port() {
        let snap = json!({
            "ssh_domains": [
                {"name": "prod", "remote_address": "10.0.0.1:2222", "username": "root",
                 "ssh_option": {"identityfile": "C:\\keys\\id_ed25519"}, "multiplexing": "None",
                 "default_prog": ["sh", "-c", "cd /data/project; exec $SHELL"]},
                {"name": "dev", "remote_address": "dev.example.com", "multiplexing": "None"}
            ]
        });
        let conns = parse_ssh_domains(&snap);
        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].name, "prod");
        assert_eq!(conns[0].host, "10.0.0.1");
        assert_eq!(conns[0].port, 2222);
        assert_eq!(conns[0].username, "root");
        assert_eq!(conns[0].key_path, "C:\\keys\\id_ed25519");
        assert_eq!(conns[0].initial_command, "cd /data/project");
        assert_eq!(conns[1].port, 22);
        assert_eq!(conns[1].key_path, "");
        assert_eq!(conns[1].initial_command, "");
    }

    #[test]
    fn missing_domains_yields_empty() {
        assert!(parse_ssh_domains(&json!({})).is_empty());
    }

    #[test]
    fn emit_roundtrip_matches_form_shapes() {
        let conns = vec![SshConnection {
            name: "prod".into(),
            host: "10.0.0.1".into(),
            port: 2222,
            username: "root".into(),
            key_path: "C:\\keys\\id_ed25519".into(),
            initial_command: String::new(),
        }];
        let lua = emit_ssh_domains(&conns);
        assert!(lua.contains("name='prod'"), "{lua}");
        assert!(lua.contains("remote_address='10.0.0.1:2222'"), "{lua}");
        assert!(lua.contains("username='root'"), "{lua}");
        assert!(lua.contains("identityfile='C:\\\\keys\\\\id_ed25519'"), "{lua}");
        assert!(lua.contains("multiplexing='None'"), "{lua}");
        assert!(!lua.contains("default_prog"), "{lua}");
        // 发射的 lua 经真实读链路解析后应还原为同一连接（拼接格式与 save() 一致）
        let src = format!(
            "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\n\n{return}{lua}return config\n",
            return = "",
        );
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        assert_eq!(parse_ssh_domains(&snap), conns);
    }

    #[test]
    fn initial_command_roundtrip_via_real_loader() {
        let conns = vec![SshConnection {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 22,
            username: String::new(),
            key_path: String::new(),
            initial_command: "cd /data/project && source env.sh".into(),
        }];
        let lua = emit_ssh_domains(&conns);
        assert!(
            lua.contains("default_prog={'sh','-c','cd /data/project && source env.sh; exec $SHELL'}"),
            "{lua}"
        );
        let src = format!(
            "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\n\n{return}{lua}return config\n",
            return = "",
        );
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        assert_eq!(parse_ssh_domains(&snap), conns);
    }

    #[test]
    fn wrap_strips_trailing_semicolons_and_empty() {
        assert_eq!(wrap_default_prog("  "), None);
        assert_eq!(wrap_default_prog(";;"), None);
        assert_eq!(
            wrap_default_prog("cd /x;"),
            Some(vec!["sh".into(), "-c".into(), "cd /x; exec $SHELL".into()])
        );
        assert_eq!(unwrap_default_prog(&wrap_default_prog("cd /x;").unwrap()), "cd /x");
        // 非本工具包装格式：近似还原不丢内容
        assert_eq!(unwrap_default_prog(&["bash".into(), "-l".into()]), "bash -l");
    }

    #[test]
    fn empty_list_emits_nothing() {
        assert_eq!(emit_ssh_domains(&[]), "");
    }

    #[test]
    fn launch_menu_lists_shells_admin_and_ssh_entries() {
        let conns = vec![SshConnection {
            name: "MSI".into(),
            host: "192.168.1.100".into(),
            port: 22,
            username: "sb".into(),
            key_path: String::new(),
            initial_command: String::new(),
        }];
        let lua = emit_launch_menu(&conns, None);
        assert!(lua.contains("label='新CMD窗口'"), "{lua}");
        // PowerShell 项跟随本机探测（pwsh 7 绝对路径 / PS 5.1），但普通项与管理员
        // 项必须同源：提取普通项的 args 断言管理员项一致，防止两臂漂移。
        let normal = lua.find("label='新PowerShell").expect("存在 PowerShell 菜单项");
        let args_start = normal + lua[normal..].find("args={").expect("args 存在");
        // 普通项行尾恒为 "args=... } },"——"} }," 的起点即 args 值自身的收尾 }
        let args_end = args_start + lua[args_start..].find("} },").expect("args 收尾");
        let ps_args = &lua[args_start..=args_end];
        assert!(
            lua.contains(&format!("{ps_args}, elevate=true")),
            "管理员 PowerShell 项应与普通项使用同一 shell：ps_args={ps_args:?} lua={lua}"
        );
        assert!(
            lua.contains("label='新管理员CMD窗口', args={'cmd.exe'}, elevate=true"),
            "{lua}"
        );
        assert!(!lua.contains("'sudo'"), "不应再依赖 sudo：{lua}");
        // SSH 条目菜单标签带「SSH连接」前缀，DomainName 仍用连接原名，
        // 按配置顺序排在四个 shell 之后
        let cmd_pos = lua.find("label='新CMD窗口'").unwrap();
        let admin_pos = lua.find("label='新管理员CMD窗口'").unwrap();
        let ssh_pos = lua.find("label='SSH连接（MSI）'").unwrap();
        assert!(cmd_pos < admin_pos && admin_pos < ssh_pos, "{lua}");
        assert!(
            lua.contains("label='SSH连接（MSI）', domain={ DomainName='MSI' }"),
            "{lua}"
        );
        let empty = emit_launch_menu(&[], None);
        assert!(
            empty.contains("新CMD窗口") && empty.contains("新管理员PowerShell") && !empty.contains("DomainName"),
            "{empty}"
        );
    }

    #[test]
    fn pwsh_default_prog_forces_pwsh_menu_even_on_green_install() {
        // 绿色版 pwsh（不在 PATH/ProgramFiles）兜底层：配置快照 default_prog
        // 指向 pwsh.exe（探测只看文件名，路径任意）时菜单必须是 pwsh 7 项。
        // 本机位置探测层可能命中也可能不命中（环境相关），因此这里断言的是
        // 环境无关的回归不变量：绝不发射裸名 pwsh.exe，且 pwsh 项一定带
        // 绝对路径形式的 args。
        let conns = vec![SshConnection {
            name: "MSI".into(),
            host: "192.168.1.100".into(),
            port: 22,
            username: "sb".into(),
            key_path: String::new(),
            initial_command: String::new(),
        }];
        let prog: Vec<String> = vec![r"X:\nonexistent\pwsh.exe".into(), "-NoLogo".into()];
        let lua = emit_launch_menu(&conns, Some(&prog));
        assert!(
            lua.contains("label='新PowerShell 7窗口'"),
            "default_prog 指向 pwsh.exe 时应发射 pwsh 7 菜单项：{lua}"
        );
        assert!(
            !lua.contains("args={'pwsh.exe'"),
            "绝不能发射裸名 pwsh.exe（绿色版不在 PATH 时启动失败）：{lua}"
        );
        assert!(
            lua.contains("pwsh.exe', '-NoLogo'"),
            "pwsh 项应带绝对路径 + -NoLogo：{lua}"
        );
    }

    #[test]
    fn launch_menu_never_emits_bare_pwsh() {
        // 回归：配置 UI 每次保存都无条件重写 launch_menu，曾把 PowerShell 菜单项
        // 发射成裸名 pwsh.exe，覆盖掉用户手改的绝对路径（绿色版安装下选菜单
        // 即 didn't exit cleanly）。无论探测走哪一层，都不得再出现裸名。
        for lua in [emit_launch_menu(&[], None), emit_launch_menu(&[], Some(&[]))] {
            assert!(
                !lua.contains("args={'pwsh.exe'"),
                "绝不能发射裸名 pwsh.exe：{lua}"
            );
        }
    }

    #[test]
    fn default_prog_filename_probe_distinguishes_pwsh_from_ps5() {
        // 文件名判定层（纯函数）只认 pwsh(.exe)，不把 powershell.exe 误判成 pwsh 7。
        // 路径逐字透传（即安装位），供发射层作绝对路径使用。
        let probe = super::pwsh_path_from_prog;
        let pwsh = vec![r"X:\anywhere\pwsh.exe".to_string()];
        let pwsh_unix = vec!["/opt/pwsh".to_string()];
        let ps5 = vec!["powershell.exe".to_string()];
        let cmd = vec!["cmd.exe".to_string()];
        assert_eq!(probe(&pwsh), Some(r"X:\anywhere\pwsh.exe".to_string()));
        assert_eq!(probe(&pwsh_unix), Some("/opt/pwsh".to_string()));
        assert_eq!(probe(&ps5), None, "powershell.exe 不是 pwsh");
        assert_eq!(probe(&cmd), None, "cmd.exe 不是 pwsh");
    }

    #[test]
    fn validate_rejects_empty_and_duplicates() {
        let ok = vec![SshConnection {
            name: "a".into(),
            host: "h".into(),
            port: 22,
            username: String::new(),
            key_path: String::new(),
            initial_command: String::new(),
        }];
        assert!(validate(&ok).is_ok());
        let no_host = vec![SshConnection {
            name: "a".into(),
            host: String::new(),
            port: 22,
            username: String::new(),
            key_path: String::new(),
            initial_command: String::new(),
        }];
        assert!(validate(&no_host).is_err());
        let dup = vec![
            ok[0].clone(),
            SshConnection {
                name: "a".into(),
                host: "h2".into(),
                port: 22,
                username: String::new(),
                key_path: String::new(),
                initial_command: String::new(),
            },
        ];
        assert!(validate(&dup).is_err());
    }
}
