//! SFTP 独立窗口:复用 TermWindow 作渲染宿主的第二个 OS 窗口。
//!
//! 与主终端窗口(经 mux 的 tab/pane 体系)不同,SFTP 窗口直接
//! Window::new_window 创建自己的 OS 窗口:事件回调挂在 SftpWindowHost
//! (一个简化构造的 TermWindow 实例——box_model 渲染方法、字体、
//! GL 资源全套可用,但不注册到 frontend 的 known_windows、不参与
//! workspace reconcile、无 mux 订阅,窗口关闭即销毁)。
//! SFTP 数据/交互状态在 SftpWindowState(见 sftp_panel.rs),
//! 传输引擎在 sftp_transfer.rs。
//! 入口 = open_for_active_pane(标签栏 SFTP 按钮 / CTRL+SHIFT+F /
//! 命令面板),每域一窗,重复打开聚焦既有窗口。

use crate::termwindow::sftp_panel::{
    SftpButton, SFTP_WINDOW_HEIGHT, SFTP_WINDOW_WIDTH, SFTP_SCROLL_STEP, PendingDelete,
    RenameEdit, SidebarStatus, SftpWindowState,
};
use crate::termwindow::sftp_transfer;
use crate::termwindow::TermWindow;
use config::{Dimension, DimensionContext, GeometryOrigin};
use ::window::{
    RequestedWindowGeometry, Window, WindowEvent, WindowOps,
};
use std::cell::RefCell;
use std::rc::Rc;
use wezterm_ssh::Utf8PathBuf;
use wezterm_term::color::ColorPalette;

/// SFTP 窗口事件载荷(Window::notify 的 Any,与主窗口的 TermWindowNotif
/// downcast 互不干扰:不同窗口各自的 events 队列)
pub enum SftpWindowNotif {
    /// 闭包(SFTP 窗口事件线程执行)
    Apply(Box<dyn FnOnce(&mut SftpWindowHost) + Send + Sync>),
    /// 异步目录读取完成回填
    DirectoryLoaded(
        Utf8PathBuf,
        anyhow::Result<Vec<(Utf8PathBuf, wezterm_ssh::Metadata)>>,
    ),
}

/// SFTP 窗口宿主:渲染/窗口资源走 TermWindow(构造自足、无 mux 依赖),
/// SFTP 业务状态在 state。
pub struct SftpWindowHost {
    pub tw: TermWindow,
    pub state: SftpWindowState,
}

impl SftpWindowHost {
    /// 异步读取远端目录,完成后经 SftpWindowNotif 回填
    pub fn load_directory(&mut self, path: Utf8PathBuf) {
        let Some(sftp) = self.state.sftp.clone() else {
            return;
        };
        let requested = path.clone();
        self.state.status = SidebarStatus::Loading;
        self.state.cwd = requested.clone();
        self.state.scroll_top = 0;
        self.state.rename = None;
        self.state.pending_delete = None;
        self.state.entries.clear();
        self.state.invalidate();
        let Some(window) = self.tw.window.clone() else {
            return;
        };
        promise::spawn::spawn(async move {
            let result = sftp.read_dir(&requested).await.map_err(anyhow::Error::new);
            let _ = window.notify(SftpWindowNotif::DirectoryLoaded(requested, result));
        })
        .detach();
    }

    /// 刷新当前目录
    pub fn schedule_refresh(&mut self) {
        let cwd = self.state.cwd.clone();
        self.load_directory(cwd);
    }

    /// 进入目录 / 回上级
    pub fn navigate(&mut self, path: Utf8PathBuf) {
        if self.state.sftp.is_none() {
            return;
        }
        self.load_directory(path);
    }

    fn paint(&mut self) -> bool {
        let gl = match self.tw.gl.as_ref() {
            Some(gl) => Rc::clone(gl),
            None => return false,
        };
        let width = self.tw.dimensions.pixel_width;
        let height = self.tw.dimensions.pixel_height;
        if width == 0 || height == 0 {
            return false;
        }

        let frame = ::window::glium::Frame::new(
            Rc::clone(&gl),
            (width as u32, height as u32),
        );
        // 多 pass 直到 quad 分配完毕(与主窗口 paint_impl 同语义)
        loop {
            if let Err(err) = self.paint_pass(width, height) {
                log::error!("sftp window paint_pass failed: {err:#}");
                return false;
            }
            let more = self
                .tw
                .render_state
                .as_mut()
                .unwrap()
                .allocated_more_quads();
            match more {
                Ok(false) => break,
                Ok(true) => {
                    self.state.invalidate();
                }
                Err(_) => return false,
            }
        }
        let window = self.tw.window.as_ref().unwrap();
        let ok = window.finish_frame(frame).is_ok();
        if !ok {
            log::error!("sftp window finish_frame failed");
        }
        ok
    }

    fn paint_pass(
        &mut self,
        width: usize,
        height: usize,
    ) -> anyhow::Result<()> {
        use crate::termwindow::box_model::LayoutContext;
        // 先做纯数据阶段:palette 懒加载 + 构建 Element 树(&mut self)
        let content_and_ctx = match self.state.computed.take() {
            Some(_) => None,
            None => Some(self.build_element_tree(width, height)?),
        };
        // compute/render 阶段:只共享借用 render_state
        let computed = match content_and_ctx {
            None => self
                .state
                .computed
                .take()
                .expect("computed cached above"),
            Some((content, metrics)) => {
                let render_state = match self.tw.render_state.as_ref() {
                    Some(rs) => rs,
                    None => anyhow::bail!("no render state"),
                };
                let mut computed = self.tw.compute_element(
                    &LayoutContext {
                        width: DimensionContext {
                            dpi: self.tw.dimensions.dpi as f32,
                            pixel_max: width as f32,
                            pixel_cell: metrics.cell_size.width as f32,
                        },
                        height: DimensionContext {
                            dpi: self.tw.dimensions.dpi as f32,
                            pixel_max: height as f32,
                            pixel_cell: metrics.cell_size.height as f32,
                        },
                        bounds: euclid::rect(0., 0., width as f32, height as f32),
                        metrics: &metrics,
                        gl_state: render_state,
                        zindex: 0,
                    },
                    &content,
                )?;
                // 坐标系原点在窗口中心:translate 到左上角
                computed.translate(euclid::vec2(
                    -(width as f32) / 2.,
                    -(height as f32) / 2.,
                ));
                computed
            }
        };
        let ui_items = computed.ui_items();
        let render_state = match self.tw.render_state.as_ref() {
            Some(rs) => rs,
            None => anyhow::bail!("no render state"),
        };
        self.tw.render_element(&computed, render_state, None)?;
        self.state.computed = Some(computed);
        self.tw.ui_items.clear();
        self.tw.ui_items.extend(ui_items);
        Ok(())
    }

    /// 纯数据阶段:构建 Element 树(需要 &mut self:palette 懒加载)。
    /// 返回 (树, 渲染度量)——compute 阶段再借 render_state。
    #[allow(clippy::type_complexity)]
    fn build_element_tree(
        &mut self,
        width: usize,
        height: usize,
    ) -> anyhow::Result<(
        crate::termwindow::box_model::Element,
        crate::utilsprites::RenderMetrics,
    )> {
        use crate::termwindow::box_model::{
            BorderColor, BoxDimension, DisplayType, Element, ElementColors, ElementContent,
            Float, VerticalAlign,
        };
        use crate::utilsprites::RenderMetrics;

        let font = self.tw.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let palette: ColorPalette = self.palette_for_sftp();

        let fg = palette.foreground.to_linear();
        let bg = palette.background.to_linear();
        let header_colors = ElementColors {
            border: BorderColor::default(),
            bg: bg.into(),
            text: fg.into(),
        };
        let line_h: f64 = 1.6;
        let state = &self.state;

        // 头部:路径 + 上级 + 刷新
        let mut header_children = vec![];
        let cwd_display = {
            let s = state.cwd.as_str().trim_start_matches('/');
            if s.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", s)
            }
        };
        let cwd_el = Element::new(
            &font,
            ElementContent::Text(format!(" {} ", cwd_display)),
        )
        .max_width(Some(Dimension::Pixels(width as f32 - 80.)))
        .line_height(Some(line_h))
        .vertical_align(VerticalAlign::Middle)
        .colors(header_colors.clone());
        header_children.push(cwd_el);

        let hover_colors = || ElementColors {
            border: BorderColor::default(),
            bg: pidx(&palette, 60).to_linear().into(),
            text: palette.foreground.to_linear().into(),
        };
        let parent_el = Element::new(&font, ElementContent::Text(" ⬆ ".into()))
            .item_type(super::termwindow::UIItemType::SftpParentDir)
            .line_height(Some(line_h))
            .vertical_align(VerticalAlign::Middle)
            .padding(BoxDimension {
                left: Dimension::Cells(0.3),
                right: Dimension::Cells(0.3),
                top: Dimension::Cells(0.1),
                bottom: Dimension::Cells(0.1),
            })
            .colors(header_colors.clone())
            .hover_colors(Some(hover_colors()));
        header_children.push(parent_el);

        let refresh_el = Element::new(&font, ElementContent::Text(" ⟳ ".into()))
            .item_type(super::termwindow::UIItemType::SftpRefresh)
            .line_height(Some(line_h))
            .vertical_align(VerticalAlign::Middle)
            .float(Float::Right)
            .padding(BoxDimension {
                left: Dimension::Cells(0.3),
                right: Dimension::Cells(0.3),
                top: Dimension::Cells(0.1),
                bottom: Dimension::Cells(0.1),
            })
            .colors(header_colors.clone())
            .hover_colors(Some(hover_colors()));
        header_children.push(refresh_el);

        let header = Element::new(&font, ElementContent::Children(header_children))
            .min_width(Some(Dimension::Pixels(width as f32)))
            .min_height(Some(Dimension::Pixels(
                metrics.cell_size.height as f32 * 1.6,
            )))
            .line_height(Some(line_h))
            .colors(header_colors.clone());

        // 列表
        let mut list_children = vec![];
        match &state.status {
            SidebarStatus::NotSshPane => {
                let el = Element::new(
                    &font,
                    ElementContent::Text(" 当前 pane 不是 SSH 连接\n 无法浏览文件".to_string()),
                )
                .line_height(Some(line_h))
                .padding(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.5),
                    bottom: Dimension::Cells(0.),
                })
                .colors(header_colors.clone());
                list_children.push(el);
            }
            SidebarStatus::NoSession => {
                let el = Element::new(
                    &font,
                    ElementContent::Text(" SSH 会话未建立\n 请先连接 SSH".to_string()),
                )
                .line_height(Some(line_h))
                .padding(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.5),
                    bottom: Dimension::Cells(0.),
                })
                .colors(header_colors.clone());
                list_children.push(el);
            }
            SidebarStatus::Loading => {
                let el = Element::new(&font, ElementContent::Text(" 加载中...".to_string()))
                    .line_height(Some(line_h))
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.5),
                        bottom: Dimension::Cells(0.),
                    })
                    .colors(header_colors.clone());
                list_children.push(el);
            }
            SidebarStatus::Error(msg) => {
                let el =
                    Element::new(&font, ElementContent::Text(format!(" {}\n 按 ⟳ 重试", msg)))
                        .line_height(Some(line_h))
                        .padding(BoxDimension {
                            left: Dimension::Cells(0.5),
                            right: Dimension::Cells(0.),
                            top: Dimension::Cells(0.5),
                            bottom: Dimension::Cells(0.),
                        })
                        .colors(header_colors.clone());
                list_children.push(el);
            }
            SidebarStatus::Ready => {
                let header_h = metrics.cell_size.height as f32 * 1.6;
                let visible_rows = ((height as f32 - header_h)
                    / (metrics.cell_size.height as f32 * line_h as f32))
                    as usize;
                for (row, entry) in state
                    .entries
                    .iter()
                    .enumerate()
                    .skip(state.scroll_top)
                    .take(visible_rows)
                {
                    let display = if entry.is_dir {
                        format!("{}/", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let is_renaming = state
                        .rename
                        .as_ref()
                        .map(|r| r.entry_index == row)
                        .unwrap_or(false);
                    let text = if is_renaming {
                        format!("{}▏", state.rename.as_ref().unwrap().buffer)
                    } else {
                        display
                    };
                    let selected = state.selected == Some(row);
                    let pending_del = state
                        .pending_delete
                        .as_ref()
                        .map(|d| d.entry_index == row)
                        .unwrap_or(false);

                    let (ebg, text_color) = if pending_del {
                        (pidx(&palette, 52), pidx(&palette, 15))
                    } else if selected {
                        (pidx(&palette, 24), palette.foreground)
                    } else if entry.is_dir {
                        (palette.background, pidx(&palette, 39))
                    } else {
                        (palette.background, palette.foreground)
                    };

                    let el = Element::new(&font, ElementContent::Text(text))
                        .item_type(super::termwindow::UIItemType::SftpEntry(row))
                        .line_height(Some(line_h))
                        .max_width(Some(Dimension::Pixels(width as f32 - 4.)))
                        .padding(BoxDimension {
                            left: Dimension::Cells(0.3),
                            right: Dimension::Cells(0.),
                            top: Dimension::Cells(0.),
                            bottom: Dimension::Cells(0.),
                        })
                        .colors(ElementColors {
                            border: BorderColor::default(),
                            bg: ebg.to_linear().into(),
                            text: text_color.to_linear().into(),
                        })
                        .hover_colors(Some(ElementColors {
                            border: BorderColor::default(),
                            bg: pidx(&palette, 60).to_linear().into(),
                            text: palette.foreground.to_linear().into(),
                        }));
                    list_children.push(el);
                }
            }
        }

        let list = Element::new(&font, ElementContent::Children(list_children))
            .min_width(Some(Dimension::Pixels(width as f32)))
            .display(DisplayType::Block)
            .item_type(super::termwindow::UIItemType::SftpSidebar)
            .line_height(Some(line_h))
            .colors(header_colors.clone());

        let content = Element::new(&font, ElementContent::Children(vec![header, list]))
            .display(DisplayType::Block)
            .min_width(Some(Dimension::Pixels(width as f32)))
            .min_height(Some(Dimension::Pixels(height as f32)))
            .line_height(Some(line_h))
            .colors(ElementColors {
                border: BorderColor::new(pidx(&palette, 8).to_linear()),
                bg: bg.into(),
                text: fg.into(),
            });

        Ok((content, metrics))
    }

    /// SFTP 窗口配色:与主窗口同源(TermConfig::color_palette,
    /// 随 color_scheme 解析)。palette 懒初始化需要 &mut,这里在
    /// open_for_active_pane 阶段已由首次 paint 前预热——取不到时回退默认。
    fn palette_for_sftp(&mut self) -> ColorPalette {
        self.tw.palette().clone()
    }
}

/// 取 256 色调色板索引色(sRGB)
fn pidx(palette: &ColorPalette, idx: u8) -> wezterm_term::color::SrgbaTuple {
    palette.colors.0[idx as usize]
}

// ---------------------------------------------------------------------------
// 交互(从侧栏版平移,改挂 SftpWindowHost)
// ---------------------------------------------------------------------------

impl SftpWindowHost {
    /// 条目行左键:选中/双击进目录/两击确认删除;右键:标记删除
    pub fn entry_clicked(
        &mut self,
        idx: usize,
        button: ::window::MousePress,
        streak_two: bool,
    ) {
        match button {
            ::window::MousePress::Left => {
                if self.state.rename.is_some() {
                    self.commit_rename();
                    return;
                }
                let already_pending = self
                    .state
                    .pending_delete
                    .as_ref()
                    .map(|d| d.entry_index == idx)
                    .unwrap_or(false);
                if already_pending {
                    self.commit_delete();
                    return;
                }
                self.state.pending_delete = None;
                self.state.selected = Some(idx);
                self.state.invalidate();
                self.invalidate_window();
                if streak_two {
                    let entry = self.state.entries.get(idx).cloned();
                    if let Some(entry) = entry.filter(|e| e.is_dir) {
                        self.navigate(entry.path.clone());
                    }
                }
            }
            ::window::MousePress::Right => {
                self.state.selected = Some(idx);
                self.request_delete(idx);
                self.invalidate_window();
            }
            _ => {}
        }
    }

    /// 滚轮滚动列表
    pub fn scroll(&mut self, amount: isize) {
        let max_top = self.state.entries.len().saturating_sub(1);
        if amount > 0 {
            self.state.scroll_top = self
                .state
                .scroll_top
                .saturating_sub(amount as usize * SFTP_SCROLL_STEP);
        } else {
            self.state.scroll_top = (self.state.scroll_top
                + (-amount) as usize * SFTP_SCROLL_STEP)
                .min(max_top);
        }
        self.state.invalidate();
        self.invalidate_window();
    }

    /// 条目拖出检测:按住左键且距按下点超过 6px 触发
    pub fn dragout_check(&mut self, coords: (isize, isize), left_down: bool) {
        let Some((idx, sx, sy)) = self.state.drag_start else {
            return;
        };
        if !left_down {
            return;
        }
        let dx = coords.0 - sx;
        let dy = coords.1 - sy;
        if dx * dx + dy * dy > 36 {
            self.state.drag_start = None;
            self.begin_dragout(idx);
        }
    }

    /// 启动拖出下载(Windows OLE;其他平台暂不支持)
    #[cfg(windows)]
    fn begin_dragout(&mut self, idx: usize) {
        let Some(entry) = self.state.entries.get(idx).cloned() else {
            return;
        };
        let Some(sftp) = self.state.sftp.clone() else {
            return;
        };
        let temp_dir = std::env::temp_dir().join(format!(
            "orcaterm-sftp-dragout-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        self.invalidate_window();
        // DoDragDrop 模态循环里传输线程独立,无死锁
        let downloads = vec![(sftp, entry.path.clone(), entry.is_dir)];
        let cleanup_dir = temp_dir.clone();
        unsafe {
            if let Err(err) = crate::sftp_dragout::sftp_start_drag_out(downloads, temp_dir) {
                log::error!("sftp dragout failed: {err:#}");
            }
        }
        let _ = std::fs::remove_dir_all(&cleanup_dir);
        self.schedule_refresh();
    }

    #[cfg(not(windows))]
    fn begin_dragout(&mut self, _idx: usize) {}

    /// 拖入上传:把本地文件列表上传到当前目录
    pub fn upload_files(&mut self, local_paths: Vec<std::path::PathBuf>) {
        let Some(w) = self.tw.window.clone() else {
            return;
        };
        sftp_transfer::sftp_upload_files(&w, &mut self.state, local_paths);
    }

    /// 头部按钮
    pub fn button_clicked(&mut self, button: SftpButton) {
        match button {
            SftpButton::ParentDir => {
                if let Some(parent) = self.state.cwd.parent() {
                    let parent = if parent.as_str().is_empty() {
                        Utf8PathBuf::from("/")
                    } else {
                        parent.to_path_buf()
                    };
                    self.navigate(parent);
                }
            }
            SftpButton::Refresh => {
                self.schedule_refresh();
            }
        }
    }

    // ---- 改名 ----

    pub fn start_rename(&mut self, idx: usize) {
        if let Some(entry) = self.state.entries.get(idx) {
            self.state.rename = Some(RenameEdit {
                entry_index: idx,
                original: entry.name.clone(),
                buffer: entry.name.clone(),
                cursor: entry.name.len(),
            });
            self.state.invalidate();
            self.invalidate_window();
        }
    }

    pub fn rename_input(&mut self, ch: char) {
        if let Some(rename) = self.state.rename.as_mut() {
            rename.buffer.push(ch);
            rename.cursor = rename.buffer.len();
            self.state.invalidate();
            self.invalidate_window();
        }
    }

    pub fn rename_backspace(&mut self) {
        if let Some(rename) = self.state.rename.as_mut() {
            rename.buffer.pop();
            rename.cursor = rename.buffer.len();
            self.state.invalidate();
            self.invalidate_window();
        }
    }

    pub fn cancel_edit(&mut self) {
        self.state.rename = None;
        self.state.pending_delete = None;
        self.state.invalidate();
        self.invalidate_window();
    }

    pub fn commit_rename(&mut self) {
        let Some(rename) = self.state.rename.clone() else {
            return;
        };
        self.state.rename = None;
        let new_name = rename.buffer.trim();
        if new_name.is_empty() || new_name == rename.original {
            self.state.invalidate();
            self.invalidate_window();
            return;
        }
        let Some(entry) = self.state.entries.get(rename.entry_index).cloned() else {
            return;
        };
        let Some(sftp) = self.state.sftp.clone() else {
            return;
        };
        let dest = entry.path.parent().unwrap_or(&self.state.cwd).join(new_name);
        let Some(window) = self.tw.window.clone() else {
            return;
        };
        promise::spawn::spawn(async move {
            let result = sftp
                .rename(
                    entry.path.clone(),
                    dest.clone(),
                    wezterm_ssh::RenameOptions {
                        overwrite: false,
                        atomic: true,
                        native: false,
                    },
                )
                .await;
            let _ = window.notify(SftpWindowNotif::Apply(Box::new(move |sw| {
                if let Err(err) = &result {
                    sw.state.status = SidebarStatus::Error(format!("改名失败: {err:#}"));
                }
                sw.schedule_refresh();
            })));
        })
        .detach();
    }

    // ---- 删除 ----

    pub fn request_delete(&mut self, idx: usize) {
        if let Some(entry) = self.state.entries.get(idx) {
            self.state.pending_delete = Some(PendingDelete {
                entry_index: idx,
                path: entry.path.clone(),
                is_dir: entry.is_dir,
            });
            self.state.invalidate();
            self.invalidate_window();
        }
    }

    pub fn commit_delete(&mut self) {
        let Some(pending) = self.state.pending_delete.clone() else {
            return;
        };
        self.state.pending_delete = None;
        let Some(sftp) = self.state.sftp.clone() else {
            return;
        };
        let Some(window) = self.tw.window.clone() else {
            return;
        };
        promise::spawn::spawn(async move {
            // 目录:递归删除;文件:直接删
            async fn remove_tree(
                sftp: &wezterm_ssh::Sftp,
                path: &Utf8PathBuf,
                is_dir: bool,
            ) -> anyhow::Result<()> {
                if is_dir {
                    let items = sftp.read_dir(path).await?;
                    for (child, meta) in items {
                        let mut is_dir = meta.is_dir();
                        // 符号链接直接 unlink,不跟随
                        if meta.is_symlink() {
                            is_dir = false;
                        }
                        Box::pin(remove_tree(sftp, &child, is_dir)).await?;
                    }
                    sftp.remove_dir(path).await?;
                } else {
                    sftp.remove_file(path).await?;
                }
                Ok(())
            }
            let result = Box::pin(remove_tree(&sftp, &pending.path, pending.is_dir)).await;
            let _ = window.notify(SftpWindowNotif::Apply(Box::new(move |sw| {
                if let Err(err) = &result {
                    sw.state.status = SidebarStatus::Error(format!("删除失败: {err:#}"));
                }
                sw.schedule_refresh();
            })));
        })
        .detach();
    }

    // ---- 键盘 ----

    /// 键盘处理:改名态吃键;选中条目支持 F2/Delete/F5
    pub fn handle_key(&mut self, key: &::window::KeyCode) {
        use ::window::KeyCode;
        if self.state.rename.is_some() {
            match key {
                KeyCode::Char('\r') => self.commit_rename(),
                KeyCode::Char('\u{1b}') => self.cancel_edit(),
                KeyCode::Char('\u{8}') => self.rename_backspace(),
                KeyCode::Char(c) => self.rename_input(*c),
                _ => {}
            }
            self.invalidate_window();
            return;
        }
        let selected = self.state.selected;
        if let Some(idx) = selected {
            match key {
                KeyCode::Function(2) => self.start_rename(idx),
                KeyCode::Char('\u{7f}') => self.request_delete(idx),
                KeyCode::Function(5) => self.schedule_refresh(),
                _ => {}
            }
        }
        self.invalidate_window();
    }

    fn invalidate_window(&mut self) {
        if let Some(w) = self.tw.window.as_ref() {
            w.invalidate();
        }
    }
}

// ---------------------------------------------------------------------------
// 窗口创建与事件分发
// ---------------------------------------------------------------------------

// 已打开的 SFTP 窗口登记(域 ID → 宿主)。事件回调线程与
// open_for_active_pane(主线程)通过 RefCell 访问;窗口关闭时注销。
thread_local! {
    static OPEN_WINDOWS: RefCell<Vec<(mux::domain::DomainId, Rc<RefCell<SftpWindowHost>>)>> =
        RefCell::new(vec![]);
}

/// 入口:为当前活动 pane 的 SSH 域打开(或聚焦)SFTP 窗口。
/// 非 SSH pane:不开窗(按钮侧已灰化,这里兜底静默返回)。
pub fn open_for_active_pane(tw: &mut TermWindow) {
    let mux = mux::Mux::get();
    let Some(pane) = tw.get_active_pane_or_overlay() else {
        return;
    };
    let domain_id = pane.domain_id();
    let Some(domain) = mux.get_domain(domain_id) else {
        return;
    };
    let Some(ssh_domain) = domain.downcast_ref::<mux::ssh::RemoteSshDomain>() else {
        return;
    };

    // 已开:聚焦既有窗口
    let existing = OPEN_WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .find(|(id, _)| *id == domain_id)
            .map(|(_, sw)| Rc::clone(sw))
    });
    if let Some(sw) = existing {
        if let Some(w) = sw.borrow().tw.window.as_ref() {
            w.show();
        }
        return;
    }

    use mux::domain::Domain as _;
    let domain_name = ssh_domain.domain_name().to_string();
    let mut state = SftpWindowState::new(domain_id, domain_name);
    match ssh_domain.session() {
        Some(session) => {
            state.sftp = Some(session.sftp());
        }
        None => {
            state.status = SidebarStatus::NoSession;
        }
    }

    promise::spawn::spawn(async move {
        if let Err(err) = sftp_window_main(domain_id, state).await {
            log::error!("SFTP window error: {err:#}");
            OPEN_WINDOWS.with(|w| w.borrow_mut().retain(|(id, _)| *id != domain_id));
        }
    })
    .detach();
}

async fn sftp_window_main(
    domain_id: mux::domain::DomainId,
    state: SftpWindowState,
) -> anyhow::Result<()> {
    let host = Rc::new(RefCell::new(SftpWindowHost {
        tw: TermWindow::new_sftp_host().await?,
        state,
    }));
    OPEN_WINDOWS.with(|w| w.borrow_mut().push((domain_id, Rc::clone(&host))));

    let config = config::configuration();
    let geometry = RequestedWindowGeometry {
        width: Dimension::Pixels(SFTP_WINDOW_WIDTH),
        height: Dimension::Pixels(SFTP_WINDOW_HEIGHT),
        x: None,
        y: None,
        origin: GeometryOrigin::default(),
    };
    let event_host = Rc::clone(&host);
    let window = Window::new_window(
        &super::termwindow::get_window_class(),
        &format!("SFTP - {}", host.borrow().state.domain_name),
        geometry,
        Some(&config),
        Rc::clone(&host.borrow().tw.fonts),
        move |event, window| {
            // 创建期(created() 内字体/GL 初始化会同步 pump Windows 消息)
            // 本回调可能被重入,此时主流程仍持有 host 借用;创建期的
            // FocusChanged/NeedRepaint 丢弃无害(后续事件会补上),绝不 panic。
            let Ok(mut host) = event_host.try_borrow_mut() else {
                log::debug!("sftp event dropped during host init: {event:?}");
                return;
            };
            if let Err(err) = dispatch_sftp_event(&mut host, event, window) {
                log::error!("sftp window event: {err:#}");
            }
        },
    )
    .await?;

    {
        let mut host = host.borrow_mut();
        host.tw.window = Some(window.clone());
    }
    // 初始化链上任一步失败都关窗:留下未渲染的白壳窗口只会误导用户
    if let Err(err) = TermWindow::apply_icon(&window) {
        window.close();
        return Err(err);
    }

    let gl = match window.enable_opengl().await {
        Ok(gl) => gl,
        Err(err) => {
            window.close();
            return Err(err);
        }
    };
    {
        let mut host = host.borrow_mut();
        if let Err(err) = host.tw.created(crate::renderstate::RenderContext::Glium(gl)) {
            drop(host);
            window.close();
            return Err(err);
        }
    }

    // 初始目录
    {
        let mut host = host.borrow_mut();
        if host.state.sftp.is_some() {
            let root = host.state.cwd.clone();
            host.load_directory(root);
        }
    }
    window.invalidate();

    // 窗口关闭时从登记表移除;Rc 释放后宿主销毁
    Ok(())
}

/// SFTP 窗口事件分发(窄化版 dispatch:只处理渲染与交互必需事件)
fn dispatch_sftp_event(
    host: &mut SftpWindowHost,
    event: WindowEvent,
    window: &Window,
) -> anyhow::Result<bool> {
    match event {
        WindowEvent::Destroyed => {
            let domain_id = host.state.domain_id;
            OPEN_WINDOWS.with(|w| w.borrow_mut().retain(|(id, _)| *id != domain_id));
            Ok(false)
        }
        WindowEvent::CloseRequested => {
            window.close();
            Ok(true)
        }
        WindowEvent::Notification(item) => {
            if let Ok(notif) = item.downcast::<SftpWindowNotif>() {
                match *notif {
                    SftpWindowNotif::Apply(f) => f(host),
                    SftpWindowNotif::DirectoryLoaded(requested, result) => {
                        host.state.directory_loaded(requested, result);
                    }
                }
                window.invalidate();
            }
            Ok(true)
        }
        WindowEvent::NeedRepaint => {
            if !host.paint() {
                // 绘制失败(GL 未就绪/首帧异常等):限次重排重试,避免
                // 首帧失败后没有下一次 NeedRepaint 导致窗口永远空白。
                host.state.paint_failures += 1;
                if host.state.paint_failures <= 10 {
                    window.invalidate();
                } else if host.state.paint_failures == 11 {
                    log::error!(
                        "sftp window paint failed {} times consecutively; giving up",
                        host.state.paint_failures - 1
                    );
                }
            } else {
                host.state.paint_failures = 0;
            }
            Ok(true)
        }
        WindowEvent::Resized {
            dimensions,
            window_state: _,
            live_resizing: _,
        } => {
            host.tw.dimensions = dimensions;
            host.state.invalidate();
            window.invalidate();
            Ok(true)
        }
        WindowEvent::FocusChanged(focused) => {
            host.tw.focused = focused.then(std::time::Instant::now);
            Ok(true)
        }
        WindowEvent::MouseEvent(event) => {
            host.tw.current_mouse_event = Some(event.clone());
            sftp_mouse_event(host, event);
            Ok(true)
        }
        WindowEvent::MouseLeave => {
            host.tw.current_mouse_event = None;
            window.invalidate();
            Ok(true)
        }
        WindowEvent::KeyEvent(event) => {
            if event.key_is_down {
                host.handle_key(&event.key);
            }
            Ok(true)
        }
        WindowEvent::DroppedFile(paths) => {
            if !paths.is_empty() {
                host.upload_files(paths);
            }
            Ok(true)
        }
        WindowEvent::DraggedFile(paths) => {
            // 拖入即视作将上传(提示态);真正上传在 DroppedFile
            let _ = paths;
            Ok(true)
        }
        WindowEvent::AppearanceChanged(_) => Ok(true),
        WindowEvent::SetInnerSizeCompleted => Ok(true),
        WindowEvent::AdviseDeadKeyStatus(_) => Ok(true),
        _ => Ok(true),
    }
}

/// SFTP 窗口鼠标交互:命中 ui_items 上的 Sftp* 项分发
fn sftp_mouse_event(host: &mut SftpWindowHost, event: ::window::MouseEvent) {
    use ::window::{MouseButtons, MousePress};
    let (x, y) = (event.coords.x, event.coords.y);
    match event.kind {
        ::window::MouseEventKind::Press(button) => {
            // 命中测试(坐标 → ui_item)
            let hit = host
                .tw
                .ui_items
                .iter()
                .find(|item| {
                    x >= item.x as isize
                        && x < (item.x + item.width) as isize
                        && y >= item.y as isize
                        && y < (item.y + item.height) as isize
                })
                .cloned();
            let Some(item) = hit else {
                return;
            };
            match item.item_type {
                super::termwindow::UIItemType::SftpEntry(idx) => {
                    // 拖出起点记录
                    if button == MousePress::Left {
                        host.state.drag_start = Some((idx, x, y));
                    }
                    // 双击检测:同一条目 500ms 内二次左键
                    let now = std::time::Instant::now();
                    let streak_two = host
                        .state
                        .rename
                        .is_none()
                        && button == MousePress::Left
                        && host
                            .state
                            .last_click
                            .as_ref()
                            .map(|(prev_idx, prev_time)| {
                                *prev_idx == idx
                                    && now.duration_since(*prev_time).as_millis() < 500
                            })
                            .unwrap_or(false);
                    if button == MousePress::Left {
                        host.state.last_click = Some((idx, now));
                    }
                    host.entry_clicked(idx, button, streak_two);
                }
                super::termwindow::UIItemType::SftpParentDir => {
                    if button == MousePress::Left {
                        host.button_clicked(SftpButton::ParentDir);
                    }
                }
                super::termwindow::UIItemType::SftpRefresh => {
                    if button == MousePress::Left {
                        host.button_clicked(SftpButton::Refresh);
                    }
                }
                super::termwindow::UIItemType::SftpSidebar => match button {
                    MousePress::Left => {
                        // 改名态点空白 = 提交
                        if host.state.rename.is_some() {
                            host.commit_rename();
                        } else if host.state.pending_delete.is_some() {
                            host.state.pending_delete = None;
                            host.state.invalidate();
                            host.invalidate_window();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        ::window::MouseEventKind::Release(press) => {
            if press == MousePress::Left {
                host.state.drag_start = None;
            }
        }
        ::window::MouseEventKind::Move => {
            let left_down = event.mouse_buttons.contains(MouseButtons::LEFT);
            host.dragout_check((x, y), left_down);
        }
        ::window::MouseEventKind::VertWheel(amount) => {
            host.scroll(amount as isize);
        }
        _ => {}
    }
}
