//! OrcaTerm:SSH 提示符注入。
//!
//! 目标:在远端机器不安装任何东西的前提下,让 OrcaTerm 的 SSH 交互式
//! shell 拥有 starship 风格的提示符。提示符由远端 shell 打印,终端侧
//! 唯一的办法是把提示符逻辑送进远端 shell,因此在建立交互式 shell 时:
//!
//! 1. exec 一段探测命令,取得远端 $SHELL/$HOME/架构,并创建注入目录;
//! 2. 经 SFTP(不可用时退化为 exec+base64)上传提示符脚本;starship
//!    模式还会上传静态编译的 starship 二进制(来源:显式配置的本地
//!    路径 -> 程序同目录 dist/starship/ -> 本地 PATH -> 本地缓存 ->
//!    GitHub Releases 下载);
//! 3. 以 `bash --rcfile` / `zsh ZDOTDIR` 方式启动交互式 shell,加载
//!    注入的 rc(其中会先 source 用户原有的 rc);
//! 4. 任何一步失败都回退为普通的 request_shell,不影响连接本身;
//!    starship 获取失败时降级为内置提示符。
//!
//! 注入只作用于未显式指定远端命令的场景(`new_pty` 的 command_line
//! 为 None);配置项见 config::SshPromptInjection。

use crate::sftp::types::{OpenFileType, OpenOptions, WriteMode};
use crate::sessioninner::SessionInner;
use crate::sessionwrap::SessionWrap;
use anyhow::{anyhow, bail, Context};
use base64::Engine;
use camino::Utf8Path;
use portable_pty::ExitStatus;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

const INJECT_MODE_KEY: &str = "orcaterm.inject_prompt";
const STARSHIP_LOCAL_KEY: &str = "orcaterm.starship_path";

/// 远端 starship 的版本号;升级时改这里即可触发重新上传
const STARSHIP_VERSION: &str = "1.26.0";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

const BUILTIN_SCRIPT: &str = include_str!("orca_prompt_builtin.sh");

#[derive(Copy, Clone, PartialEq, Eq)]
enum InjectMode {
    Builtin,
    Starship,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ShellKind {
    Bash,
    Zsh,
}

impl ShellKind {
    fn name(self) -> &'static str {
        match self {
            ShellKind::Bash => "bash",
            ShellKind::Zsh => "zsh",
        }
    }
}

/// 远端探测结果
struct RemoteProbe {
    shell: String,
    home: String,
    arch: String,
    zdotdir: String,
}

fn shell_kind(probe: &RemoteProbe) -> Option<ShellKind> {
    match probe.shell.rsplit('/').next().unwrap_or("") {
        "bash" => Some(ShellKind::Bash),
        "zsh" => Some(ShellKind::Zsh),
        _ => None,
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 与远端架构对应的 ELF e_machine 值
fn elf_machine(arch: &str) -> Option<u16> {
    match arch {
        "x86_64" | "amd64" => Some(0x3e),
        "aarch64" | "arm64" => Some(0xb7),
        _ => None,
    }
}

/// starship 官方 release 的 musl target 三元组
fn starship_target(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" | "amd64" => Some("x86_64-unknown-linux-musl"),
        "aarch64" | "arm64" => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// 校验字节串是 64 位小端 Linux ELF 且架构匹配
fn is_linux_elf(data: &[u8], arch: &str) -> bool {
    match elf_machine(arch) {
        Some(machine) => {
            data.len() >= 20
                && data[0..4] == [0x7f, b'E', b'L', b'F']
                && data[4] == 2 // ELFCLASS64
                && data[5] == 1 // 小端
                && u16::from_le_bytes([data[18], data[19]]) == machine
        }
        None => false,
    }
}

/// 本地 starship 缓存文件路径
fn starship_cache_file(target: &str) -> Option<PathBuf> {
    dirs_next::cache_dir()
        .map(|dir| dir.join("orcaterm").join("starship").join(target))
}

/// 从 PATH 里找本机 starship
fn which_starship() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let cand = dir.join("starship");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// 解析并读取可上传的 starship 二进制:
/// 显式配置路径 -> 程序同目录 dist/starship/ -> 本地 PATH -> 本地缓存 ->
/// GitHub Releases 下载。
/// 所有候选都要求是与远端架构匹配的 Linux ELF(本机的 macOS/Windows
/// 二进制会被拒绝,避免把跑不起来的东西传到远端)。
fn resolve_local_starship(explicit: Option<&str>, arch: &str) -> anyhow::Result<Vec<u8>> {
    let target = starship_target(arch)
        .ok_or_else(|| anyhow!("unsupported remote architecture: {arch}"))?
        .to_string();

    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(p) = explicit {
        let path = PathBuf::from(p);
        if path.is_dir() {
            candidates.push(path.join("starship"));
        } else {
            candidates.push(path);
        }
    }
    // 程序同目录的 starship/ 由 build.sh 打包时附带,离线环境开箱即用
    if let Some(target) = starship_target(arch) {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("starship").join(format!("starship-{target}")));
            }
        }
    }
    if let Some(p) = which_starship() {
        candidates.push(p);
    }
    if let Some(p) = starship_cache_file(&target) {
        candidates.push(p);
    }

    for cand in &candidates {
        match std::fs::read(cand) {
            Ok(data) => {
                if is_linux_elf(&data, arch) {
                    log::info!("OrcaTerm: using local starship binary {}", cand.display());
                    return Ok(data);
                }
                log::debug!(
                    "OrcaTerm: {} is not a linux {arch} ELF, skipping",
                    cand.display()
                );
            }
            Err(err) => {
                log::debug!("OrcaTerm: failed to read {}: {err}", cand.display());
            }
        }
    }

    // 本地没有可用的,下载官方静态 musl 版并写入缓存
    let data = download_starship(&target)?;
    if let Some(cache) = starship_cache_file(&target) {
        if let Some(parent) = cache.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // 写缓存失败不影响注入,下次连接会重新下载
        let _ = std::fs::write(&cache, &data);
    }
    Ok(data)
}

/// 从 GitHub Releases 下载 starship 静态 musl 二进制并校验 sha256
fn download_starship(target: &str) -> anyhow::Result<Vec<u8>> {
    let url = format!(
        "https://github.com/starship/starship/releases/download/v{STARSHIP_VERSION}/starship-{target}.tar.gz"
    );
    log::info!("OrcaTerm: downloading starship {STARSHIP_VERSION} ({target})");
    let client = reqwest::blocking::Client::builder()
        .user_agent("OrcaTerm")
        .connect_timeout(Duration::from_secs(30))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .context("building http client")?;
    let tarball = client
        .get(&url)
        .send()
        .context("downloading starship release")?
        .error_for_status()?
        .bytes()?;

    // 官方 release 附带 .sha256 文件,校验内容完整性
    let expected = client
        .get(format!("{url}.sha256"))
        .send()?
        .error_for_status()?
        .text()?;
    let expected = expected.split_whitespace().next().unwrap_or("");
    if expected.len() == 64 {
        use sha2::{Digest, Sha256};
        let digest = hex::encode(Sha256::digest(&tarball));
        if digest != expected.to_ascii_lowercase() {
            bail!("starship download checksum mismatch: expected {expected}, got {digest}");
        }
    } else {
        log::warn!("OrcaTerm: starship sha256 unavailable, skipping verification");
    }

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&tarball[..]));
    for entry in archive.entries().context("reading starship tarball")? {
        let mut entry = entry?;
        if entry.path()?.file_name() == Some(std::ffi::OsStr::new("starship")) {
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            return Ok(data);
        }
    }
    bail!("starship binary not found in release tarball")
}

impl SessionInner {
    /// `new_pty` 入口:在 request_shell 之前尝试注入,返回用于
    /// request_exec 的启动命令;None 表示按原样 request_shell。
    pub(crate) fn maybe_inject_prompt(&mut self, sess: &mut SessionWrap) -> Option<String> {
        let mode = match self.config.get(INJECT_MODE_KEY).map(|s| s.as_str()) {
            Some("builtin") => InjectMode::Builtin,
            Some("starship") => InjectMode::Starship,
            _ => return None,
        };
        match self.inject_prompt(sess, mode) {
            Ok(Some(cmd)) => {
                log::info!("OrcaTerm: remote prompt injected");
                Some(cmd)
            }
            Ok(None) => None,
            Err(err) => {
                log::warn!(
                    "OrcaTerm: prompt injection failed, falling back to remote default shell: {err:#}"
                );
                None
            }
        }
    }

    fn inject_prompt(
        &mut self,
        sess: &mut SessionWrap,
        mode: InjectMode,
    ) -> anyhow::Result<Option<String>> {
        let probe = self.remote_probe(sess)?;

        let shell = match shell_kind(&probe) {
            Some(kind) => kind,
            None => {
                log::debug!(
                    "OrcaTerm: remote login shell `{}` does not support prompt injection",
                    probe.shell
                );
                return Ok(None);
            }
        };

        let base = format!("{}/.cache/orcaterm", probe.home);
        let bin_dir = format!("{base}/bin");

        let rc_or_zdotdir = match mode {
            InjectMode::Starship => match self.setup_starship(sess, shell, &probe, &base, &bin_dir)
            {
                Ok(path) => path,
                Err(err) => {
                    // 拿不到 starship 时降级为内置提示符,而不是放弃注入
                    log::warn!(
                        "OrcaTerm: starship unavailable ({err:#}), falling back to builtin prompt"
                    );
                    self.setup_builtin(sess, shell, &probe, &base)?
                }
            },
            InjectMode::Builtin => self.setup_builtin(sess, shell, &probe, &base)?,
        };

        Ok(Some(match shell {
            ShellKind::Bash => format!("exec bash --rcfile {}", shell_quote(&rc_or_zdotdir)),
            ShellKind::Zsh => format!(
                "export ZDOTDIR={}; exec zsh",
                shell_quote(&rc_or_zdotdir)
            ),
        }))
    }

    /// 上传 starship 二进制(如远端尚未有可用副本)和 rc。
    /// 返回 rc 路径(bash)或 ZDOTDIR 路径(zsh)。
    fn setup_starship(
        &mut self,
        sess: &mut SessionWrap,
        shell: ShellKind,
        probe: &RemoteProbe,
        base: &str,
        bin_dir: &str,
    ) -> anyhow::Result<String> {
        let remote_bin = format!("{bin_dir}/{STARSHIP_VERSION}/starship");
        if !self.remote_binary_ok(sess, &remote_bin).unwrap_or(false) {
            let bin_parent = format!("{bin_dir}/{STARSHIP_VERSION}");
            self.remote_exec_capture(
                sess,
                &format!("mkdir -p {}", shell_quote(&bin_parent)),
            )?;
            let explicit = self.config.get(STARSHIP_LOCAL_KEY).cloned();
            let data = resolve_local_starship(explicit.as_deref(), &probe.arch)
                .context("resolving a local starship binary to upload")?;
            self.upload_file(sess, &remote_bin, &data, 0o755)?;
            if !self.remote_binary_ok(sess, &remote_bin)? {
                bail!("uploaded starship binary failed to run on the remote host");
            }
        }

        let prompt_dir = format!("{base}/prompt");
        let starship_eval = format!(
            "eval \"$({} init {})\"",
            shell_quote(&remote_bin),
            shell.name()
        );
        let path_export = format!(
            "case \":$PATH:\" in *\":{bin_dir}:\"*) ;; *) export PATH={}:\"$PATH\" ;; esac",
            shell_quote(bin_dir)
        );

        let rc_content = match shell {
            ShellKind::Bash => format!(
                "# Generated by OrcaTerm prompt injection.\n\
                 [ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n\
                 {path_export}\n\
                 {starship_eval}\n"
            ),
            ShellKind::Zsh => format!(
                "# Generated by OrcaTerm prompt injection.\n\
                 ORCA_ORIG_ZDOTDIR={orig}\n\
                 ZDOTDIR=\"$ORCA_ORIG_ZDOTDIR\"; export ZDOTDIR\n\
                 [ -f \"$ORCA_ORIG_ZDOTDIR/.zshrc\" ] && . \"$ORCA_ORIG_ZDOTDIR/.zshrc\"\n\
                 {path_export}\n\
                 {starship_eval}\n",
                orig = shell_quote(orca_orig_zdotdir(probe)),
            ),
        };

        let rc = match shell {
            ShellKind::Bash => format!("{prompt_dir}/bashrc"),
            ShellKind::Zsh => format!("{prompt_dir}/zdotdir/.zshrc"),
        };
        self.upload_file(sess, &rc, rc_content.as_bytes(), 0o644)?;
        Ok(match shell {
            ShellKind::Bash => rc,
            ShellKind::Zsh => format!("{prompt_dir}/zdotdir"),
        })
    }

    /// 上传内置提示符 rc。返回 rc 路径(bash)或 ZDOTDIR 路径(zsh)。
    fn setup_builtin(
        &mut self,
        sess: &mut SessionWrap,
        shell: ShellKind,
        probe: &RemoteProbe,
        base: &str,
    ) -> anyhow::Result<String> {
        let prompt_dir = format!("{base}/prompt");

        let header = match shell {
            ShellKind::Bash => "# Generated by OrcaTerm prompt injection.\n\
                 [ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n"
                .to_string(),
            ShellKind::Zsh => format!(
                "# Generated by OrcaTerm prompt injection.\n\
                 ORCA_ORIG_ZDOTDIR={orig}\n\
                 ZDOTDIR=\"$ORCA_ORIG_ZDOTDIR\"; export ZDOTDIR\n\
                 [ -f \"$ORCA_ORIG_ZDOTDIR/.zshrc\" ] && . \"$ORCA_ORIG_ZDOTDIR/.zshrc\"\n",
                orig = shell_quote(orca_orig_zdotdir(probe)),
            ),
        };

        let rc_content = format!("{header}{BUILTIN_SCRIPT}");
        let rc = match shell {
            ShellKind::Bash => format!("{prompt_dir}/bashrc"),
            ShellKind::Zsh => format!("{prompt_dir}/zdotdir/.zshrc"),
        };
        self.upload_file(sess, &rc, rc_content.as_bytes(), 0o644)?;
        Ok(match shell {
            ShellKind::Bash => rc,
            ShellKind::Zsh => format!("{prompt_dir}/zdotdir"),
        })
    }

    /// exec 探测命令,收集远端环境并预创建注入目录
    fn remote_probe(&mut self, sess: &mut SessionWrap) -> anyhow::Result<RemoteProbe> {
        let command = format!(
            "echo \"ORCA_SHELL=$SHELL\";\
echo \"ORCA_HOME=$HOME\";\
echo \"ORCA_ARCH=$(uname -m)\";\
echo \"ORCA_ZDOTDIR=${{ZDOTDIR:-}}\";\
mkdir -p \"$HOME/.cache/orcaterm/prompt/zdotdir\" \"$HOME/.cache/orcaterm/bin\" 2>/dev/null;\
echo ORCA_DONE"
        );
        let (out, status) = self.remote_exec_capture(sess, &command)?;
        if !out.contains("ORCA_DONE") {
            bail!(
                "probe command failed (status={:?}): {out:?}",
                status.map(|s| s.exit_code())
            );
        }

        let mut probe = RemoteProbe {
            shell: String::new(),
            home: String::new(),
            arch: String::new(),
            zdotdir: String::new(),
        };
        for line in out.lines() {
            if let Some(v) = line.strip_prefix("ORCA_SHELL=") {
                probe.shell = v.to_string();
            } else if let Some(v) = line.strip_prefix("ORCA_HOME=") {
                probe.home = v.to_string();
            } else if let Some(v) = line.strip_prefix("ORCA_ARCH=") {
                probe.arch = v.to_string();
            } else if let Some(v) = line.strip_prefix("ORCA_ZDOTDIR=") {
                probe.zdotdir = v.to_string();
            }
        }
        if probe.home.is_empty() || probe.arch.is_empty() {
            bail!("probe output incomplete: {out:?}");
        }
        Ok(probe)
    }

    /// 远端二进制能否运行
    fn remote_binary_ok(
        &mut self,
        sess: &mut SessionWrap,
        remote_bin: &str,
    ) -> anyhow::Result<bool> {
        let (out, status) = self.remote_exec_capture(
            sess,
            &format!("{} --version 2>/dev/null", shell_quote(remote_bin)),
        )?;
        Ok(status.map(|s| s.success()).unwrap_or(false) && out.contains("starship"))
    }

    /// 同步执行一条远端命令并收集 stdout,返回 (stdout, exit status)。
    /// 仅在会话阻塞模式下(new_pty 内部)调用。
    fn remote_exec_capture(
        &mut self,
        sess: &mut SessionWrap,
        command_line: &str,
    ) -> anyhow::Result<(String, Option<ExitStatus>)> {
        let mut channel = sess.open_session()?;
        channel.request_exec(command_line)?;
        let mut out = Vec::new();
        channel
            .reader(0)
            .read_to_end(&mut out)
            .context("reading exec output")?;
        let status = channel.exit_status();
        channel.close();
        Ok((String::from_utf8_lossy(&out).into_owned(), status))
    }

    /// 同步执行一条远端命令并向其 stdin 写入数据(以 EOF 结束),
    /// 用于 SFTP 不可用时的 base64 上传兜底。
    fn remote_exec_stdin(
        &mut self,
        sess: &mut SessionWrap,
        command_line: &str,
        data: &[u8],
    ) -> anyhow::Result<Option<ExitStatus>> {
        let mut channel = sess.open_session()?;
        channel.request_exec(command_line)?;
        {
            let mut stdin = channel.writer();
            stdin
                .write_all(data)
                .and_then(|_| stdin.flush())
                .context("writing exec stdin")?;
        }
        channel.send_eof().context("sending EOF")?;
        let mut sink = Vec::new();
        let _ = channel.reader(0).read_to_end(&mut sink);
        let status = channel.exit_status();
        channel.close();
        Ok(status)
    }

    /// 上传文件到远端:优先 SFTP,失败时退化为 `base64 -d` over exec。
    fn upload_file(
        &mut self,
        sess: &mut SessionWrap,
        remote_path: &str,
        data: &[u8],
        unix_mode: u32,
    ) -> anyhow::Result<()> {
        if self.sftp_upload(sess, remote_path, data, unix_mode).is_ok() {
            return Ok(());
        }
        log::debug!("OrcaTerm: sftp upload failed, falling back to base64 over exec");
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        let quoted = shell_quote(remote_path);
        let command = format!(
            "base64 -d > {q}.tmp && chmod {mode:o} {q}.tmp && mv -f {q}.tmp {q}",
            q = quoted,
            mode = unix_mode,
        );
        let status = self.remote_exec_stdin(sess, &command, encoded.as_bytes())?;
        match status {
            Some(s) if s.success() => Ok(()),
            Some(s) => bail!("base64 upload failed with {}", s.exit_code()),
            None => bail!("base64 upload failed without an exit status"),
        }
    }

    fn sftp_upload(
        &mut self,
        sess: &mut SessionWrap,
        remote_path: &str,
        data: &[u8],
        unix_mode: u32,
    ) -> anyhow::Result<()> {
        let sftp = self
            .init_sftp(sess)
            .map_err(|e| anyhow!("sftp unavailable: {e}"))?;
        let mut file = sftp
            .open(
                Utf8Path::new(remote_path),
                OpenOptions {
                    read: false,
                    write: Some(WriteMode::Write),
                    mode: unix_mode as i32,
                    ty: OpenFileType::File,
                },
            )
            .map_err(|e| anyhow!("sftp open {remote_path}: {e}"))?;
        {
            let mut writer = file.writer();
            writer
                .write_all(data)
                .and_then(|_| writer.flush())
                .map_err(|e| anyhow!("sftp write {remote_path}: {e}"))?;
        }
        Ok(())
    }
}

/// 注入的 zsh rc 里用于回指用户原始 ZDOTDIR 的值;未设置时退回 $HOME
fn orca_orig_zdotdir(probe: &RemoteProbe) -> &str {
    if probe.zdotdir.is_empty() {
        &probe.home
    } else {
        &probe.zdotdir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote() {
        assert_eq!(shell_quote("/a b/c"), "'/a b/c'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn elf_arch_check() {
        // 最小 ELF 头:class=64 位、data=小端、e_machine=x86_64
        let mut elf = vec![
            0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0x3e, 0,
        ];
        assert!(is_linux_elf(&elf, "x86_64"));
        elf[18] = 0xb7; // aarch64
        assert!(is_linux_elf(&elf, "aarch64"));
        assert!(!is_linux_elf(&elf, "x86_64"));
        assert!(!is_linux_elf(&[], "x86_64"));
        assert!(!is_linux_elf(&elf, "riscv64"));
        assert!(elf_machine("riscv64").is_none());
        assert_eq!(starship_target("x86_64"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(
            starship_target("aarch64"),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(starship_target("mips"), None);
    }
}
