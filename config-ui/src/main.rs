#![windows_subsystem = "windows"]

mod text_input;
mod theme;
mod widgets;

use std::path::PathBuf;
use std::ptr::null_mut;

use config_ui_core::form::FormState;
use config_ui_core::keybinds::{self, Binding};
use config_ui_core::load::{LoadError, config_to_json, load_from_source};
use config_ui_core::schemes::{SchemeInfo, builtin_schemes};
use config_ui_core::settings::{self, Kind, SettingValue, SETTINGS};
use config_ui_core::ssh::{self, SshConnection};
use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Bounds, Context, Entity, Focusable, KeyBinding,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba,
    Window, WindowBounds, WindowOptions,
};
use notify::Watcher;
use text_input::{TextInput, bind_input_keys};
use widgets::{segment, toggle};

#[cfg(windows)]
use winapi::shared::minwindef::TRUE;
#[cfg(windows)]
use winapi::shared::windef::HICON;
#[cfg(windows)]
use winapi::um::wingdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
#[cfg(windows)]
use winapi::um::winuser::{
    CreateIconIndirect, GetDC, ICONINFO, ReleaseDC, SendMessageW, ICON_BIG, ICON_SMALL,
    WM_SETICON,
};

actions!(config_ui, [Quit, MoveFocusNext, MoveFocusPrev]);

const SLIDER_MIN: f32 = 8.0;
const SLIDER_MAX: f32 = 24.0;
const LINE_HEIGHT_MIN: f32 = 0.8;
const LINE_HEIGHT_MAX: f32 = 2.5;
const OPACITY_MIN: f32 = 0.1;
const OPACITY_MAX: f32 = 1.0;

/// 已由手写滑条行渲染的注册表 key：通用行渲染器跳过，避免同屏出现两份控件
const HAND_SLIDERS: &[&str] = &["font_size", "line_height", "window_background_opacity"];

/// 默认键绑定动作的中文翻译映射
fn default_binding_action_cn(action: &str) -> String {
    if action.starts_with("CopyTo") {
        return action.replace("CopyTo", "复制到").replace("Clipboard", "剪贴板").replace("PrimarySelection", "主选区");
    }
    if action.starts_with("PasteFrom") {
        return action.replace("PasteFrom", "从...粘贴").replace("Clipboard", "剪贴板").replace("PrimarySelection", "主选区");
    }
    action
        .replace("SpawnTab", "新建标签页")
        .replace("SpawnWindow", "新建窗口")
        .replace("CloseCurrentTab", "关闭当前标签页")
        .replace("ReloadConfiguration", "重载配置")
        .replace("ShowDebugOverlay", "显示调试覆盖层")
        .replace("ActivateCommandPalette", "激活命令面板")
        .replace("ActivateCopyMode", "激活复制模式")
        .replace("CharSelect", "字符选择")
        .replace("QuickSelect", "快速选择")
        .replace("Search", "搜索")
        .replace("ClearScrollback", "清除回滚缓冲")
        .replace("TogglePaneZoomState", "切换面板缩放状态")
        .replace("ToggleFullScreen", "切换全屏")
        .replace("DecreaseFontSize", "减小字体")
        .replace("IncreaseFontSize", "增大字体")
        .replace("ResetFontSize", "重置字体大小")
        .replace("ActivateTabRelative(-1)", "激活相对标签页(-1)")
        .replace("ActivateTabRelative(1)", "激活相对标签页(1)")
        .replace("MoveTabRelative(-1)", "移动相对标签页(-1)")
        .replace("MoveTabRelative(1)", "移动相对标签页(1)")
        .replace("ScrollByPage(-1)", "按页滚动(-1)")
        .replace("ScrollByPage(1)", "按页滚动(1)")
        .replace("ActivatePaneDirection 'Left'", "激活面板方向 '左'")
        .replace("ActivatePaneDirection 'Right'", "激活面板方向 '右'")
        .replace("ActivatePaneDirection 'Up'", "激活面板方向 '上'")
        .replace("ActivatePaneDirection 'Down'", "激活面板方向 '下'")
}

/// `#rrggbb[aa]` → GPUI 颜色；非法输入回退黑色
fn hex_rgb(s: &str) -> Rgba {
    let hex: String = s.trim_start_matches('#').chars().take(6).collect();
    rgb(u32::from_str_radix(&hex, 16).unwrap_or(0))
}

#[derive(Clone, Copy, PartialEq)]
enum SliderDrag {
    FontSize,
    LineHeight,
    Opacity,
}

fn main() {
    // ponytail: 单实例用本地端口占位检测，聚焦既有窗口需 IPC，后续再补
    let single_instance = std::net::TcpListener::bind(("127.0.0.1", 43117));
    if single_instance.is_err() {
        return;
    }

    // GUI 唯一读写目标：exe 同目录的 orca-config.lua。
    // 永不触碰用户 HOME 下的 .wezterm.lua；首次启动在此处写一份 wezterm 默认 config，
    // 之后所有改动都落在这个文件，由主程序启动时优先加载（见 config::load_with_overrides）。
    let config_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("orca-config.lua")));
    let config_path = match config_path {
        Some(p) => p,
        None => {
            eprintln!("无法确定当前 exe 所在目录");
            return;
        }
    };
    if !config_path.exists() {
        // 写一份与 wezterm 主程序启动时自动生成的 .wezterm.lua 模板等价的"空 config"
        if let Err(e) = std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\n\
             local config = wezterm.config_builder()\n\
             \n\
             return config\n",
        ) {
            eprintln!("无法创建 {}：{e}", config_path.display());
            return;
        }
    }

    let _keep = single_instance.ok();
    let _ = gpui_platform::application().run(|cx: &mut App| {
        bind_input_keys(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            // Tab/Shift+Tab 在输入框间切换焦点(TextInput 自带 track_focus,
            // 自动注册为 tab stop,序列=当前页渲染顺序)
            KeyBinding::new("tab", MoveFocusNext, None),
            KeyBinding::new("shift-tab", MoveFocusPrev, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        // Load icon (gpui WindowOptions.icon is X11-only; we set Win32 HWND icon after open)
        let icon_data = include_bytes!("../../assets/icon/terminal.png");
        let icon_image = image::load_from_memory(icon_data).ok().map(|img| img.to_rgba8());

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("orca-term 配置".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                if let Some(rgba) = icon_image.as_ref() {
                    set_window_icon(window, rgba);
                }
                cx.new(|cx| ConfigUi::new(config_path, cx))
            },
        )
        .unwrap();
    });
}

struct ConfigUi {
    path: PathBuf,
    file_text: String,
    snapshot: serde_json::Value,
    error: Option<String>,
    selected_group: usize,
    dirty: usize,
    form: FormState,
    /// 字体族下拉是否展开
    font_family_open: bool,
    /// 基线快照：始终是 wezterm 出厂默认，供 to_lua 做「仅发射非默认字段」diff
    /// （新文件格式无标记区概念，整文件就是 GUI 自身输出；与默认比对即可）。
    baseline: serde_json::Value,
    dragging_slider: Option<SliderDrag>,
    font_size_bounds: widgets::SharedBounds,
    line_height_bounds: widgets::SharedBounds,
    opacity_bounds: widgets::SharedBounds,
    /// 注册表文本输入项（下标=SETTINGS 下标）：Int/Float/Str/List 类设置共用
    setting_inputs: Vec<Option<Entity<TextInput>>>,
    color_scheme_input: Option<Entity<TextInput>>,
    scheme_search: Option<Entity<TextInput>>,
    scheme_filter: String,
    schemes: Vec<SchemeInfo>,
    // 组6 键绑定
    bindings: Vec<Binding>,
    key_search: Option<Entity<TextInput>>,
    key_filter: String,
    selected_binding: Option<usize>,
    edit_mods_input: Option<Entity<TextInput>>,
    edit_key_input: Option<Entity<TextInput>>,
    /// 第一步选中的动作（EDITABLE_ACTIONS 下标）；None=未选
    edit_action: Option<usize>,
    /// 选中绑定的动作不在快捷清单时的暂存（动作名, 参数）——未点选任何
    /// 清单动作前，「应用修改」原样保留它，避免静默改写动作
    custom_action: Option<(String, String)>,
    capture_mode: bool,
    // 组7 SSH 连接
    ssh_conns: Vec<SshConnection>,
    baseline_ssh_conns: Vec<SshConnection>,
    selected_ssh: Option<usize>,
    ssh_name_input: Option<Entity<TextInput>>,
    ssh_host_input: Option<Entity<TextInput>>,
    ssh_port_input: Option<Entity<TextInput>>,
    ssh_user_input: Option<Entity<TextInput>>,
    ssh_key_input: Option<Entity<TextInput>>,
    ssh_cmd_input: Option<Entity<TextInput>>,
    //「SFTP 命令」草案:点标签栏 SFTP 按钮时对该连接执行的外部工具命令
    ssh_sftp_input: Option<Entity<TextInput>>,
    // 外部修改检测（配置文件仍可能被外部工具改动,保护未保存的表单改动）
    external_changed: bool,
    foreground_input: Option<Entity<TextInput>>,
    background_input: Option<Entity<TextInput>>,
    cursor_color_input: Option<Entity<TextInput>>,
    padding_left_input: Option<Entity<TextInput>>,
    padding_right_input: Option<Entity<TextInput>>,
    padding_top_input: Option<Entity<TextInput>>,
    padding_bottom_input: Option<Entity<TextInput>>,
    default_prog_input: Option<Entity<TextInput>>,
}

impl ConfigUi {
    fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        // 基线 = wezterm 出厂默认；只有与默认不同的字段会被写回文件，避免每次都把全表 dump 出来
        let baseline = load_default_snapshot();
        let form = FormState::from_snapshot(&baseline, &baseline);
        let mut s = Self {
            path,
            file_text: String::new(),
            snapshot: baseline.clone(),
            error: None,
            selected_group: 0,
            dirty: 0,
            form,
            baseline,
            font_family_open: false,
            dragging_slider: None,
            font_size_bounds: Default::default(),
            line_height_bounds: Default::default(),
            opacity_bounds: Default::default(),
            setting_inputs: (0..SETTINGS.len()).map(|_| None).collect(),
            color_scheme_input: None,
            scheme_search: None,
            scheme_filter: String::new(),
            schemes: builtin_schemes(),
            bindings: vec![],
            key_search: None,
            key_filter: String::new(),
            selected_binding: None,
            edit_mods_input: None,
            edit_key_input: None,
            edit_action: None,
            custom_action: None,
            capture_mode: false,
            ssh_conns: vec![],
            baseline_ssh_conns: vec![],
            selected_ssh: None,
            ssh_name_input: None,
            ssh_host_input: None,
            ssh_port_input: None,
            ssh_user_input: None,
            ssh_key_input: None,
            ssh_cmd_input: None,
            ssh_sftp_input: None,
            external_changed: false,
            foreground_input: None,
            background_input: None,
            cursor_color_input: None,
            padding_left_input: None,
            padding_right_input: None,
            padding_top_input: None,
            padding_bottom_input: None,
            default_prog_input: None,
        };
        s.reload_from_disk(cx);
        s.start_file_watcher(cx);
        s
    }

    /// notify 监听配置文件（防抖 2s）：无未保存改动时自动重载，
    /// 有未保存改动则只亮出冲突横幅，由用户决定取舍。
    fn start_file_watcher(&mut self, cx: &mut Context<Self>) {
        let cfg = self.path.clone();
        let (tx, rx) = smol::channel::unbounded::<()>();
        std::thread::spawn(move || {
            let (wtx, wrx) = std::sync::mpsc::channel();
            let target = cfg
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let watch_file = cfg.clone();
            let mut watcher = notify::recommended_watcher(
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(ev) = res {
                        if ev.paths.iter().any(|p| *p == watch_file) {
                            let _ = wtx.send(());
                        }
                    }
                },
            )
            .ok();
            if let Some(w) = watcher.as_mut() {
                let _ = w.watch(&target, notify::RecursiveMode::NonRecursive);
            }
            for _ in wrx {
                let _ = tx.try_send(());
            }
        });
        cx.spawn(async move |this, cx| loop {
            if rx.recv().await.is_err() {
                break;
            }
            // ponytail: 固定 2s 防抖窗口，编辑器多次落盘合并为一次刷新
            while rx.try_recv().is_ok() {}
            smol::Timer::after(std::time::Duration::from_secs(2)).await;
            while rx.try_recv().is_ok() {}
            let ok = this.update(cx, |s, cx| {
                if s.dirty > 0 {
                    s.external_changed = true;
                } else {
                    s.reload_from_disk(cx);
                }
                cx.notify();
            });
            if ok.is_err() {
                break;
            }
        })
        .detach();
    }

    /// 控件←表单 单向刷新：每次 reload/reset 后把 TextInput 内容设为表单当前值。
    fn sync_inputs(&mut self, cx: &mut Context<Self>) {
        if self.color_scheme_input.is_none() {
            self.color_scheme_input = Some(cx.new(|cx| TextInput::new("Catppuccin Mocha", cx)));
            self.scheme_search = Some(cx.new(|cx| TextInput::new("搜索配色方案…", cx)));
            self.key_search = Some(cx.new(|cx| TextInput::new("搜索键位/动作…", cx)));
            self.edit_mods_input = Some(cx.new(|cx| TextInput::new("CTRL|SHIFT", cx)));
            self.edit_key_input = Some(cx.new(|cx| TextInput::new("c", cx)));
            self.foreground_input = Some(cx.new(|cx| TextInput::new("#ffffff", cx)));
            self.background_input = Some(cx.new(|cx| TextInput::new("#000000", cx)));
            self.cursor_color_input = Some(cx.new(|cx| TextInput::new("#ffffff", cx)));
            self.padding_left_input = Some(cx.new(|cx| TextInput::new("8", cx)));
            self.padding_right_input = Some(cx.new(|cx| TextInput::new("8", cx)));
            self.padding_top_input = Some(cx.new(|cx| TextInput::new("4", cx)));
            self.padding_bottom_input = Some(cx.new(|cx| TextInput::new("4", cx)));
            // 首次创建占位符仅为输入提示；form.default_prog 已由 reload_from_disk
            // 从磁盘配置或出厂默认加载，此处若硬编码覆写会把 pwsh 7 等用户设置
            // 冲成 PS 5.1，导致「只改字体保存一次」就丢默认 shell。实际内容
            // 由本函数末尾的统一回填（set_input）写入。
            self.default_prog_input = Some(cx.new(|cx| TextInput::new("如 pwsh.exe 或完整路径", cx)));
            self.ssh_name_input = Some(cx.new(|cx| TextInput::new("", cx)));
            self.ssh_host_input = Some(cx.new(|cx| TextInput::new("", cx)));
            self.ssh_port_input = Some(cx.new(|cx| TextInput::new("22", cx)));
            self.ssh_user_input = Some(cx.new(|cx| TextInput::new("", cx)));
            self.ssh_key_input = Some(cx.new(|cx| TextInput::new("", cx)));
            self.ssh_cmd_input = Some(cx.new(|cx| TextInput::new("", cx)));
            self.ssh_sftp_input = Some(cx.new(|cx| TextInput::new("", cx)));

            // 配色组：宽松校验——空串=清除覆盖，其余原样接受（含 scheme 名称与 #十六进制）
            if let Some(e) = self.color_scheme_input.clone() {
                let derived = [
                    self.foreground_input.clone(),
                    self.background_input.clone(),
                    self.cursor_color_input.clone(),
                ];
                observe_input(&e, cx, move |this, s, cx| {
                    if this.form.color_scheme != s {
                        this.form.color_scheme = s.to_string();
                        // 换方案后，旧方案解析出的调色板不再成立：清空派生色及其输入框。
                        // 否则它们会作为 colors 覆盖随保存发射，把旧方案的配色钉在新方案上。
                        this.form.foreground.clear();
                        this.form.background.clear();
                        this.form.cursor_bg.clear();
                        for slot in &derived {
                            if let Some(e) = slot {
                                e.update(cx, |i, _| i.content = "".into());
                            }
                        }
                        this.dirty += 1;
                    }
                });
            }
            // 方案网格搜索框：只改过滤词，不触碰表单与 dirty
            if let Some(e) = self.scheme_search.clone() {
                observe_input(&e, cx, |this, s, _| this.scheme_filter = s.to_lowercase());
            }
            if let Some(e) = self.key_search.clone() {
                observe_input(&e, cx, |this, s, _| this.key_filter = s.to_lowercase());
            }
            if let Some(e) = self.foreground_input.clone() {
                observe_input(&e, cx, |this, s, _| {
                    if this.form.foreground != s {
                        this.form.foreground = s.to_string();
                        this.dirty += 1;
                    }
                });
            }
            if let Some(e) = self.background_input.clone() {
                observe_input(&e, cx, |this, s, _| {
                    if this.form.background != s {
                        this.form.background = s.to_string();
                        this.dirty += 1;
                    }
                });
            }
            if let Some(e) = self.cursor_color_input.clone() {
                observe_input(&e, cx, |this, s, _| {
                    if this.form.cursor_bg != s {
                        this.form.cursor_bg = s.to_string();
                        this.dirty += 1;
                    }
                });
            }
            for (entity, slot) in [
                (&self.padding_left_input, 0usize),
                (&self.padding_right_input, 1),
                (&self.padding_top_input, 2),
                (&self.padding_bottom_input, 3),
            ] {
                if let Some(e) = entity.clone() {
                    observe_input(&e, cx, move |this, s, _| {
                        if let Ok(v) = s.parse::<i64>() {
                            let target = match slot {
                                0 => &mut this.form.padding_left,
                                1 => &mut this.form.padding_right,
                                2 => &mut this.form.padding_top,
                                _ => &mut this.form.padding_bottom,
                            };
                            if *target != v {
                                *target = v;
                                this.dirty += 1;
                            }
                        }
                    });
                }
            }
            if let Some(e) = self.default_prog_input.clone() {
                observe_input(&e, cx, |this, s, _| {
                    let v: Vec<String> =
                        s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                    let v = (!v.is_empty()).then_some(v);
                    if this.form.default_prog != v {
                        this.form.default_prog = v;
                        this.dirty += 1;
                    }
                });
            }
            // SSH 输入框 = 草案暂存区：不实时写回 SshConnection，仅触发重绘
            // （私钥路径「文件存在」指示器），点「添加连接」/「应用修改」时才收集。
            for entity in [
                &self.ssh_name_input,
                &self.ssh_host_input,
                &self.ssh_port_input,
                &self.ssh_user_input,
                &self.ssh_key_input,
                &self.ssh_cmd_input,
                &self.ssh_sftp_input,
            ] {
                if let Some(e) = entity.clone() {
                    observe_input(&e, cx, |_, _, cx| cx.notify());
                }
            }
        }
        // 注册表文本输入项（Int/Float/Str/List）：懒创建 + 挂观察者 + 内容回填
        for idx in 0..SETTINGS.len() {
            if !setting_kind_needs_input(SETTINGS[idx].kind) {
                continue;
            }
            if self.setting_inputs[idx].is_none() {
                let e = cx.new(|cx| TextInput::new("", cx));
                observe_input(&e, cx, move |this, s, _| this.apply_setting_input(idx, s));
                self.setting_inputs[idx] = Some(e);
            }
            let text = self.setting_input_text(idx);
            self.set_input(&self.setting_inputs[idx], &text, cx);
        }
        self.set_input(&self.color_scheme_input, &self.form.color_scheme, cx);
        self.set_input(&self.foreground_input, &self.form.foreground, cx);
        self.set_input(&self.background_input, &self.form.background, cx);
        self.set_input(&self.cursor_color_input, &self.form.cursor_bg, cx);
        self.set_input(&self.padding_left_input, &self.form.padding_left.to_string(), cx);
        self.set_input(&self.padding_right_input, &self.form.padding_right.to_string(), cx);
        self.set_input(&self.padding_top_input, &self.form.padding_top.to_string(), cx);
        self.set_input(&self.padding_bottom_input, &self.form.padding_bottom.to_string(), cx);
        self.set_input(
            &self.default_prog_input,
            &self.form.default_prog.as_ref().map(|v| v.join(", ")).unwrap_or_default(),
            cx,
        );
        // SSH：重载后默认选中第一条并回填（无连接则清空编辑器）
        if self.selected_ssh.is_none() && !self.ssh_conns.is_empty() {
            self.selected_ssh = Some(0);
        }
        match self.selected_ssh.and_then(|i| self.ssh_conns.get(i).cloned()) {
            Some(c) => {
                self.set_input(&self.ssh_name_input, &c.name, cx);
                self.set_input(&self.ssh_host_input, &c.host, cx);
                self.set_input(&self.ssh_port_input, &c.port.to_string(), cx);
                self.set_input(&self.ssh_user_input, &c.username, cx);
                self.set_input(&self.ssh_key_input, &c.key_path, cx);
                self.set_input(&self.ssh_cmd_input, &c.initial_command, cx);
                self.set_input(&self.ssh_sftp_input, &c.sftp_command, cx);
            }
            None => {
                for e in [
                    &self.ssh_name_input,
                    &self.ssh_host_input,
                    &self.ssh_port_input,
                    &self.ssh_user_input,
                    &self.ssh_key_input,
                    &self.ssh_cmd_input,
                    &self.ssh_sftp_input,
                ] {
                    self.set_input(e, "", cx);
                }
            }
        }
    }

    fn set_input(&self, e: &Option<Entity<TextInput>>, val: &str, cx: &mut Context<Self>) {
        if let Some(e) = e {
            e.update(cx, |i, _| i.content = val.into());
        }
    }

    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let path = self.path.clone();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.file_text = text.clone();
            }
            Err(_) => {
                self.file_text.clear();
            }
        }
        match load_from_source(&self.file_text, &path) {
            Ok(loaded) => {
                self.snapshot = config_to_json(&loaded.config);
                if self.snapshot.is_null() {
                    self.snapshot = serde_json::json!({});
                }
            }
            Err(e) => {
                self.error = Some(fmt_load_error(&e));
                self.snapshot = serde_json::json!({});
            }
        }
        self.form = FormState::from_snapshot(&self.snapshot, &self.baseline);
        self.bindings = keybinds::parse_bindings(&self.snapshot);
        self.selected_binding = None;
        self.ssh_conns = ssh::parse_ssh_domains(&self.snapshot);
        self.baseline_ssh_conns = self.ssh_conns.clone();
        self.selected_ssh = None;
        // 伪设置项「标签页按颜色区分」读回：状态只体现在发射的 Lua 文本里
        //（JSON 快照无此字段）。配置里没有任何标记（全新/历史遗留）时默认**开**
        //——该功能的预期是开箱即见，用户可在标签栏页显式关闭（off 标记）。
        let tab_colors = ssh::detect_tab_colors(&self.file_text).unwrap_or(true);
        if let Some(idx) = settings::index(settings::TAB_COLOR_DISTINCT_KEY) {
            self.form.scalars.set_bool(idx, tab_colors);
        }
        self.sync_inputs(cx);
    }

    fn save(&mut self, _: &MouseDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let path = self.path.clone();
        // to_lua / emit_bindings 内部会先发 `local wezterm = require 'wezterm'\n`；
        // 我们的统一头已经声明过同名 local，剥掉内层声明避免第二次 `local config` shadow 掉 builder。
        let strip_wezterm_require = |s: &str| -> String {
            s.replace("local wezterm = require 'wezterm'\n", "")
        };
        let mut body = strip_wezterm_require(&self.form.to_lua(&self.baseline));
        // 颜色字段校验：非空时必须是 #RGB/#RRGGBB/#RRGGBBAA 三种十六进制形式。
        // 非法颜色会随 colors 块发射，主程序整个配置加载失败——「保存成功但
        // 终端报错」比保存失败更坑，必须当场拦下。
        for (label, v) in [
            ("前景色", self.form.foreground.trim()),
            ("背景色", self.form.background.trim()),
            ("光标色", self.form.cursor_bg.trim()),
        ] {
            if v.is_empty() || is_valid_color(v) {
                continue;
            }
            self.error = Some(format!(
                "{label}「{v}」格式无效：应为 #RRGGBB（或 #RGB / #RRGGBBAA），留空表示跟随配色方案"
            ));
            cx.notify();
            return;
        }
        // 键绑定：与出厂默认不同才发射。文件是全量重建的，「与上次保存相同」不是跳过理由，
        // 否则后续任何不改键位的保存都会把已有的 config.keys 块抹掉（曾因此在 ssh_domains 上丢过连接）。
        if !bindings_equal(&self.bindings, &keybinds::parse_bindings(&self.baseline)) {
            body.push_str(&strip_wezterm_require(&keybinds::emit_bindings(
                &self.bindings,
            )));
        }
        // SSH 连接：非空即发射（同上，全量重建下按「是否有变化」跳过会丢块）；
        // 清空列表 = 从配置中移除，靠不发射自然完成。校验失败中止保存。
        if !self.ssh_conns.is_empty() {
            if let Err(msg) = ssh::validate(&self.ssh_conns) {
                self.error = Some(msg);
                cx.notify();
                return;
            }
            body.push_str(&ssh::emit_ssh_domains(&self.ssh_conns));
        }
        // 启动菜单（"+"左右键弹出）：由 SSH 连接列表派生，保存时始终重写保持同步；
        // 传当前表单的 default_prog 作为 pwsh 探测的最终证据（绿色版 pwsh 不在
        // PATH/ProgramFiles 里，见 config-ui-core ssh::resolve_pwsh）
        body.push_str(&ssh::emit_launch_menu(
            &self.ssh_conns,
            self.form.default_prog.as_deref(),
        ));
        // format-tab-title：「标签页按颜色区分」开=彩色版（激活 tab 按主题
        // ANSI 亮色六色循环着色），关=普通版。始终发射：整文件重写后回调必须
        // 仍在，且关态会自然替换掉旧的彩色标记。
        let tab_colors = settings::index(settings::TAB_COLOR_DISTINCT_KEY)
            .map(|idx| self.form.scalars.bool_at(idx))
            .unwrap_or(false);
        body.push_str(&ssh::emit_format_tab_title(tab_colors));
        let text = format!(
            "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\n\n{body}return config\n"
        );
        match std::fs::write(&path, text.as_bytes()) {
            Ok(_) => {
                self.file_text = text;
                self.dirty = 0;
                self.reload_from_disk(cx);
                cx.quit();
            }
            Err(e) => self.error = Some(format!("写入失败：{e}")),
        }
        cx.notify();
    }

    fn reset(&mut self, _: &MouseDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        // 放弃改动 = 丢弃未保存修改并关闭窗口(不写文件,与「保存并关闭」对称)。
        // 旧实现只从磁盘重载表单,在加载失败/无改动场景肉眼无变化,像点了没反应。
        cx.quit();
    }

    fn set_slider_value(&mut self, drag: SliderDrag, x: Pixels, cx: &mut Context<Self>) {
        let (slot, min, max, idx) = match drag {
            SliderDrag::FontSize => (
                &self.font_size_bounds,
                SLIDER_MIN,
                SLIDER_MAX,
                settings::index("font_size").unwrap(),
            ),
            SliderDrag::LineHeight => (
                &self.line_height_bounds,
                LINE_HEIGHT_MIN,
                LINE_HEIGHT_MAX,
                settings::index("line_height").unwrap(),
            ),
            SliderDrag::Opacity => (
                &self.opacity_bounds,
                OPACITY_MIN,
                OPACITY_MAX,
                settings::index("window_background_opacity").unwrap(),
            ),
        };
        let v = widgets::slider_value_at(slot, x, min, max) as f64;
        // ponytail: f32 滑条坐标转 f64 会带噪声（如 13.223226...），统一保留两位小数
        let v = (v * 100.0).round() / 100.0;
        if (self.form.scalars.float_at(idx) - v).abs() > 1e-6 {
            self.form.scalars.set_float(idx, v);
            self.dirty += 1;
            cx.notify();
        }
    }

    fn slider_down(
        &mut self,
        drag: SliderDrag,
        e: &MouseDownEvent,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dragging_slider = Some(drag);
        self.set_slider_value(drag, e.position.x, cx);
    }

    fn slider_move(
        &mut self,
        drag: SliderDrag,
        e: &MouseMoveEvent,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(d) = self.dragging_slider {
            if d == drag {
                self.set_slider_value(d, e.position.x, cx);
            }
        }
    }

    fn slider_up(&mut self, _: &MouseUpEvent, _w: &mut Window, cx: &mut Context<Self>) {
        self.dragging_slider = None;
        cx.notify();
    }

    fn nav_to(&mut self, i: usize, _: &MouseDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        self.selected_group = i;
        cx.notify();
    }

    fn render_action_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .h(px(40.))
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .bg(theme::bg_panel())
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::fg_main())
                            .child("orca-term 配置编辑器"),
                    )
                    .when(self.dirty > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::accent())
                                .child("● 未保存"),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("btn-reset")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme::bg_elevated())
                            .border_1()
                            .border_color(theme::border())
                            .text_color(theme::fg_main())
                            .text_size(px(12.))
                            .cursor_pointer()
                            .child("放弃改动")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::reset)),
                    )
                    .child(
                        div()
                            .id("btn-save")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme::accent())
                            .text_color(theme::fg_main())
                            .text_size(px(12.))
                            .cursor_pointer()
                            .child("保存并关闭")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::save)),
                    ),
            )
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .w(px(200.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme::bg_panel())
            .border_r_1()
            .border_color(theme::border())
            .py_2()
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_size(px(11.))
                    .text_color(theme::fg_dim())
                    .child("配置分组"),
            )
            .children(GROUPS.iter().enumerate().map(|(i, name)| {
                let is_selected = i == self.selected_group;
                div()
                    .id(gpui::SharedString::from(format!("nav-{i}")))
                    .px_3()
                    .py_1p5()
                    .text_size(px(13.))
                    .cursor_pointer()
                    .text_color(if is_selected { theme::fg_main() } else { theme::fg_dim() })
                    .bg(if is_selected { theme::accent_dim() } else { theme::bg_panel() })
                    .when(!is_selected, |d| d.hover(|s| s.bg(theme::bg_elevated())))
                    .border_l_2()
                    .border_color(if is_selected { theme::accent() } else { theme::bg_panel() })
                    .child(name.to_string())
                    .on_mouse_down(MouseButton::Left, {
                        let entity = cx.entity();
                        move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                            entity.update(app, |state: &mut ConfigUi, cx| {
                                state.selected_group = i;
                                cx.notify();
                            });
                        }
                    })
            }))
            .child(
                div().mt_auto().px_3().py_2()
                    .text_size(px(10.)).text_color(theme::fg_dim())
                    .child("orca-term · 基于 WezTerm (MIT)"),
            )
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex_1().flex().flex_col().min_w_0()
            .when(self.external_changed, |d| {
                d.child(
                    div().flex().items_center().gap_3().px_4().py_1()
                        .bg(theme::danger())
                        .child(
                            div().text_size(px(12.)).text_color(theme::fg_main())
                                .child("配置文件已被外部修改，当前存在未保存的表单改动"),
                        )
                        .child(
                            btn("放弃表单改动并重载", cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.dirty = 0;
                                this.external_changed = false;
                                this.reload_from_disk(cx);
                                cx.notify();
                            })),
                        ),
                )
            })
            .child(
                div().flex_1().p_4().child(self.render_tab(cx)),
            )
    }

    fn render_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .when(self.selected_group == 0, |d| d.child(self.render_font_tab(cx)))
            .when(self.selected_group == 1, |d| d.child(self.render_color_tab(cx)))
            .when(self.selected_group == 2, |d| d.child(self.render_window_tab(cx)))
            .when(self.selected_group == 3, |d| d.child(self.render_setting_rows(3, cx)))
            .when(self.selected_group == 4, |d| d.child(self.render_startup_tab(cx)))
            .when(self.selected_group == 5, |d| d.child(self.render_setting_rows(5, cx)))
            .when(self.selected_group == 6, |d| d.child(self.render_setting_rows(6, cx)))
            .when(self.selected_group == 7, |d| d.child(self.render_keys_tab(cx)))
            .when(self.selected_group == 8, |d| d.child(self.render_ssh_tab(cx)))
            .when(self.selected_group > 8, |d| d.child(div().child("未知分组")))
    }

    /// 注册表通用行渲染：按 kind 自动选控件（开关/分段选择/文本输入）。
    fn setting_row(&mut self, idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let spec = &SETTINGS[idx];
        let label = div()
            .w(px(180.))
            .text_size(px(13.))
            .text_color(theme::fg_main())
            .child(spec.label.to_string());
        let control: gpui::Div = match spec.kind {
            Kind::Bool => {
                let value = self.form.scalars.bool_at(idx);
                let entity = cx.entity();
                div()
                    .flex()
                    .items_center()
                    .child(toggle(spec.key, value))
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                        entity.update(app, |state: &mut ConfigUi, cx| {
                            let next = !state.form.scalars.bool_at(idx);
                            state.form.scalars.set_bool(idx, next);
                            state.dirty += 1;
                            cx.notify();
                        });
                    })
            }
            Kind::Enum(opts) => {
                let current = self.form.scalars.str_at(idx).to_string();
                let entity = cx.entity();
                let key = spec.key;
                div().flex().flex_wrap().gap_1().children(opts.iter().enumerate().map(|(oi, (val, lab))| {
                    let entity = entity.clone();
                    let val = val.to_string();
                    let is_sel = current == val;
                    segment(
                        gpui::SharedString::from(format!("{key}-{oi}")),
                        lab,
                        is_sel,
                    )
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                        entity.update(app, |state: &mut ConfigUi, cx| {
                            if state.form.scalars.str_at(idx) != val {
                                state.form.scalars.set_str(idx, val.clone());
                                state.dirty += 1;
                                cx.notify();
                            }
                        });
                    })
                }))
            }
            _ => div().w(px(220.)).child(self.input(&self.setting_inputs[idx])),
        };
        div().flex().items_center().gap_2().child(label).child(control)
    }

    /// 某页注册表项的通用行集合；手写滑条行的 key 跳过（font_size/line_height/opacity）。
    fn render_setting_rows(&mut self, page: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().gap_3();
        for idx in 0..SETTINGS.len() {
            if SETTINGS[idx].page != page || HAND_SLIDERS.contains(&SETTINGS[idx].key) {
                continue;
            }
            let row = self.setting_row(idx, cx);
            col = col.child(row);
        }
        col
    }

    /// 注册表文本输入框的显示文本。
    fn setting_input_text(&self, idx: usize) -> String {
        match SETTINGS[idx].kind {
            Kind::Int { .. } => self.form.scalars.int_at(idx).to_string(),
            Kind::Float { .. } => format!("{}", self.form.scalars.float_at(idx)),
            Kind::Str | Kind::Enum(_) => self.form.scalars.str_at(idx).to_string(),
            Kind::List => self.form.scalars.list_at(idx).join(", "),
            Kind::Bool => String::new(),
        }
    }

    /// 注册表输入内容 → 表单值：按 kind 解析并做区间校验，非法输入忽略（保留旧值）。
    fn apply_setting_input(&mut self, idx: usize, s: &str) {
        let changed = match SETTINGS[idx].kind {
            Kind::Int { min, max } => match s.trim().parse::<i64>() {
                Ok(v) if v >= min && v <= max => {
                    let diff = self.form.scalars.int_at(idx) != v;
                    if diff {
                        self.form.scalars.set_int(idx, v);
                    }
                    diff
                }
                _ => false,
            },
            Kind::Float { min, max } => match s.trim().parse::<f64>() {
                Ok(v) if v >= min && v <= max => {
                    let diff = (self.form.scalars.float_at(idx) - v).abs() > 1e-9;
                    if diff {
                        self.form.scalars.set_float(idx, v);
                    }
                    diff
                }
                _ => false,
            },
            Kind::Str => {
                let diff = self.form.scalars.str_at(idx) != s;
                if diff {
                    self.form.scalars.set_str(idx, s.to_string());
                }
                diff
            }
            Kind::List => {
                let items: Vec<String> = s
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                let diff = self.form.scalars.list_at(idx) != items;
                if diff {
                    self.form.scalars.set(idx, SettingValue::List(items));
                }
                diff
            }
            Kind::Bool | Kind::Enum(_) => false,
        };
        if changed {
            self.dirty += 1;
        }
    }

    /// 渲染 TextInput 视图实体本身（含 track_focus/on_mouse_down/key_context 的交互 div）。
    /// 不能只渲染 TextElement——那只是纯绘制元素，交互层不进元素树会导致焦点永远无法建立。
    fn input(&self, entity: &Option<Entity<TextInput>>) -> impl IntoElement {
        entity.clone().unwrap()
    }

    fn render_font_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let font_idx = settings::index("font_size").unwrap();
        let line_idx = settings::index("line_height").unwrap();
        div().flex().flex_col().gap_3()
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("字体大小"))
                    .child(
                        div().flex_1()
                            .child(widgets::slider("font-size", self.form.scalars.float_at(font_idx) as f32, SLIDER_MIN, SLIDER_MAX, self.font_size_bounds.clone()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, w, cx| this.slider_down(SliderDrag::FontSize, e, w, cx)))
                            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| this.slider_move(SliderDrag::FontSize, e, w, cx)))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::slider_up)),
                    ),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("字体族"))
                    .child(widgets::font_dropdown(
                        self.form.font_family.as_deref().unwrap_or("JetBrains Mono"),
                        self.font_family_open,
                        cx.listener(|this, _: &MouseDownEvent, _, _| {
                            this.font_family_open = !this.font_family_open;
                        }),
                        cx.listener(|this, family: &String, _, _| {
                            this.form.font_family = Some(family.clone());
                            this.font_family_open = false;
                            this.dirty += 1;
                        }),
                        cx.listener(|this, _: &MouseDownEvent, _, _| {
                            this.font_family_open = false;
                        }),
                    )),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("行高"))
                    .child(
                        div().flex_1()
                            .child(widgets::slider("line-height", self.form.scalars.float_at(line_idx) as f32, LINE_HEIGHT_MIN, LINE_HEIGHT_MAX, self.line_height_bounds.clone()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, w, cx| this.slider_down(SliderDrag::LineHeight, e, w, cx)))
                            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| this.slider_move(SliderDrag::LineHeight, e, w, cx)))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::slider_up)),
                    ),
            )
            .child(self.render_setting_rows(0, cx))
    }

    /// 网格点选方案：与 scheme 输入框观察者同语义——换方案即清空派生色，
    /// 避免旧方案调色板作为 colors 覆盖被钉在新方案上。
    fn select_scheme(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.form.color_scheme == name {
            return;
        }
        self.form.color_scheme = name.to_string();
        self.form.foreground.clear();
        self.form.background.clear();
        self.form.cursor_bg.clear();
        for slot in [&self.foreground_input, &self.background_input, &self.cursor_color_input] {
            if let Some(e) = slot {
                e.update(cx, |i, _| i.content = "".into());
            }
        }
        self.set_input(&self.color_scheme_input, name, cx);
        self.dirty += 1;
        cx.notify();
    }

    fn render_color_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.scheme_filter.clone();
        // ponytail: 每帧克隆过滤后的方案列表（数百项小字符串），交互频率低可接受；卡顿再改索引引用
        let list: Vec<SchemeInfo> = self
            .schemes
            .iter()
            .filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q))
            .cloned()
            .collect();
        let selected = self.form.color_scheme.clone();
        let grid = div()
            .flex().flex_wrap().gap_2().content_start()
            .h(px(420.)).id("scheme-grid").overflow_y_scroll()
            .p_2().rounded_md()
            .bg(theme::bg_panel())
            .border_1().border_color(theme::border())
            .children(list.into_iter().map(|sc| {
                let entity = cx.entity();
                let name = sc.name.clone();
                let is_selected = selected == sc.name;
                div().w(px(176.)).p_2().rounded_md().cursor_pointer()
                    .bg(hex_rgb(&sc.background))
                    .when(is_selected, |d| d.border_2().border_color(theme::accent()))
                    .when(!is_selected, |d| d.border_1().border_color(theme::border()))
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                        let name = name.clone();
                        entity.update(app, |state: &mut ConfigUi, cx| state.select_scheme(&name, cx));
                    })
                    .child(
                        div().text_size(px(11.)).text_color(hex_rgb(&sc.foreground))
                            .text_ellipsis().child(sc.name.clone()),
                    )
                    .child(
                        div().flex().flex_wrap().gap(px(2.)).mt_1().children(
                            sc.ansi.iter().chain(sc.brights.iter()).map(|c| {
                                div().w(px(7.)).h(px(7.)).rounded_sm().bg(hex_rgb(c))
                            }),
                        ),
                    )
            }));
        div().flex().flex_col().gap_3()
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("配色方案"))
                    .child(self.input(&self.color_scheme_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("搜索"))
                    .child(self.input(&self.scheme_search)),
            )
            .child(grid)
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("前景色"))
                    .child(self.input(&self.foreground_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("背景色"))
                    .child(self.input(&self.background_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("光标色"))
                    .child(self.input(&self.cursor_color_input)),
            )
            .child(self.render_setting_rows(1, cx))
    }

    fn render_window_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let opacity_idx = settings::index("window_background_opacity").unwrap();
        div().flex().flex_col().gap_3()
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("不透明度"))
                    .child(
                        div().flex_1()
                            .child(widgets::slider("opacity", self.form.scalars.float_at(opacity_idx) as f32, OPACITY_MIN, OPACITY_MAX, self.opacity_bounds.clone()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, w, cx| this.slider_down(SliderDrag::Opacity, e, w, cx)))
                            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| this.slider_move(SliderDrag::Opacity, e, w, cx)))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::slider_up)),
                    ),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("内边距 左"))
                    .child(self.input(&self.padding_left_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("内边距 右"))
                    .child(self.input(&self.padding_right_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("内边距 上"))
                    .child(self.input(&self.padding_top_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("内边距 下"))
                    .child(self.input(&self.padding_bottom_input)),
            )
            .child(self.render_setting_rows(2, cx))
    }

    fn render_startup_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_3()
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(180.)).text_size(px(13.)).text_color(theme::fg_main()).child("默认程序"))
                    .child(self.input(&self.default_prog_input))
                    .child(btn("浏览…", cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.pick_default_prog_file(window, cx)
                    })))
                    .child(div().text_size(px(11.)).text_color(theme::fg_dim())
                        .child("可执行文件路径；留空跟随默认（自动探测 pwsh 7 / powershell）")),
            )
            .child(self.render_setting_rows(4, cx))
    }
}

/// 键位编辑可用的动作清单:(变体名, 预绑定参数, 界面标签)。
/// 参数在清单里预绑定并拆成独立动作（复制到剪贴板/主选区、上一个/下一个标签页…），
/// 界面不设参数输入框——用户流程固定为「选动作 → 录按键 → 添加」。
/// 不在清单里的复杂动作（CloseCurrentTab/SplitPane 等）暂不支持；需要时扩充此表
///（产品决策：配置文件由界面全量管理，不提供手写 Lua 入口）。
const EDITABLE_ACTIONS: &[(&str, &str, &str)] = &[
    ("CopyTo", "Clipboard", "复制到剪贴板"),
    ("CopyTo", "PrimarySelection", "复制到主选区"),
    ("PasteFrom", "Clipboard", "从剪贴板粘贴"),
    ("PasteFrom", "PrimarySelection", "从主选区粘贴"),
    ("SpawnTab", "", "新建标签页"),
    ("SpawnWindow", "", "新建窗口"),
    ("SplitHorizontal", "", "水平分割"),
    ("SplitVertical", "", "垂直分割"),
    ("ReloadConfiguration", "", "重载配置"),
    ("ToggleFullScreen", "", "切换全屏"),
    ("TogglePaneZoomState", "", "切换面板缩放"),
    ("ActivateCopyMode", "", "激活复制模式"),
    ("QuickSelect", "", "快速选择"),
    ("ShowLauncher", "", "显示启动器"),
    ("ResetFontSize", "", "重置字体大小"),
    ("IncreaseFontSize", "", "增大字体"),
    ("DecreaseFontSize", "", "减小字体"),
    ("ScrollToTop", "", "滚动到顶部"),
    ("ScrollToBottom", "", "滚动到底部"),
    ("ActivateTabRelative", "-1", "上一个标签页"),
    ("ActivateTabRelative", "1", "下一个标签页"),
    ("MoveTabRelative", "-1", "左移标签页"),
    ("MoveTabRelative", "1", "右移标签页"),
];

fn bindings_equal(a: &[Binding], b: &[Binding]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.combo() == y.combo()
                && x.action_name == y.action_name
                && x.action_arg == y.action_arg
        })
}

fn key_display(k: &str) -> String {
    match k {
        "left" => "LeftArrow",
        "right" => "RightArrow",
        "up" => "UpArrow",
        "down" => "DownArrow",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "enter" | "return" => "Enter",
        "escape" => "Escape",
        "space" => "Space",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        other => other,
    }
    .to_string()
}

fn keystroke_to_wezterm(e: &KeyDownEvent) -> (String, String) {
    let m = &e.keystroke.modifiers;
    let mut parts = vec![];
    if m.control {
        parts.push("CTRL");
    }
    if m.platform {
        parts.push("SUPER");
    }
    if m.alt {
        parts.push("ALT");
    }
    if m.shift {
        parts.push("SHIFT");
    }
    (parts.join("|"), key_display(&e.keystroke.key))
}

fn action_display(b: &Binding) -> String {
    match EDITABLE_ACTIONS
        .iter()
        .find(|(n, a, _)| *n == b.action_name && *a == b.action_arg)
    {
        Some((_, _, label)) => label.to_string(),
        None => {
            let name_cn = default_binding_action_cn(&b.action_name);
            if b.action_arg.is_empty() {
                name_cn
            } else {
                format!("{} {}", name_cn, b.action_arg)
            }
        }
    }
}

impl ConfigUi {
    fn select_binding(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected_binding = Some(i);
        let b = self.bindings[i].clone();
        self.set_input(&self.edit_mods_input, &b.mods, cx);
        self.set_input(&self.edit_key_input, &b.key, cx);
        // 动作匹配（名称+参数）:在清单里则高亮对应项;不在（旧配置遗留的
        // 清单外动作）暂存原样,未点选清单动作前「应用修改」不会改写它
        match EDITABLE_ACTIONS
            .iter()
            .position(|(n, a, _)| *n == b.action_name && *a == b.action_arg)
        {
            Some(idx) => {
                self.edit_action = Some(idx);
                self.custom_action = None;
            }
            None => {
                self.edit_action = None;
                self.custom_action = Some((b.action_name.clone(), b.action_arg.clone()));
            }
        }
        cx.notify();
    }

    /// 从编辑器控件读取内容，写回选中绑定（apply=true）或追加为新绑定。
    /// 流程固定为「选动作 → 录按键 → 添加/应用」；动作参数在清单里预绑定，
    /// 无参数框。所有字段先规范化校验，失败在顶部错误横幅给出一句话原因——
    /// 绝不静默无效。
    fn commit_editor(&mut self, apply: bool, cx: &mut Context<Self>) {
        let read = |e: &Option<Entity<TextInput>>| {
            e.as_ref().map(|e| e.read(cx).content.to_string()).unwrap_or_default()
        };
        let fail = |this: &mut Self, msg: String, cx: &mut Context<Self>| {
            this.error = Some(format!("键位编辑：{msg}"));
            cx.notify();
        };
        let (name, arg) = if let Some((n, a)) = self.custom_action.clone() {
            (n, a)
        } else if let Some(idx) = self.edit_action {
            let (n, a, _) = EDITABLE_ACTIONS[idx];
            (n.to_string(), a.to_string())
        } else {
            return fail(self, "请先在第一步选择动作".into(), cx);
        };
        let mods = match normalize_mods(&read(&self.edit_mods_input)) {
            Ok(m) => m,
            Err(token) => {
                return fail(self, format!("无法识别的修饰键「{token}」（可用 CTRL/SHIFT/ALT/SUPER，分隔符用 + 或 |，或点「录入按键」）"), cx);
            }
        };
        let key = match normalize_key(&read(&self.edit_key_input)) {
            Ok(k) => k,
            Err(msg) => return fail(self, msg, cx),
        };
        let b = Binding { mods, key, action_name: name, action_arg: arg };
        if apply {
            if let Some(i) = self.selected_binding {
                if i < self.bindings.len() {
                    self.bindings[i] = b;
                    self.dirty += 1;
                }
            }
        } else {
            self.bindings.push(b);
            self.selected_binding = Some(self.bindings.len() - 1);
            self.dirty += 1;
        }
        self.error = None;
        cx.notify();
    }

    fn delete_binding(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.selected_binding.take() {
            if i < self.bindings.len() {
                self.bindings.remove(i);
                self.dirty += 1;
                cx.notify();
            }
        }
    }

    /// 从编辑器输入框收集一条连接草案：名称留空自动命名，端口解析失败回退 22。
    /// 输入框只是暂存区，是否入列由调用方决定（添加=新条目，应用=写回选中）。
    fn ssh_conn_from_editor(&self, cx: &App) -> SshConnection {
        let text = |slot: &Option<Entity<TextInput>>| {
            slot.as_ref()
                .map(|e| e.read(cx).content.trim().to_string())
                .unwrap_or_default()
        };
        let name = text(&self.ssh_name_input);
        let name = if name.is_empty() {
            format!("连接{}", self.ssh_conns.len() + 1)
        } else {
            name
        };
        SshConnection {
            name,
            host: text(&self.ssh_host_input),
            port: text(&self.ssh_port_input).parse::<u16>().unwrap_or(22),
            username: text(&self.ssh_user_input),
            key_path: text(&self.ssh_key_input),
            initial_command: text(&self.ssh_cmd_input),
            sftp_command: text(&self.ssh_sftp_input),
        }
    }

    fn select_ssh(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.ssh_conns.len() {
            return;
        }
        self.selected_ssh = Some(i);
        let c = self.ssh_conns[i].clone();
        self.set_input(&self.ssh_name_input, &c.name, cx);
        self.set_input(&self.ssh_host_input, &c.host, cx);
        self.set_input(&self.ssh_port_input, &c.port.to_string(), cx);
        self.set_input(&self.ssh_user_input, &c.username, cx);
        self.set_input(&self.ssh_key_input, &c.key_path, cx);
        self.set_input(&self.ssh_cmd_input, &c.initial_command, cx);
        self.set_input(&self.ssh_sftp_input, &c.sftp_command, cx);
        cx.notify();
    }

    fn add_ssh(&mut self, cx: &mut Context<Self>) {
        // 先填后加：编辑器内容作为草案，点「添加连接」才入列为新连接
        let conn = self.ssh_conn_from_editor(cx);
        self.ssh_conns.push(conn);
        let i = self.ssh_conns.len() - 1;
        self.dirty += 1;
        self.select_ssh(i, cx);
    }

    /// 编辑器草案写回选中的连接（「应用修改」按钮）。
    fn apply_ssh(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.selected_ssh else { return };
        if i >= self.ssh_conns.len() {
            return;
        }
        self.ssh_conns[i] = self.ssh_conn_from_editor(cx);
        self.dirty += 1;
        self.select_ssh(i, cx);
    }

    /// 清空编辑器并取消选中，回到「填写新连接」草案态。
    fn new_ssh(&mut self, cx: &mut Context<Self>) {
        self.selected_ssh = None;
        for e in [
            &self.ssh_name_input,
            &self.ssh_host_input,
            &self.ssh_user_input,
            &self.ssh_key_input,
            &self.ssh_cmd_input,
            &self.ssh_sftp_input,
        ] {
            self.set_input(e, "", cx);
        }
        self.set_input(&self.ssh_port_input, "22", cx);
        cx.notify();
    }

    /// 弹出系统文件选择框选 SSH 私钥文件，结果写回私钥路径输入框。
    /// gpui Windows 无现成文件对话框，直接调 Win32 comdlg32 GetOpenFileNameW
    /// （PowerShell 子进程方案会闪 conhost 终端窗体，已废弃）。
    /// 同步等待返回（对话框打开期间配置窗口不重绘，属可接受代价）。
    fn pick_private_key_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = pick_file_win32(window, "选择 SSH 私钥文件");
        if let (Some(path), Some(entity)) = (path, self.ssh_key_input.as_ref()) {
            entity.update(cx, |input, cx| {
                input.content = path.into();
                cx.notify();
            });
        }
    }

    /// 弹出系统文件选择框选默认启动程序（可执行文件），结果写回输入框。
    /// 触发输入框的 observe 联动（split 逗号参数 → form.default_prog），无需重复赋值。
    fn pick_default_prog_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = pick_file_win32(window, "选择默认启动程序");
        if let (Some(path), Some(entity)) = (path, self.default_prog_input.as_ref()) {
            entity.update(cx, |input, cx| {
                input.content = path.into();
                cx.notify();
            });
        }
    }

    fn delete_ssh(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.selected_ssh.take() {
            if i < self.ssh_conns.len() {
                self.ssh_conns.remove(i);
                self.dirty += 1;
                for e in [
                    &self.ssh_name_input,
                    &self.ssh_host_input,
                    &self.ssh_user_input,
                    &self.ssh_key_input,
                    &self.ssh_cmd_input,
                    &self.ssh_sftp_input,
                ] {
                    self.set_input(e, "", cx);
                }
                self.set_input(&self.ssh_port_input, "22", cx);
                cx.notify();
            }
        }
    }

    /// 编辑器草案里私钥路径的状态：""=未填（不显示标识） "ok"=存在 "bad"=不存在
    fn ssh_editor_key_state(&self, cx: &App) -> &'static str {
        let Some(e) = &self.ssh_key_input else { return "" };
        let path = e.read(cx).content.trim();
        if path.is_empty() {
            return "";
        }
        if std::path::Path::new(path).is_file() {
            "ok"
        } else {
            "bad"
        }
    }

    fn render_ssh_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let conns = self.ssh_conns.clone();
        let selected = self.selected_ssh;

        let mut list = div().flex().flex_col().gap_1();
        for (i, c) in conns.iter().enumerate() {
            let is_sel = selected == Some(i);
            let entity = cx.entity();
            let addr = if c.port == 22 {
                c.host.clone()
            } else {
                format!("{}:{}", c.host, c.port)
            };
            list = list.child(
                div().flex().items_center().gap_2().px_2().py_1().rounded_sm().cursor_pointer()
                    .when(is_sel, |d| d.bg(theme::bg_elevated()))
                    .when(!is_sel, |d| d.bg(theme::bg_panel()))
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                        entity.update(app, |state: &mut ConfigUi, cx| state.select_ssh(i, cx));
                    })
                    .child(
                        div().w(px(110.)).text_size(px(12.)).font_family("JetBrains Mono")
                            .text_color(theme::fg_main())
                            .child(c.name.clone()),
                    )
                    .child(
                        div().text_size(px(12.)).text_color(theme::fg_dim()).child(addr),
                    ),
            );
        }
        if conns.is_empty() {
            list = list.child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("尚无连接，在右侧填写后点「添加连接」"),
            );
        }

        let has_sel = selected.is_some();
        let key_state = self.ssh_editor_key_state(cx);
        let editor = div().flex_1().min_w_0().flex().flex_col().gap_2()
            .p_2().rounded_md().bg(theme::bg_panel())
            .border_1().border_color(theme::border())
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("名称"))
                    .child(self.input(&self.ssh_name_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("主机"))
                    .child(self.input(&self.ssh_host_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("端口"))
                    .child(self.input(&self.ssh_port_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("用户名"))
                    .child(self.input(&self.ssh_user_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("私钥路径"))
                    .child(self.input(&self.ssh_key_input))
                    .child(btn("浏览…", cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.pick_private_key_file(window, cx)
                    })))
                    .when(!key_state.is_empty(), |d| {
                        d.child(
                            div().text_size(px(11.))
                                .text_color(if key_state == "ok" { theme::accent() } else { theme::danger() })
                                .child(if key_state == "ok" { "✓ 文件存在" } else { "✗ 文件不存在" }),
                        )
                    }),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("连接后命令"))
                    .child(self.input(&self.ssh_cmd_input)),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("SFTP 命令"))
                    .child(self.input(&self.ssh_sftp_input)),
            )
            .child(
                div().flex().gap_2()
                    .child(btn("添加连接", cx.listener(|this, _: &MouseDownEvent, _, cx| this.add_ssh(cx))))
                    .when(has_sel, |d| {
                        d.child(btn("新建", cx.listener(|this, _: &MouseDownEvent, _, cx| this.new_ssh(cx))))
                            .child(btn("应用修改", cx.listener(|this, _: &MouseDownEvent, _, cx| this.apply_ssh(cx))))
                            .child(btn("删除选中", cx.listener(|this, _: &MouseDownEvent, _, cx| this.delete_ssh(cx))))
                    }),
            )
            .child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("新增：先在上方填写各项，点「添加连接」才入列；修改：选中连接 → 改字段 → 点「应用修改」；「新建」清空编辑器重新填写。"),
            )
            .child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("认证顺序：私钥文件 → ssh-agent → 默认密钥；私钥路径留空即用 agent/默认密钥。保存后即可从终端连接。"),
            )
            .child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("连接后命令（可选）：登录后自动执行，如 cd /data/project；执行完进入交互 shell，命令失败也会正常进入。"),
            )
            .child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("SFTP 命令（可选）：点终端标签栏 SFTP 按钮时执行的外部工具命令，如 \"D:\\Program Files (x86)\\WinSCP\\WinSCP.exe\" \"会话名\" /newinstance；留空=按钮提示未配置。"),
            );

        div().flex().flex_col().gap_3()
            .child(
                div().flex().gap_3()
                    .child(
                        div().w(px(280.)).flex().flex_col().gap_1().p_2().rounded_md().bg(theme::bg_base())
                            .child(div().text_size(px(12.)).text_color(theme::fg_main()).child("已保存连接"))
                            .child(list),
                    )
                    .child(editor),
            )
            .child(
                div().text_size(px(12.)).text_color(theme::fg_dim()).child("Mux / SSH 后端："),
            )
            .child(self.render_setting_rows(9, cx))
    }

    fn render_keys_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.key_filter.clone();
        let dupes = keybinds::find_conflicts(&self.bindings);
        let bindings = self.bindings.clone();
        let selected = self.selected_binding;

        let row_matches =
            |mods: &str, key: &str, act: &str| -> bool {
                q.is_empty()
                    || mods.to_lowercase().contains(&q)
                    || key.to_lowercase().contains(&q)
                    || act.to_lowercase().contains(&q)
            };

        let mut list = div().flex().flex_col().gap_1();
        for (i, b) in bindings.iter().enumerate() {
            let disp = action_display(b);
            if !row_matches(&b.mods, &b.key, &disp) {
                continue;
            }
            let is_dup = dupes.contains(&b.combo());
            let is_sel = selected == Some(i);
            let entity = cx.entity();
            list = list.child(
                div().flex().items_center().gap_3().px_2().py_1().rounded_sm().cursor_pointer()
                    .when(is_sel, |d| d.bg(theme::bg_elevated()))
                    .when(!is_sel, |d| d.bg(theme::bg_panel()))
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                        entity.update(app, |state: &mut ConfigUi, cx| state.select_binding(i, cx));
                    })
                    .child(
                        div().w(px(150.)).text_size(px(12.)).font_family("JetBrains Mono")
                            .text_color(if is_dup { theme::danger() } else { theme::fg_main() })
                            .child(format!("{}+{}", b.mods.replace('|', "+"), b.key)),
                    )
                    .child(
                        div().text_size(px(12.))
                            .text_color(if is_dup { theme::danger() } else { theme::fg_dim() })
                            .child(disp),
                    ),
            );
        }
        list = list.child(
            div().mt_2().text_size(px(11.)).text_color(theme::fg_dim())
                .child("── 默认键位（常用，只读） ──"),
        );
        if bindings.is_empty() {
            list = list.child(
                div().text_size(px(11.)).text_color(theme::fg_dim())
                    .child("尚无自定义键位：在下方编辑器填写按键、选动作后，点「新增绑定」入列。"),
            );
        }
        for (dm, dk, dact) in keybinds::DEFAULT_BINDINGS {
            if !row_matches(dm, dk, dact) {
                continue;
            }
            let hit = bindings
                .iter()
                .any(|b| b.combo() == (keybinds::norm_mods(dm), dk.to_lowercase()));
            list = list.child(
                div().flex().items_center().gap_3().px_2().py_0p5().rounded_sm()
                    .child(
                        div().w(px(150.)).text_size(px(11.)).font_family("JetBrains Mono")
                            .text_color(theme::fg_dim())
                            .child(format!("{}+{}", dm.replace('|', "+"), dk)),
                    )
                    .child(
                        div().text_size(px(11.))
                            .text_color(if hit { theme::accent() } else { theme::fg_dim() })
                            .child(default_binding_action_cn(dact)),
                    ),
            );
        }

        // 编辑器面板：无选中时提示；有选中时显示字段 + 应用/删除
        let capture_label = if self.capture_mode { "请按下组合键… (Esc 取消)" } else { "录入按键" };
        // 编辑器三步布局（用户直觉流程）：①选动作 → ②录按键 → ③添加。
        // 步骤标号直接写在界面上，不需要说明书也能看懂顺序。
        let step_label = |text: &str| {
            div().text_size(px(11.)).text_color(theme::fg_dim()).child(text.to_string())
        };
        let editor = div().flex().flex_col().gap_2()
            .p_2().rounded_md()
            .bg(theme::bg_panel())
            .border_1().border_color(theme::border())
            .when(self.capture_mode, |d| {
                d.on_key_down(cx.listener(|this, e: &KeyDownEvent, _w, cx| {
                    this.capture_mode = false;
                    if e.keystroke.key != "escape" {
                        let (mods, key) = keystroke_to_wezterm(e);
                        this.set_input(&this.edit_mods_input, &mods, cx);
                        this.set_input(&this.edit_key_input, &key, cx);
                    }
                    cx.notify();
                }))
            })
            .child(step_label("第一步：选择动作"))
            .child(
                div().flex().flex_wrap().gap_1().children(
                    EDITABLE_ACTIONS.iter().enumerate().map(|(idx, (_, _, label))| {
                        let entity = cx.entity();
                        let is_sel = !self.custom_action.is_some() && self.edit_action == Some(idx);
                        div().px_2().py_0p5().rounded_sm().cursor_pointer().text_size(px(11.))
                            .when(is_sel, |d| d.bg(theme::accent_dim()).text_color(theme::fg_main()))
                            .when(!is_sel, |d| d.bg(theme::bg_elevated()).text_color(theme::fg_dim()))
                            .child(*label)
                            .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _: &mut Window, app: &mut App| {
                                entity.update(app, |state: &mut ConfigUi, cx| {
                                    state.edit_action = Some(idx);
                                    state.custom_action = None;
                                    cx.notify();
                                });
                            })
                    }),
                ),
            )
            .when_some(self.custom_action.clone(), |d, ca: (String, String)| {
                d.child(
                    div().text_size(px(11.)).text_color(theme::fg_dim())
                        .child(format!("当前选中行的动作「{}」不在上方清单里，点选任一动作会覆盖它", ca.0)),
                )
            })
            .child(step_label("第二步：录入按键（点按钮后直接按组合键）"))
            .child(
                div().flex().items_center().gap_2()
                    .child(lbl("修饰键"))
                    .child(self.input(&self.edit_mods_input))
                    .child(lbl("键名"))
                    .child(self.input(&self.edit_key_input))
                    .child(
                        div().px_2().py_1().rounded_sm().cursor_pointer()
                            .bg(if self.capture_mode { theme::accent_dim() } else { theme::bg_elevated() })
                            .text_size(px(12.)).text_color(theme::fg_main())
                            .child(capture_label)
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                this.capture_mode = !this.capture_mode;
                                // 按键事件只沿焦点路径冒泡:不聚焦时容器的 on_key_down
                                // 截不到任何键,用户就得先手动点一下输入框——进入捕获态
                                // 直接聚焦键名框,按下组合键即被截获。
                                if this.capture_mode {
                                    if let Some(input) = &this.edit_key_input {
                                        let handle = input.read(cx).focus_handle(cx);
                                        window.focus(&handle, cx);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(step_label("第三步：添加（或先在上方列表选中一行后修改/删除）"))
            .child(
                div().flex().gap_2()
                    // 与 SSH 连接页同一交互模型:未选中行时不显示「应用修改/
                    // 删除此绑定」——无选中时它们没有可写回的目标。
                    .when(selected.is_some(), |d| {
                        d.child(btn("应用修改", cx.listener(|this, _: &MouseDownEvent, _, cx| this.commit_editor(true, cx))))
                            .child(btn("删除此绑定", cx.listener(|this, _: &MouseDownEvent, _, cx| this.delete_binding(cx))))
                    })
                    .child(btn("添加键绑定", cx.listener(|this, _: &MouseDownEvent, _, cx| this.commit_editor(false, cx)))),
            );

        div().flex().flex_col().gap_3()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _w, cx| {
                // 兜底：捕获态下即使焦点在输入框，也在标签页层截获
                if this.capture_mode {
                    this.capture_mode = false;
                    if e.keystroke.key != "escape" {
                        let (mods, key) = keystroke_to_wezterm(e);
                        this.set_input(&this.edit_mods_input, &mods, cx);
                        this.set_input(&this.edit_key_input, &key, cx);
                    }
                    cx.notify();
                }
            }))
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(120.)).text_size(px(13.)).text_color(theme::fg_main()).child("搜索"))
                    .child(self.input(&self.key_search)),
            )
            .child(list.h(px(320.)).id("keys-list").overflow_y_scroll())
            .child(editor)
    }
}

fn lbl(text: &str) -> impl IntoElement {
    div().w(px(60.)).text_size(px(12.)).text_color(theme::fg_main()).child(text.to_string())
}

/// 颜色字面量校验：#RGB / #RRGGBB / #RRGGBBAA，或纯字母的命名色
/// （wezterm 接受 X11 颜色名，如 red/tomato）。裸十六进制、符号、
/// 中文等无法确认有效的形式拦下——它们会发射非法 Lua 使配置加载失败。
fn is_valid_color(s: &str) -> bool {
    if let Some(hex) = s.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    !s.is_empty() && s.len() <= 30 && s.chars().all(|c| c.is_ascii_alphabetic())
}

/// Int/Float/Str/List 类设置用文本输入；Bool 用开关、Enum 用分段选择渲染。
fn setting_kind_needs_input(kind: Kind) -> bool {
    matches!(kind, Kind::Int { .. } | Kind::Float { .. } | Kind::Str | Kind::List)
}

fn btn(text: &str, listener: impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static) -> impl IntoElement {
    div().px_2().py_1().rounded_sm().cursor_pointer()
        .bg(theme::bg_elevated())
        .text_size(px(12.)).text_color(theme::fg_main())
        .child(text.to_string())
        .on_mouse_down(MouseButton::Left, move |e, w, app| listener(e, w, app))
}

/// 修饰键自由文本 → 规范形式（分隔符 + / | 皆可，令牌大小写不敏感）。
/// 无法识别的令牌返回 Err(原文)，绝不让垃圾文本写进配置（否则快捷键永不生效）。
fn normalize_mods(s: &str) -> Result<String, String> {
    let mut parts: Vec<&'static str> = vec![];
    for token in s.split(['|', '+']) {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        let canonical = match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL",
            "shift" => "SHIFT",
            "alt" | "option" | "opt" => "ALT",
            "super" | "cmd" | "command" | "win" | "windows" | "meta" => "SUPER",
            other => return Err(other.to_string()),
        };
        if !parts.contains(&canonical) {
            parts.push(canonical);
        }
    }
    Ok(parts.join("|"))
}

/// 键名自由文本 → 规范形式：常见特殊键大小写不敏感映射（含录入器产出的
/// gpui 小写名与用户手输的变体），F1-F24 规范大小写；其余原样保留。
/// 含空白或非 ASCII（如中文）直接报错——这类值写进配置后快捷键永不生效。
fn normalize_key(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("键名为空，请填写或点「录入按键」按物理组合键录入".into());
    }
    if t.chars().any(|c| c.is_whitespace() || !c.is_ascii()) {
        return Err(format!("键名「{t}」无法识别，请点「录入按键」按物理组合键录入"));
    }
    let lower = t.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "left" | "leftarrow" => "LeftArrow",
        "right" | "rightarrow" => "RightArrow",
        "up" | "uparrow" => "UpArrow",
        "down" | "downarrow" => "DownArrow",
        "enter" | "return" => "Enter",
        "escape" | "esc" => "Escape",
        "space" => "Space",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        other => {
            if let Some(n) = other.strip_prefix('f') {
                if let Ok(num) = n.parse::<u32>() {
                    if (1..=24).contains(&num) {
                        return Ok(format!("F{num}"));
                    }
                }
            }
            return Ok(t.to_string());
        }
    };
    Ok(canonical.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_display_maps_special_keys() {
        assert_eq!(key_display("left"), "LeftArrow");
        assert_eq!(key_display("pagedown"), "PageDown");
        assert_eq!(key_display("c"), "c");
    }

    #[test]
    fn keystroke_maps_modifiers() {
        use gpui::Keystroke;
        let ks = Keystroke::parse("ctrl-shift-c").unwrap();
        let e = KeyDownEvent { keystroke: ks, is_held: false, prefer_character_input: false };
        let (mods, key) = keystroke_to_wezterm(&e);
        assert_eq!(mods.split('|').collect::<Vec<_>>().len(), 2);
        assert_eq!(key, "c");
    }

    #[test]
    fn normalize_mods_canonicalizes_free_text() {
        assert_eq!(normalize_mods("alt").unwrap(), "ALT");
        assert_eq!(normalize_mods("ctrl+shift").unwrap(), "CTRL|SHIFT");
        assert_eq!(normalize_mods("CTRL | SHIFT").unwrap(), "CTRL|SHIFT");
        assert_eq!(normalize_mods("win").unwrap(), "SUPER");
        assert_eq!(normalize_mods("").unwrap(), "");
        assert!(normalize_mods("向右").is_err());
    }

    #[test]
    fn normalize_key_canonicalizes_free_text() {
        assert_eq!(normalize_key("right").unwrap(), "RightArrow");
        assert_eq!(normalize_key("RightArrow").unwrap(), "RightArrow");
        assert_eq!(normalize_key("esc").unwrap(), "Escape");
        assert_eq!(normalize_key("f12").unwrap(), "F12");
        assert_eq!(normalize_key("c").unwrap(), "c");
        assert!(normalize_key("").is_err());
        assert!(normalize_key("向右").is_err());
    }

    #[test]
    fn color_validation_accepts_wezterm_forms() {
        assert!(is_valid_color("#1b1d2e"));
        assert!(is_valid_color("#fff"));
        assert!(is_valid_color("#1b1d2e80"));
        assert!(is_valid_color("red"));
        assert!(!is_valid_color(""));
        assert!(!is_valid_color("1b1d2e"));
        assert!(!is_valid_color("#xyz"));
        assert!(!is_valid_color("红色"));
    }
}

const GROUPS: &[&str] = &[
    "字体与光标", "配色", "窗口外观", "标签栏", "启动与默认行为", "终端行为", "鼠标与选择",
    "键绑定", "SSH 连接",
];

impl Render for ConfigUi {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg_base())
            .text_color(theme::fg_main())
            // Tab/Shift+Tab 移动焦点;gpui 的 focus_next/prev 到序列末尾不环绕,
            // 这里补常规对话框的循环语义(焦点没变=已到头,blur 后再走一次=回绕)
            .on_action(cx.listener(|_, _: &MoveFocusNext, window, cx| {
                let before = window.focused(cx);
                window.focus_next(cx);
                if window.focused(cx) == before && before.is_some() {
                    window.blur();
                    window.focus_next(cx);
                }
            }))
            .on_action(cx.listener(|_, _: &MoveFocusPrev, window, cx| {
                let before = window.focused(cx);
                window.focus_prev(cx);
                if window.focused(cx) == before && before.is_some() {
                    window.blur();
                    window.focus_prev(cx);
                }
            }))
            .child(self.render_action_bar(cx))
            .when(self.error.is_some(), |d| {
                d.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1()
                        .bg(gpui::rgb(0x5c2a2a))
                        .text_color(theme::fg_main())
                        .text_size(px(12.))
                        .child(self.error.clone().unwrap_or_default()),
                )
            })
            .child(
                div().flex_1().min_h_0().flex()
                    .child(self.render_sidebar(cx))
                    .child(self.render_content(cx))
            )
    }
}

/// TextInput 内容变化 → 表单 的单向观察者。apply 内部只在值真正变化时写 form/dirty，
/// 因此 sync_inputs 反向回填内容不会触发额外脏计数，天然无环。
fn observe_input(
    entity: &Entity<TextInput>,
    cx: &mut Context<ConfigUi>,
    mut apply: impl FnMut(&mut ConfigUi, &str, &mut Context<ConfigUi>) + 'static,
) {
    cx.observe(entity, move |this, e: Entity<TextInput>, cx| {
        let content = e.read(cx).content.to_string();
        apply(this, &content, cx);
        cx.notify();
    })
    .detach();
}

fn fmt_load_error(e: &LoadError) -> String {
    let mut s = format!("配置加载失败（第 {} 行）：{}", e.line.unwrap_or(0), e.message);
    for w in &e.warnings {
        s.push_str(&format!("\n警告：{w}"));
    }
    s
}

/// Windows 平台把 RGBA 图标设到 gpui 窗口。gpui 的 `WindowOptions.icon` 注释为
/// "X11 only" 在 Windows 不生效，故窗口创建后裸调 Win32：基于 wezterm `window::os::windows::WindowOps::set_icon`
/// 同样的实现思路（CreateDIBSection + CreateIconIndirect + SendMessage WM_SETICON）。
#[cfg(windows)]
fn set_window_icon(window: &Window, rgba: &image::RgbaImage) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winapi::shared::windef::HWND;

    let raw = match HasWindowHandle::window_handle(window) {
        Ok(h) => h,
        Err(_) => return,
    };
    let hwnd: HWND = match raw.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get() as HWND,
        _ => return,
    };
    if hwnd.is_null() {
        return;
    }

    let width = rgba.width() as i32;
    let height = rgba.height() as i32;
    let mut bi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = width;
    bi.bmiHeader.biHeight = -height; // top-down
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut u8 = std::ptr::null_mut();
    let hdc = unsafe { GetDC(hwnd) };
    let hbitmap = unsafe {
        CreateDIBSection(
            hdc,
            &bi,
            DIB_RGB_COLORS,
            &mut bits as *mut _ as *mut _,
            null_mut(),
            0,
        )
    };
    unsafe { ReleaseDC(hwnd, hdc) };

    if hbitmap.is_null() {
        return;
    }

    // image::RgbaImage is straight RGBA; Windows 32-bit DIB 期望 BGRA 且需要 pre-multiplied alpha
    let (w, h) = rgba.dimensions();
    let src = rgba.as_raw();
    unsafe {
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + x) as usize) * 4;
                let r = src[s] as u32;
                let g = src[s + 1] as u32;
                let b = src[s + 2] as u32;
                let a = src[s + 3];
                *bits.add(s) = ((b * a as u32) / 255) as u8;
                *bits.add(s + 1) = ((g * a as u32) / 255) as u8;
                *bits.add(s + 2) = ((r * a as u32) / 255) as u8;
                *bits.add(s + 3) = a;
            }
        }
    }

    let hmask = unsafe { CreateBitmap(width, height, 1, 1, null_mut()) };

    let mut icon_info: ICONINFO = unsafe { std::mem::zeroed() };
    icon_info.fIcon = TRUE;
    icon_info.hbmColor = hbitmap as _;
    icon_info.hbmMask = hmask as _;

    let hicon: HICON = unsafe { CreateIconIndirect(&mut icon_info) };

    unsafe {
        DeleteObject(hbitmap as _);
        DeleteObject(hmask as _);
    }

    if !hicon.is_null() {
        // WM_SETICON 的 ICON_BIG/ICON_SMALL 都设，标题栏 + 任务栏都生效
        unsafe {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, hicon as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, hicon as isize);
        }
    }
}

#[cfg(not(windows))]
fn set_window_icon(_window: &Window, _rgba: &image::RgbaImage) {
    // non-Windows: gpui 走 X11/Wayland，WindowOptions.icon 已处理
}

/// Windows 原生「打开文件」对话框（comdlg32 GetOpenFileNameW）。
/// 返回用户选中的文件路径；取消/失败返回 None。
/// 注：经典样式对话框（非 Vista 新样式），对本场景足够。
#[cfg(windows)]
fn pick_file_win32(window: &Window, title: &str) -> Option<String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winapi::shared::windef::HWND;
    use winapi::um::commdlg::{GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_HIDEREADONLY, OFN_PATHMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW};

    let raw = HasWindowHandle::window_handle(window).ok()?;
    let hwnd: HWND = match raw.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get() as HWND,
        _ => return None,
    };

    let mut title_w: Vec<u16> = title.encode_utf16().collect();
    title_w.push(0);
    let filter_w: Vec<u16> = "所有文件\0*.*\0\0"
        .encode_utf16()
        .collect();
    // 双 NUL 结尾由末尾 \0\0 保证
    let mut file_buf = [0u16; 1024];

    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrTitle = title_w.as_ptr();
    ofn.lpstrFilter = filter_w.as_ptr();
    ofn.lpstrFile = file_buf.as_mut_ptr();
    ofn.nMaxFile = file_buf.len() as u32;
    ofn.Flags = OFN_EXPLORER | OFN_HIDEREADONLY | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    let _ = OFN_ALLOWMULTISELECT; // 抑制未使用警告（未启用多选）

    let ok = unsafe { GetOpenFileNameW(&mut ofn) };
    if ok == winapi::shared::minwindef::TRUE {
        let len = file_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(file_buf.len());
        let path = String::from_utf16_lossy(&file_buf[..len]);
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    } else {
        None
    }
}

#[cfg(not(windows))]
fn pick_file_win32(_window: &Window, _title: &str) -> Option<String> {
    // non-Windows：未实现（当前仅 Windows 分发）
    None
}

/// 加载 wezterm 出厂默认 config 的 JSON 快照。GUI 始终以「与默认不同的字段」为 diff 基准，
/// 避免每次保存都把全表 dump 出来。
fn load_default_snapshot() -> serde_json::Value {
    let src = "return {}\n";
    match load_from_source(src, std::path::Path::new("defaults.lua")) {
        Ok(loaded) => {
            let json = config_to_json(&loaded.config);
            if json.is_null() {
                serde_json::json!({})
            } else {
                json
            }
        }
        Err(_) => serde_json::json!({}),
    }
}
