//! SFTP 文件浏览器独立窗口:状态与数据模型。
//!
//! 仅支持 `SshMultiplexing::None` 的 RemoteSshDomain(OrcaTerm 配置 UI
//! 保存的连接固定为该模式),通过复用已认证的 SSH Session 懒初始化
//! SFTP 子系统。功能:浏览/上传/下载/改名/删除,无断点续传。
//! SftpWindowState 是 SFTP 窗口自持状态(不挂在 TermWindow 上);
//! 渲染/交互由 sftp_window.rs 的窗口循环驱动,传输引擎在 sftp_transfer.rs。

use wezterm_ssh::{Sftp, Utf8PathBuf};

/// 列表内滚动步长(行)
pub const SFTP_SCROLL_STEP: usize = 3;

/// 独立窗口默认客户区尺寸(逻辑像素)
pub const SFTP_WINDOW_WIDTH: f32 = 520.;
pub const SFTP_WINDOW_HEIGHT: f32 = 560.;

/// 头部按钮(上级目录/刷新)
#[derive(Clone, Copy, Debug)]
pub enum SftpButton {
    ParentDir,
    Refresh,
}

#[derive(Clone, Debug)]
pub struct SidebarEntry {
    pub name: String,
    pub path: Utf8PathBuf,
    pub is_dir: bool,
    // is_symlink/size/modified 当前 UI 未展示(列表只显示文件名),
    // 供将来文件详情/大小列扩展使用
    #[allow(dead_code)]
    pub is_symlink: bool,
    #[allow(dead_code)]
    pub size: u64,
    #[allow(dead_code)]
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Debug)]
pub enum SidebarStatus {
    Loading,
    Ready,
    /// 远端操作错误信息
    Error(String),
    /// pane 不是 SSH 连接
    /// (渲染有此分支;当前入口对非 SSH pane 直接静默返回,构造点为
    /// 将来在侧栏内展示原因时启用)
    #[allow(dead_code)]
    NotSshPane,
    /// SSH 会话未建立(域存在但没 session)
    NoSession,
}

#[derive(Clone, Debug)]
pub struct RenameEdit {
    pub entry_index: usize,
    pub original: String,
    pub buffer: String,
    pub cursor: usize,
}

#[derive(Clone, Debug)]
pub struct PendingDelete {
    pub entry_index: usize,
    pub path: Utf8PathBuf,
    pub is_dir: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransferDirection {
    Upload,
    /// 下载方向当前未走 transfers 列表:拖出下载由
    /// sftp_dragout::sftp_download_to 落地到拖放目标,不经此引擎。
    /// 保留变体供将来"下载到指定目录"功能复用。
    #[allow(dead_code)]
    Download,
}

/// 传输进度条目。name/direction 当前 UI 未读取(进度列表尚未做展示),
/// 传输引擎写入、进度回填按 id 匹配。
#[derive(Clone, Debug)]
pub struct TransferItem {
    pub id: u64,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub direction: TransferDirection,
    pub total_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub finished: bool,
    pub error: Option<String>,
}

pub struct SftpWindowState {
    pub sftp: Option<Sftp>,
    pub domain_id: mux::domain::DomainId,
    /// 窗口标题里显示的 SSH 域名
    pub domain_name: String,
    /// 当前远端目录
    pub cwd: Utf8PathBuf,
    pub entries: Vec<SidebarEntry>,
    pub status: SidebarStatus,
    /// 列表首行可见索引(滚动)
    pub scroll_top: usize,
    pub selected: Option<usize>,
    pub rename: Option<RenameEdit>,
    pub pending_delete: Option<PendingDelete>,
    pub transfers: Vec<TransferItem>,
    /// 条目拖出检测:左键按下时的窗口坐标
    pub drag_start: Option<(usize, isize, isize)>,
    /// 双击检测:最近一次条目左键 (索引, 时间)
    pub last_click: Option<(usize, std::time::Instant)>,
    /// 渲染缓存(box_model);内容变化时置 None 重建
    pub(crate) computed: Option<crate::termwindow::box_model::ComputedElement>,
    /// 连续绘制失败计数(paint 失败时限次重试,成功即清零)
    pub paint_failures: usize,
    next_transfer_id: u64,
}

impl SftpWindowState {
    pub fn new(domain_id: mux::domain::DomainId, domain_name: String) -> Self {
        Self {
            sftp: None,
            domain_id,
            domain_name,
            cwd: Utf8PathBuf::from("/"),
            entries: vec![],
            status: SidebarStatus::Loading,
            scroll_top: 0,
            selected: None,
            rename: None,
            pending_delete: None,
            transfers: vec![],
            drag_start: None,
            last_click: None,
            computed: None,
            paint_failures: 0,
            next_transfer_id: 0,
        }
    }

    pub fn alloc_transfer_id(&mut self) -> u64 {
        let id = self.next_transfer_id;
        self.next_transfer_id += 1;
        id
    }

    /// 使渲染缓存失效(数据/状态变化后调用)
    pub fn invalidate(&mut self) {
        self.computed.take();
    }

    /// read_dir 结果回填;请求期间目录已切换则丢弃过期结果
    pub fn directory_loaded(
        &mut self,
        requested: Utf8PathBuf,
        result: anyhow::Result<Vec<(Utf8PathBuf, wezterm_ssh::Metadata)>>,
    ) {
        if self.cwd != requested {
            return;
        }
        match result {
            Ok(mut items) => {
                items.sort_by(|a, b| {
                    let a_dir = a.1.is_dir();
                    let b_dir = b.1.is_dir();
                    // 目录优先,其余按名称(不区分大小写)
                    b_dir.cmp(&a_dir).then_with(|| {
                        a.0
                            .file_name()
                            .unwrap_or_default()
                            .to_lowercase()
                            .cmp(&b.0.file_name().unwrap_or_default().to_lowercase())
                    })
                });
                self.entries = items
                    .into_iter()
                    .map(|(path, meta)| SidebarEntry {
                        name: path
                            .file_name()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| path.to_string()),
                        path,
                        is_dir: meta.is_dir(),
                        is_symlink: meta.is_symlink(),
                        size: meta.size.unwrap_or(0),
                        modified: meta
                            .modified
                            .and_then(|ms| std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(ms))),
                    })
                    .collect();
                self.status = SidebarStatus::Ready;
                self.selected = None;
            }
            Err(err) => {
                self.status = SidebarStatus::Error(format!("{err:#}"));
                self.entries.clear();
            }
        }
        self.invalidate();
    }
}
