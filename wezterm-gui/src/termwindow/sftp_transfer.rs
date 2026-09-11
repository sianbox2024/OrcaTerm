//! SFTP 传输引擎:上传/下载的执行线程与进度上报。
//!
//! 每个传输任务在专用 std::thread 上用 smol::block_on 逐请求驱动
//! SFTP 通道(与 GUI promise 队列完全解耦,DoDragDrop 模态期间也能继续);
//! 进度/完成经 Window::notify 投递 SftpWindowNotif 回 SFTP 窗口主循环。
//! 文件按 32KB 分块,目录递归。

use super::sftp_panel::{TransferDirection, TransferItem};
use crate::sftp_window::SftpWindowNotif;
use ::window::{Window, WindowOps};

use std::path::{Path, PathBuf};
use wezterm_ssh::{OpenFileType, OpenOptions, Sftp, Utf8PathBuf, WriteMode};

/// 分块大小
const CHUNK: usize = 32 * 1024;

/// 把一组本地文件/目录上传到 SFTP 窗口的当前远端目录。
/// `window` 是 SFTP 窗口自己的 OS 窗口(通知接收方也是它)。
pub fn sftp_upload_files(
    window: &Window,
    state: &mut super::sftp_panel::SftpWindowState,
    local_paths: Vec<PathBuf>,
) {
    let sftp = match state.sftp.clone() {
        Some(s) => s,
        None => return,
    };
    let cwd = state.cwd.clone();

    let mut jobs = vec![];
    for lp in &local_paths {
        let name = lp
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| lp.to_string_lossy().to_string());
        let id = state.alloc_transfer_id();
        state.transfers.push(TransferItem {
            id,
            name,
            direction: TransferDirection::Upload,
            total_bytes: None,
            transferred_bytes: 0,
            finished: false,
            error: None,
        });
        jobs.push((id, lp.clone()));
    }
    state.invalidate();
    let window = window.clone();

    std::thread::spawn(move || {
        for (id, lp) in &jobs {
            let dest_dir = cwd.clone();
            let sftp = sftp.clone();
            let window = window.clone();
            let r = smol::block_on(upload_one(&sftp, lp, &dest_dir, *id, &window));
            if let Err(err) = r {
                transfer_finished(&window, *id, None, Some(format!("{err:#}")));
            }
        }
    });
}

/// 上传单个文件或目录(递归)
async fn upload_one(
    sftp: &Sftp,
    local: &Path,
    dest_dir: &Utf8PathBuf,
    id: u64,
    window: &Window,
) -> anyhow::Result<()> {
    let dest = dest_dir.join(
        local
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    if local.is_dir() {
        sftp.create_dir(&dest, 0o755).await?;
        use futures_lite::stream::StreamExt;
        let mut rd = smol::fs::read_dir(local).await?;
        while let Some(entry) = rd.next().await.transpose()? {
            Box::pin(upload_one(sftp, &entry.path(), &dest, id, window)).await?;
        }
        Ok(())
    } else {
        let total = local.metadata()?.len();
        transfer_progress(window, id, Some(total), 0);
        let mut local_file = futures_lite::io::BufReader::new(smol::fs::File::open(local).await?);
        let mut remote_file = sftp
            .open_with_mode(
                &dest,
                OpenOptions {
                    read: false,
                    write: Some(WriteMode::Write),
                    mode: 0o644,
                    ty: OpenFileType::File,
                },
            )
            .await?;
        use futures_lite::io::AsyncReadExt;
        let mut buf = vec![0u8; CHUNK];
        let mut transferred: u64 = 0;
        loop {
            let n = local_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            use futures_lite::io::AsyncWriteExt;
            remote_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            transfer_progress(window, id, Some(total), transferred);
        }
        transfer_finished(window, id, Some(transferred), None);
        Ok(())
    }
}

/// 进度上报(传输线程调用,跨线程投递到 SFTP 窗口)
fn transfer_progress(window: &Window, id: u64, total: Option<u64>, done: u64) {
    let _ = window.notify(SftpWindowNotif::Apply(Box::new(move |sw| {
        if let Some(t) = sw.state.transfers.iter_mut().find(|t| t.id == id) {
            t.total_bytes = total;
            t.transferred_bytes = done;
        }
        sw.state.invalidate();
        if let Some(w) = sw.tw.window.as_ref() {
            w.invalidate();
        }
    })));
}

/// 完成/失败上报(传输线程调用)
fn transfer_finished(window: &Window, id: u64, done: Option<u64>, error: Option<String>) {
    let _ = window.notify(SftpWindowNotif::Apply(Box::new(move |sw| {
        if let Some(t) = sw.state.transfers.iter_mut().find(|t| t.id == id) {
            if let Some(done) = done {
                t.transferred_bytes = done;
            }
            t.finished = true;
            t.error = error.clone();
        }
        // 全部完成时刷新目录;有错误也刷(部分文件可能已写入)
        if sw.state.transfers.iter().all(|t| t.finished) {
            sw.schedule_refresh();
        }
        sw.state.invalidate();
        if let Some(w) = sw.tw.window.as_ref() {
            w.invalidate();
        }
    })));
}

/// 下载远端文件到本地目录(拖出用):在调用线程 block_on 全量下载,
/// 返回本地文件路径。目录递归下载。
/// `progress` 回调在传输线程同步调用(轻量,勿阻塞)。
pub fn sftp_download_to(
    sftp: &Sftp,
    remote: &Utf8PathBuf,
    is_dir: bool,
    local_dir: &Path,
    progress: &dyn Fn(u64, u64),
) -> anyhow::Result<PathBuf> {
    let name = remote
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| remote.to_string());
    let local = local_dir.join(&name);
    smol::block_on(download_one(sftp, remote, is_dir, &local, progress))?;
    Ok(local)
}

async fn download_one(
    sftp: &Sftp,
    remote: &Utf8PathBuf,
    is_dir: bool,
    local: &Path,
    progress: &dyn Fn(u64, u64),
) -> anyhow::Result<()> {
    if is_dir {
        std::fs::create_dir_all(local)?;
        let items = sftp.read_dir(remote).await?;
        for (child, meta) in items {
            Box::pin(download_one(
                sftp,
                &child,
                meta.is_dir(),
                &local.join(child.file_name().map(|s| s.to_string()).unwrap_or_default()),
                progress,
            ))
            .await?;
        }
        Ok(())
    } else {
        let meta = sftp.metadata(remote).await?;
        let total = meta.size.unwrap_or(0);
        let mut remote_file = sftp.open(remote).await?;
        let mut local_file = smol::fs::File::create(local).await?;
        use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = vec![0u8; CHUNK];
        let mut transferred: u64 = 0;
        loop {
            let n = remote_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            local_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            progress(total, transferred);
        }
        Ok(())
    }
}
