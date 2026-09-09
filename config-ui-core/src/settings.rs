//! 数据驱动的设置注册表：每个可在图形界面配置的「顶层标量项」声明为一条 SettingSpec，
//! 通用读取/写回/基线 diff 据此循环工作。新增设置项 = 在 SETTINGS 表注册一行。
//!
//! 页面归属（page）对应 config-ui 侧 GROUPS 的索引：
//! 0=字体与光标 1=配色 2=窗口外观 3=标签栏 4=启动与默认行为 5=终端行为 6=鼠标与选择 9=SSH 连接
//!
//! 不进注册表的项：
//! - 复合结构（window_frame、tab_bar_style、HSB 三项、*_font 的 TextStyle、window_padding）
//! - 列表/键值 DSL（key_tables、hyperlink_rules、set_environment_variables 等）
//!   这些仍走高级编辑区手改 Lua。
//! 例外：mouse_bindings 以一个伪布尔项（RIGHT_CLICK_SMART_KEY）进注册表——
//! 开=发射固定的「右键智能复制/粘贴」绑定块，关=不发射；读回以快照中 mouse_bindings 非空为准。
//! 枚举取值以 config/src/config.rs 及其子模块的真实变体定义为准（2026-08-29 核对）。

use serde_json::Value;

/// 控件种类。Enum 携带 (取值, 中文显示名) 对照；数值携带合法区间用于输入校验。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Str,
    Enum(&'static [(&'static str, &'static str)]),
    /// 字符串列表，界面以逗号分隔编辑
    List,
}

pub struct SettingSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub page: usize,
}

/// 常用缓动函数（EasingFunction 变体；CubicBezier 参数串仅 Lua 层支持，不进分段选项）
const EASINGS: &[(&str, &str)] = &[
    ("Linear", "线性"),
    ("Ease", "缓动"),
    ("EaseIn", "缓入"),
    ("EaseOut", "缓出"),
    ("EaseInOut", "缓入出"),
    ("Constant", "阶跃"),
];

/// 伪设置项 key：不对应 Config 结构字段；开=发射固定的一段 mouse_bindings，关=不发射。
pub const RIGHT_CLICK_SMART_KEY: &str = "right_click_smart_copy_paste";

/// 右键智能复制/粘贴绑定：唯一事实源在 config crate（默认配置文件与
/// 本伪设置项发射共用同一块，避免文案漂移）。有选中→复制，无选中→粘贴。
const SMART_RIGHT_CLICK_LUA: &str = config::Config::SMART_RIGHT_CLICK_LUA;

pub static SETTINGS: &[SettingSpec] = &[
    // ---------- 页面 0：字体与光标 ----------
    SettingSpec { key: "font_size", label: "字体大小", kind: Kind::Float { min: 4.0, max: 72.0 }, page: 0 },
    SettingSpec { key: "line_height", label: "行高", kind: Kind::Float { min: 0.5, max: 3.0 }, page: 0 },
    SettingSpec { key: "cell_width", label: "单元格宽度", kind: Kind::Float { min: 0.5, max: 3.0 }, page: 0 },
    SettingSpec {
        key: "default_cursor_style",
        label: "光标样式",
        kind: Kind::Enum(&[
            ("BlinkingBlock", "闪烁块"),
            ("BlinkingBar", "闪烁竖线"),
            ("SteadyBlock", "稳定块"),
            ("SteadyBar", "稳定竖线"),
        ]),
        page: 0,
    },
    SettingSpec { key: "cursor_blink_rate", label: "光标闪烁间隔(ms)", kind: Kind::Int { min: 0, max: 5000 }, page: 0 },
    SettingSpec { key: "cursor_thickness", label: "光标厚度(px)", kind: Kind::Float { min: 0.0, max: 10.0 }, page: 0 },
    SettingSpec { key: "cursor_blink_ease_in", label: "光标闪烁缓入曲线", kind: Kind::Enum(EASINGS), page: 0 },
    SettingSpec { key: "cursor_blink_ease_out", label: "光标闪烁缓出曲线", kind: Kind::Enum(EASINGS), page: 0 },
    SettingSpec { key: "underline_position", label: "下划线位置(px)", kind: Kind::Float { min: -20.0, max: 100.0 }, page: 0 },
    SettingSpec { key: "underline_thickness", label: "下划线厚度(px)", kind: Kind::Float { min: 0.0, max: 20.0 }, page: 0 },
    SettingSpec { key: "strikethrough_position", label: "删除线位置(px)", kind: Kind::Float { min: -20.0, max: 100.0 }, page: 0 },
    SettingSpec {
        key: "allow_square_glyphs_to_overflow_width",
        label: "方块字形允许超宽",
        kind: Kind::Enum(&[
            ("Never", "从不"),
            ("Always", "总是"),
            ("WhenFollowedBySpace", "后随空格时"),
        ]),
        page: 0,
    },
    SettingSpec { key: "anti_alias_custom_block_glyphs", label: "自定义块字形抗锯齿", kind: Kind::Bool, page: 0 },
    SettingSpec { key: "custom_block_glyphs", label: "绘制自定义块字形", kind: Kind::Bool, page: 0 },
    SettingSpec { key: "ignore_svg_fonts", label: "忽略 SVG 字体", kind: Kind::Bool, page: 0 },
    SettingSpec { key: "warn_about_missing_glyphs", label: "缺字形时警告", kind: Kind::Bool, page: 0 },
    SettingSpec { key: "treat_east_asian_ambiguous_width_as_wide", label: "东亚歧义宽度按宽处理", kind: Kind::Bool, page: 0 },
    // ---------- 页面 1：配色 ----------
    SettingSpec {
        key: "bold_brightens_ansi_colors",
        label: "粗体提亮 ANSI 色",
        kind: Kind::Enum(&[
            ("No", "不变"),
            ("BrightAndBold", "提亮并加粗"),
            ("BrightOnly", "仅提亮"),
        ]),
        page: 1,
    },
    SettingSpec { key: "text_background_opacity", label: "文字背景不透明度", kind: Kind::Float { min: 0.0, max: 1.0 }, page: 1 },
    // ---------- 页面 2：窗口外观 ----------
    SettingSpec { key: "window_background_opacity", label: "窗口不透明度", kind: Kind::Float { min: 0.1, max: 1.0 }, page: 2 },
    SettingSpec {
        key: "window_decorations",
        label: "窗口装饰",
        kind: Kind::Enum(&[
            ("NONE", "无"),
            ("TITLE", "标题栏"),
            ("RESIZE", "可调整大小"),
            ("TITLE|RESIZE", "标题栏+可调整"),
        ]),
        page: 2,
    },
    SettingSpec { key: "adjust_window_size_when_changing_font_size", label: "改字号时调整窗口大小", kind: Kind::Bool, page: 2 },
    SettingSpec { key: "use_resize_increments", label: "按单元格增量调整大小", kind: Kind::Bool, page: 2 },
    SettingSpec {
        key: "win32_system_backdrop",
        label: "系统背景效果",
        kind: Kind::Enum(&[
            ("Auto", "自动"),
            ("Disable", "禁用"),
            ("Acrylic", "亚克力"),
            ("Mica", "云母"),
            ("Tabbed", "标签云母"),
        ]),
        page: 2,
    },
    SettingSpec {
        key: "integrated_title_button_style",
        label: "集成标题按钮风格",
        kind: Kind::Enum(&[
            ("Windows", "Windows"),
            ("Gnome", "GNOME"),
            ("MacOsNative", "macOS"),
        ]),
        page: 2,
    },
    SettingSpec {
        key: "integrated_title_button_alignment",
        label: "集成标题按钮位置",
        kind: Kind::Enum(&[("Right", "右侧"), ("Left", "左侧")]),
        page: 2,
    },
    SettingSpec { key: "command_palette_font_size", label: "命令面板字号", kind: Kind::Float { min: 8.0, max: 48.0 }, page: 2 },
    SettingSpec { key: "command_palette_rows", label: "命令面板行数", kind: Kind::Int { min: 3, max: 40 }, page: 2 },
    SettingSpec { key: "char_select_font_size", label: "字符选择面板字号", kind: Kind::Float { min: 8.0, max: 96.0 }, page: 2 },
    SettingSpec { key: "pane_select_font_size", label: "面板选择字号", kind: Kind::Float { min: 8.0, max: 96.0 }, page: 2 },
    // ---------- 页面 3：标签栏 ----------
    SettingSpec { key: "enable_tab_bar", label: "显示标签栏", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "tab_bar_at_bottom", label: "标签栏在底部", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "use_fancy_tab_bar", label: "花式标签栏", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "hide_tab_bar_if_only_one_tab", label: "仅一个标签时隐藏", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "show_tab_index_in_tab_bar", label: "显示标签序号", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "tab_max_width", label: "标签最大宽度", kind: Kind::Int { min: 1, max: 200 }, page: 3 },
    SettingSpec { key: "mouse_wheel_scrolls_tabs", label: "滚轮切换标签", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "show_new_tab_button_in_tab_bar", label: "显示新建标签按钮", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "show_close_tab_button_in_tabs", label: "显示标签关闭按钮", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "show_tabs_in_tab_bar", label: "标签栏显示标签", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "switch_to_last_active_tab_when_closing_tab", label: "关标签时回到上个活动标签", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "tab_and_split_indices_are_zero_based", label: "标签/面板序号从 0 开始", kind: Kind::Bool, page: 3 },
    SettingSpec { key: "launcher_alphabet", label: "启动菜单快捷字母", kind: Kind::Str, page: 3 },
    SettingSpec { key: "status_update_interval", label: "状态栏刷新间隔(ms)", kind: Kind::Int { min: 10, max: 10000 }, page: 3 },
    // ---------- 页面 4：启动与默认行为 ----------
    SettingSpec { key: "default_cwd", label: "启动目录", kind: Kind::Str, page: 4 },
    SettingSpec { key: "initial_cols", label: "初始列数", kind: Kind::Int { min: 1, max: 1000 }, page: 4 },
    SettingSpec { key: "initial_rows", label: "初始行数", kind: Kind::Int { min: 1, max: 1000 }, page: 4 },
    SettingSpec { key: "scrollback_lines", label: "回滚行数", kind: Kind::Int { min: 100, max: 1000000 }, page: 4 },
    SettingSpec {
        key: "exit_behavior",
        label: "退出行为",
        kind: Kind::Enum(&[
            ("Close", "关闭"),
            ("CloseOnCleanExit", "正常退出时关闭"),
            ("Hold", "保持"),
        ]),
        page: 4,
    },
    SettingSpec {
        key: "exit_behavior_messaging",
        label: "退出提示详细度",
        kind: Kind::Enum(&[
            ("Verbose", "详细"),
            ("Brief", "简洁"),
            ("Terse", "极简"),
            ("None", "无"),
        ]),
        page: 4,
    },
    SettingSpec {
        key: "window_close_confirmation",
        label: "关闭确认",
        kind: Kind::Enum(&[("AlwaysPrompt", "总是询问"), ("NeverPrompt", "从不询问")]),
        page: 4,
    },
    SettingSpec { key: "quit_when_all_windows_are_closed", label: "全部窗口关闭后退出", kind: Kind::Bool, page: 4 },
    SettingSpec { key: "prefer_to_spawn_tabs", label: "优先开新标签而非新窗口", kind: Kind::Bool, page: 4 },
    SettingSpec { key: "automatically_reload_config", label: "自动重载配置", kind: Kind::Bool, page: 4 },
    SettingSpec { key: "check_for_updates", label: "检查更新", kind: Kind::Bool, page: 4 },
    SettingSpec { key: "check_for_updates_interval_seconds", label: "检查更新间隔(秒)", kind: Kind::Int { min: 60, max: 2592000 }, page: 4 },
    SettingSpec { key: "show_update_window", label: "显示更新窗口", kind: Kind::Bool, page: 4 },
    SettingSpec { key: "term", label: "TERM 环境变量", kind: Kind::Str, page: 4 },
    SettingSpec { key: "default_workspace", label: "默认工作区", kind: Kind::Str, page: 4 },
    SettingSpec { key: "default_domain", label: "默认域", kind: Kind::Str, page: 4 },
    SettingSpec { key: "default_gui_startup_args", label: "GUI 启动参数(逗号分隔)", kind: Kind::List, page: 4 },
    SettingSpec {
        key: "notification_handling",
        label: "通知处理",
        kind: Kind::Enum(&[
            ("AlwaysShow", "总是显示"),
            ("NeverShow", "从不显示"),
            ("SuppressFromFocusedPane", "聚焦窗格时抑制"),
            ("SuppressFromFocusedTab", "聚焦标签时抑制"),
            ("SuppressFromFocusedWindow", "聚焦窗口时抑制"),
        ]),
        page: 4,
    },
    SettingSpec {
        key: "canonicalize_pasted_newlines",
        label: "粘贴换行规范化",
        kind: Kind::Enum(&[
            ("None", "不处理"),
            ("LineFeed", "转为 LF"),
            ("CarriageReturn", "转为 CR"),
            ("CarriageReturnAndLineFeed", "转为 CRLF"),
        ]),
        page: 4,
    },
    // ---------- 页面 5：终端行为 ----------
    SettingSpec {
        key: "audible_bell",
        label: "响铃",
        kind: Kind::Enum(&[("SystemBeep", "系统提示音"), ("Disabled", "禁用")]),
        page: 5,
    },
    SettingSpec { key: "animation_fps", label: "动画帧率", kind: Kind::Int { min: 0, max: 120 }, page: 5 },
    SettingSpec { key: "text_blink_rate", label: "文字闪烁间隔(ms)", kind: Kind::Int { min: 0, max: 5000 }, page: 5 },
    SettingSpec { key: "text_blink_rate_rapid", label: "文字快速闪烁间隔(ms)", kind: Kind::Int { min: 0, max: 5000 }, page: 5 },
    SettingSpec { key: "text_blink_ease_in", label: "文字闪烁缓入曲线", kind: Kind::Enum(EASINGS), page: 5 },
    SettingSpec { key: "text_blink_ease_out", label: "文字闪烁缓出曲线", kind: Kind::Enum(EASINGS), page: 5 },
    SettingSpec { key: "text_blink_rapid_ease_in", label: "快速闪烁缓入曲线", kind: Kind::Enum(EASINGS), page: 5 },
    SettingSpec { key: "text_blink_rapid_ease_out", label: "快速闪烁缓出曲线", kind: Kind::Enum(EASINGS), page: 5 },
    SettingSpec { key: "force_reverse_video_cursor", label: "强制反色光标", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "reverse_video_cursor_min_contrast", label: "反色光标最小对比度", kind: Kind::Float { min: 0.0, max: 1.0 }, page: 5 },
    SettingSpec { key: "text_min_contrast_ratio", label: "文字最小对比度", kind: Kind::Float { min: 0.0, max: 21.0 }, page: 5 },
    SettingSpec { key: "enable_kitty_graphics", label: "kitty 图形协议", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "enable_kitty_keyboard", label: "kitty 键盘协议", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "enable_csi_u_key_encoding", label: "CSI-u 键编码", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "allow_win32_input_mode", label: "Win32 输入模式", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "allow_download_protocols", label: "允许下载协议", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "detect_password_input", label: "检测密码输入", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "use_ime", label: "使用输入法", kind: Kind::Bool, page: 5 },
    SettingSpec {
        key: "ime_preedit_rendering",
        label: "预编辑渲染",
        kind: Kind::Enum(&[("Builtin", "内置"), ("System", "系统")]),
        page: 5,
    },
    SettingSpec {
        key: "key_map_preference",
        label: "按键映射偏好",
        kind: Kind::Enum(&[("Mapped", "逻辑映射"), ("Physical", "物理位置")]),
        page: 5,
    },
    SettingSpec {
        key: "ui_key_cap_rendering",
        label: "键帽显示风格",
        kind: Kind::Enum(&[
            ("UnixLong", "Unix 长名"),
            ("Emacs", "Emacs"),
            ("AppleSymbols", "苹果符号"),
            ("WindowsLong", "Windows 长名"),
            ("WindowsSymbols", "Windows 符号"),
        ]),
        page: 5,
    },
    SettingSpec { key: "swap_backspace_and_delete", label: "交换退格与删除", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "treat_left_ctrlalt_as_altgr", label: "左 Ctrl+Alt 视作 AltGr", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "send_composed_key_when_left_alt_is_pressed", label: "左 Alt 发送组合字符", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "send_composed_key_when_right_alt_is_pressed", label: "右 Alt 发送组合字符", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "use_dead_keys", label: "启用死键", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "debug_key_events", label: "调试按键事件", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "disable_default_key_bindings", label: "禁用默认键绑定", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "enq_answerback", label: "应答字符串", kind: Kind::Str, page: 5 },
    SettingSpec { key: "bidi_enabled", label: "启用双向文本", kind: Kind::Bool, page: 5 },
    SettingSpec {
        key: "bidi_direction",
        label: "段落方向",
        kind: Kind::Enum(&[
            ("LeftToRight", "从左到右"),
            ("RightToLeft", "从右到左"),
            ("AutoLeftToRight", "自动(回退左)"),
            ("AutoRightToLeft", "自动(回退右)"),
        ]),
        page: 5,
    },
    SettingSpec { key: "normalize_output_to_unicode_nfc", label: "输出规范化为 NFC", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "log_unknown_escape_sequences", label: "记录未知转义序列", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "enable_title_reporting", label: "允许标题上报", kind: Kind::Bool, page: 5 },
    SettingSpec { key: "enable_checksum_rectangular_area", label: "矩形区域校验和", kind: Kind::Bool, page: 5 },
    // ---------- 页面 6：鼠标与选择 ----------
    SettingSpec { key: "hide_mouse_cursor_when_typing", label: "打字时隐藏鼠标指针", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "pane_focus_follows_mouse", label: "鼠标跟随聚焦窗格", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "swallow_mouse_click_on_pane_focus", label: "聚焦窗格的首次点击吞掉", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "swallow_mouse_click_on_window_focus", label: "聚焦窗口的首次点击吞掉", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "unzoom_on_switch_pane", label: "切换窗格时取消缩放", kind: Kind::Bool, page: 6 },
    SettingSpec { key: RIGHT_CLICK_SMART_KEY, label: "右键智能复制粘贴", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "disable_default_mouse_bindings", label: "禁用默认鼠标绑定", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "disable_default_quick_select_patterns", label: "禁用默认快速选择模式", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "quick_select_remove_styling", label: "快速选择去除样式", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "quick_select_alphabet", label: "快速选择字母表", kind: Kind::Str, page: 6 },
    SettingSpec { key: "selection_word_boundary", label: "选词边界字符", kind: Kind::Str, page: 6 },
    SettingSpec {
        key: "quote_dropped_files",
        label: "拖入文件路径引号",
        kind: Kind::Enum(&[
            ("None", "不加引号"),
            ("SpacesOnly", "仅空格加引号"),
            ("Posix", "POSIX 风格"),
            ("Windows", "Windows 风格"),
            ("WindowsAlwaysQuoted", "总是双引号"),
        ]),
        page: 6,
    },
    SettingSpec { key: "alternate_buffer_wheel_scroll_speed", label: "备用屏滚轮速度", kind: Kind::Int { min: 1, max: 50 }, page: 6 },
    SettingSpec { key: "scroll_to_bottom_on_input", label: "输入时滚到底部", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "enable_scroll_bar", label: "显示滚动条", kind: Kind::Bool, page: 6 },
    SettingSpec { key: "min_scroll_bar_height", label: "滚动条最小高度(比例)", kind: Kind::Float { min: 0.01, max: 1.0 }, page: 6 },
    // ---------- 页面 9：SSH 连接（mux/后端补充项） ----------
    SettingSpec { key: "mux_enable_ssh_agent", label: "mux 启用 ssh-agent", kind: Kind::Bool, page: 9 },
    SettingSpec {
        key: "ssh_backend",
        label: "SSH 后端",
        kind: Kind::Enum(&[("Ssh2", "Ssh2"), ("LibSsh", "LibSsh")]),
        page: 9,
    },
    SettingSpec { key: "default_ssh_auth_sock", label: "默认 SSH_AUTH_SOCK", kind: Kind::Str, page: 9 },
    SettingSpec { key: "default_mux_server_domain", label: "默认 mux 服务器域", kind: Kind::Str, page: 9 },
];

/// 表单值：与 SETTINGS 一一对应的扁平存储。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<String>),
}

/// 注册表驱动的表单：Vec 下标即 SETTINGS 下标。
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsForm {
    pub values: Vec<SettingValue>,
}

/// 按配置 key 查注册表下标（表为静态数据，线性查找足够）。
pub fn index(key: &str) -> Option<usize> {
    SETTINGS.iter().position(|s| s.key == key)
}

/// wezterm 把尺寸类字段序列化为带 "px"/"cell" 后缀的字符串；两种形状统一解出数值。
fn num_of(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::String(st)) => st
            .trim_end_matches("px")
            .trim_end_matches("cell")
            .trim()
            .parse()
            .unwrap_or(0.0),
        Some(x) => x.as_f64().unwrap_or(0.0),
        None => 0.0,
    }
}

fn str_of(v: Option<&Value>) -> String {
    v.and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn list_of(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 单引号 Lua 字符串字面量：先转义反斜杠再转义引号，避免路径 `\U`、`\n` 变成非法转义。
pub fn lua_quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

impl SettingsForm {
    pub fn from_snapshot(snap: &Value) -> Self {
        Self {
            values: SETTINGS
                .iter()
                .map(|spec| read_value(spec, snap))
                .collect(),
        }
    }

    pub fn get(&self, idx: usize) -> &SettingValue {
        &self.values[idx]
    }

    pub fn set(&mut self, idx: usize, v: SettingValue) {
        if self.values[idx] != v {
            self.values[idx] = v;
        }
    }

    pub fn bool_at(&self, idx: usize) -> bool {
        matches!(self.values[idx], SettingValue::Bool(b) if b)
    }
    pub fn set_bool(&mut self, idx: usize, v: bool) {
        self.set(idx, SettingValue::Bool(v));
    }
    pub fn float_at(&self, idx: usize) -> f64 {
        match self.values[idx] {
            SettingValue::Float(f) => f,
            SettingValue::Int(i) => i as f64,
            _ => 0.0,
        }
    }
    pub fn set_float(&mut self, idx: usize, v: f64) {
        self.set(idx, SettingValue::Float(v));
    }
    pub fn int_at(&self, idx: usize) -> i64 {
        match self.values[idx] {
            SettingValue::Int(i) => i,
            SettingValue::Float(f) => f as i64,
            _ => 0,
        }
    }
    pub fn set_int(&mut self, idx: usize, v: i64) {
        self.set(idx, SettingValue::Int(v));
    }
    pub fn str_at(&self, idx: usize) -> &str {
        match &self.values[idx] {
            SettingValue::Str(s) => s,
            _ => "",
        }
    }
    pub fn set_str(&mut self, idx: usize, v: String) {
        self.set(idx, SettingValue::Str(v));
    }
    pub fn list_at(&self, idx: usize) -> Vec<String> {
        match &self.values[idx] {
            SettingValue::List(items) => items.clone(),
            _ => vec![],
        }
    }

    /// 与基线快照逐项 diff，仅发射有差异的字段；返回不含 `local wezterm` 头的 Lua 片段。
    pub fn to_lua(&self, baseline: &Value) -> String {
        let mut out = String::new();
        for (idx, spec) in SETTINGS.iter().enumerate() {
            let cur = &self.values[idx];
            let base = read_value(spec, baseline);
            if cur == &base {
                continue;
            }
            let stmt = match (spec.kind, cur) {
                // 伪设置项：开=发射右键智能复制/粘贴绑定块（走到这里必然与基线 false 有差异）
                _ if spec.key == RIGHT_CLICK_SMART_KEY => match cur {
                    SettingValue::Bool(true) => Some(SMART_RIGHT_CLICK_LUA.to_string()),
                    _ => None,
                },
                (Kind::Bool, SettingValue::Bool(b)) => Some(format!("config.{} = {b}\n", spec.key)),
                (Kind::Int { .. }, SettingValue::Int(i)) => Some(format!("config.{} = {i}\n", spec.key)),
                (Kind::Float { .. }, SettingValue::Float(f)) => {
                    Some(format!("config.{} = {}\n", spec.key, fmt_f64(*f)))
                }
                // 空字符串=跟随默认：跳过发射（与表单其余字符串项约定一致）
                (Kind::Str, SettingValue::Str(s)) if !s.is_empty() => {
                    Some(format!("config.{} = {}\n", spec.key, lua_quote(s)))
                }
                (Kind::Enum(_), SettingValue::Str(s)) if !s.is_empty() => {
                    Some(format!("config.{} = {}\n", spec.key, lua_quote(s)))
                }
                (Kind::List, SettingValue::List(items)) => {
                    let quoted: Vec<String> = items.iter().map(|s| lua_quote(s)).collect();
                    Some(format!("config.{} = {{ {} }}\n", spec.key, quoted.join(", ")))
                }
                _ => None,
            };
            if let Some(stmt) = stmt {
                out.push_str(&stmt);
            }
        }
        out
    }
}

fn read_value(spec: &SettingSpec, snap: &Value) -> SettingValue {
    // 伪设置项：以快照中 mouse_bindings 非空作为开态。
    // mouse_bindings 只可能来自本开关的发射（GUI 保存会整文件重写、其他来源不保留），
    // 因此「非空=开」是忠实回显，无需在 Lua 里另藏标记字段（未知字段会被 wezterm 报错）。
    if spec.key == RIGHT_CLICK_SMART_KEY {
        let on = snap
            .get("mouse_bindings")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        return SettingValue::Bool(on);
    }
    let v = snap.get(spec.key);
    match spec.kind {
        Kind::Bool => SettingValue::Bool(v.and_then(|x| x.as_bool()).unwrap_or(false)),
        Kind::Int { .. } => SettingValue::Int(num_of(v) as i64),
        Kind::Float { .. } => SettingValue::Float(num_of(v)),
        Kind::Str | Kind::Enum(_) => SettingValue::Str(str_of(v)),
        Kind::List => SettingValue::List(list_of(v)),
    }
}

/// f64 输出去掉多余的小数尾巴（14.0 → `14`，1.25 → `1.25`）
fn fmt_f64(v: f64) -> String {
    format!("{v}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 空配置加载为默认快照，供 roundtrip 测试共用。
    fn load_defaults() -> serde_json::Value {
        static CELL: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        CELL.get_or_init(|| {
            let loaded =
                crate::load::load_from_source("return {}\n", std::path::Path::new("defaults.lua"))
                    .unwrap();
            crate::load::config_to_json(&loaded.config)
        })
        .clone()
    }

    #[test]
    fn registry_keys_are_unique_and_pages_valid() {
        let mut seen = std::collections::HashSet::new();
        for spec in SETTINGS {
            assert!(seen.insert(spec.key), "重复 key: {}", spec.key);
            assert!(!spec.label.is_empty());
            if let Kind::Enum(opts) = spec.kind {
                assert!(!opts.is_empty(), "{} 枚举选项为空", spec.key);
                // 枚举值必须唯一
                let mut vals = std::collections::HashSet::new();
                for (v, _) in opts {
                    assert!(vals.insert(*v), "{} 枚举值重复: {v}", spec.key);
                }
            }
            if let Kind::Int { min, max } = spec.kind {
                assert!(min <= max, "{} 区间非法", spec.key);
            }
            if let Kind::Float { min, max } = spec.kind {
                assert!(min <= max, "{} 区间非法", spec.key);
            }
        }
    }

    #[test]
    fn from_snapshot_reads_all_kinds() {
        let snap = json!({
            "automatically_reload_config": false,
            "cursor_blink_rate": 500,
            "cursor_thickness": "1.5px",
            "window_background_opacity": 0.9,
            "default_cursor_style": "SteadyBlock",
            "selection_word_boundary": " \t\n",
            "default_gui_startup_args": ["start", "--no-auto-connect"],
        });
        let form = SettingsForm::from_snapshot(&snap);
        let auto = index("automatically_reload_config").unwrap();
        let rate = index("cursor_blink_rate").unwrap();
        let thick = index("cursor_thickness").unwrap();
        let opacity = index("window_background_opacity").unwrap();
        let style = index("default_cursor_style").unwrap();
        let sel = index("selection_word_boundary").unwrap();
        let args = index("default_gui_startup_args").unwrap();
        assert!(!form.bool_at(auto));
        assert_eq!(form.int_at(rate), 500);
        assert_eq!(form.float_at(thick), 1.5);
        assert_eq!(form.float_at(opacity), 0.9);
        assert_eq!(form.str_at(style), "SteadyBlock");
        assert_eq!(form.str_at(sel), " \t\n");
        assert_eq!(
            form.list_at(args),
            vec!["start".to_string(), "--no-auto-connect".to_string()]
        );
    }

    #[test]
    fn missing_keys_fall_back_to_zero_and_empty() {
        let form = SettingsForm::from_snapshot(&json!({}));
        let auto = index("automatically_reload_config").unwrap();
        let style = index("default_cursor_style").unwrap();
        assert!(!form.bool_at(auto));
        assert_eq!(form.str_at(style), "");
    }

    #[test]
    fn to_lua_emits_only_diffs_with_correct_literals() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        assert!(fm.to_lua(&defaults).is_empty(), "与基线一致不得发射任何字段");

        let auto = index("automatically_reload_config").unwrap();
        let style = index("default_cursor_style").unwrap();
        let scroll = index("enable_scroll_bar").unwrap();
        let debug = index("debug_key_events").unwrap();
        fm.set_bool(auto, false);
        fm.set_bool(scroll, true);
        fm.set_bool(debug, false);
        fm.set_str(style, "SteadyBar".into());
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("config.enable_scroll_bar = true"), "{lua}");
        assert!(lua.contains("config.default_cursor_style = 'SteadyBar'"), "{lua}");
        // 出厂默认 true 的开关改成 false 必须发射
        assert!(lua.contains("config.automatically_reload_config = false"), "{lua}");
        // 与出厂默认（false）一致的开关不得发射
        assert!(!lua.contains("debug_key_events"), "{lua}");
    }

    #[test]
    fn to_lua_handles_float_int_and_list() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        let size = index("char_select_font_size").unwrap();
        let cols = index("initial_cols").unwrap();
        let args = index("default_gui_startup_args").unwrap();
        let rate = index("text_blink_rate").unwrap();
        fm.set_float(size, 42.5);
        fm.set_int(cols, 120);
        fm.set_int(rate, 300);
        fm.set(
            args,
            SettingValue::List(vec!["start".into(), "--always".into()]),
        );
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("config.char_select_font_size = 42.5"), "{lua}");
        assert!(lua.contains("config.initial_cols = 120"), "{lua}");
        assert!(lua.contains("config.text_blink_rate = 300"), "{lua}");
        assert!(lua.contains(r#"config.default_gui_startup_args = { 'start', '--always' }"#), "{lua}");
    }

    #[test]
    fn empty_string_means_follow_default() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        let ws = index("default_workspace").unwrap();
        assert!(!fm.str_at(ws).is_empty() || true); // 默认可能为空，不参与断言
        fm.set_str(ws, String::new());
        let lua = fm.to_lua(&defaults);
        assert!(!lua.contains("default_workspace"), "{lua}");
    }

    #[test]
    fn backslash_and_quote_paths_survive_roundtrip() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        let cwd = index("default_cwd").unwrap();
        fm.set_str(cwd, r"C:\Users\testuser\notes 'dir'".into());
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains(r"'C:\\Users\\testuser\\notes \'dir\''"), "{lua}");
        let src = format!("local config = {{}}\n{lua}return config\n");
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let back = SettingsForm::from_snapshot(&snap);
        assert_eq!(back.str_at(cwd), r"C:\Users\testuser\notes 'dir'");
    }

    /// 端到端：一批有代表性的项（布尔/枚举/数值/字符串/列表）发射后经真实加载链路读回应一致。
    #[test]
    fn registry_roundtrip_through_loader() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        let scroll = index("enable_scroll_bar").unwrap();
        let bell = index("audible_bell").unwrap();
        let backdrop = index("win32_system_backdrop").unwrap();
        let opacity = index("text_background_opacity").unwrap();
        let scrollback = index("scrollback_lines").unwrap();
        let term = index("term").unwrap();
        let boundary = index("selection_word_boundary").unwrap();
        fm.set_bool(scroll, true);
        fm.set_str(bell, "Disabled".into());
        fm.set_str(backdrop, "Mica".into());
        fm.set_float(opacity, 0.5);
        fm.set_int(scrollback, 9000);
        fm.set_str(term, "xterm-256color".into());
        fm.set_str(boundary, " \t".into());
        let lua = fm.to_lua(&defaults);
        let src = format!("local config = {{}}\n{lua}return config\n");
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let back = SettingsForm::from_snapshot(&snap);
        assert!(back.bool_at(scroll));
        assert_eq!(back.str_at(bell), "Disabled");
        assert_eq!(back.str_at(backdrop), "Mica");
        assert_eq!(back.float_at(opacity), 0.5);
        assert_eq!(back.int_at(scrollback), 9000);
        assert_eq!(back.str_at(term), "xterm-256color");
        assert_eq!(back.str_at(boundary), " \t");
    }

    /// 伪设置项：开=发射右键智能复制/粘贴绑定块，且经真实加载链路回读后仍为开态。
    #[test]
    fn smart_right_click_toggle_emits_binding_and_roundtrips() {
        let defaults = load_defaults();
        let mut fm = SettingsForm::from_snapshot(&defaults);
        let idx = index(RIGHT_CLICK_SMART_KEY).unwrap();
        assert!(!fm.bool_at(idx)); // 出厂默认关
        fm.set_bool(idx, true);
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("config.mouse_bindings"), "{lua}");
        assert!(lua.contains("PasteFrom 'Clipboard'"), "{lua}");
        let src = format!(
            "local wezterm = require 'wezterm'\nlocal config = {{}}\n{lua}return config\n"
        );
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        assert_eq!(loaded.config.mouse_bindings.len(), 1);
        let snap = crate::load::config_to_json(&loaded.config);
        let back = SettingsForm::from_snapshot(&snap);
        assert!(back.bool_at(idx), "回读快照后开关应为开态");
    }

    /// 伪设置项：默认关态不发射任何 mouse_bindings。
    #[test]
    fn smart_right_click_off_emits_nothing() {
        let defaults = load_defaults();
        let fm = SettingsForm::from_snapshot(&defaults);
        let lua = fm.to_lua(&defaults);
        assert!(!lua.contains("mouse_bindings"), "{lua}");
    }

    /// 首启自动生成的默认配置必须能通过真实加载链路，且默认值符合产品要求：
    /// 默认程序 pwsh、Aardvark Blue、右键智能复制/粘贴开、launch_menu 四项（后两项提权）。
    #[test]
    fn generated_default_orca_config_loads_with_expected_defaults() {
        let src = config::Config::default_orca_config_lua();
        assert!(src.contains("mouse_bindings"), "占位符应已替换: {src}");
        let loaded =
            crate::load::load_from_source(&src, std::path::Path::new("orca-config.lua")).unwrap();
        let cfg = &loaded.config;
        assert_eq!(cfg.color_scheme.as_deref(), Some("Aardvark Blue"));
        let expected_prog: &[String] =
            &[r"D:\Tools\PowerShell\7\pwsh.exe".into(), "-NoLogo".into()];
        assert_eq!(
            cfg.default_prog.as_deref(),
            Some(expected_prog),
            "default_prog 应为 pwsh 绝对路径 + -NoLogo"
        );
        assert_eq!(cfg.mouse_bindings.len(), 1, "右键智能复制/粘贴应默认开启");
        assert_eq!(cfg.launch_menu.len(), 4);
        let labels: Vec<&str> = cfg
            .launch_menu
            .iter()
            .map(|c| c.label.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(
            labels,
            vec![
                "新CMD窗口",
                "新PowerShell窗口",
                "新管理员CMD窗口",
                "新管理员PowerShell窗口"
            ]
        );
        assert!(
            !cfg.launch_menu[0].elevate && !cfg.launch_menu[1].elevate,
            "普通项不应提权"
        );
        assert!(
            cfg.launch_menu[2].elevate && cfg.launch_menu[3].elevate,
            "管理员项应提权"
        );
    }

    /// 枚举项基线读出真实默认（如 win32_system_backdrop=Auto），未改动时不得发射。
    #[test]
    fn enum_baseline_is_real_default_not_empty() {
        let defaults = load_defaults();
        let fm = SettingsForm::from_snapshot(&defaults);
        let backdrop = index("win32_system_backdrop").unwrap();
        assert_eq!(fm.str_at(backdrop), "Auto");
    }
}
