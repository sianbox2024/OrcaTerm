//! 表单状态模型：强类型表单值 ↔ JSON 快照的双向桥梁。
//! 纯逻辑无 GPUI 依赖，全部可在主 workspace 测试。
//!
//! 顶层标量项已迁入 `settings` 注册表（SettingsForm）；本结构仅保留
//! 无法用「key = 字面量」表达的特殊项：字体族、配色三色、窗口内边距、默认程序、配色方案。

use std::path::Path;

use serde_json::Value;

use crate::settings::{lua_quote, SettingsForm};

/// 计算基线快照：剥离标记区后（用户自有配置）经真实读链路解析出的生效值，
/// 作为 `to_lua` 的 diff 基准——避免把用户已有设置误判为「默认」而重复发射，
/// 也避免用出厂默认钉死 scheme 解析出的调色板。无标记时整个文件即基线；
/// 标记区损坏按未迁移处理；加载失败退化为空对象。
pub fn baseline_snapshot(file_text: &str, display_path: &Path) -> Value {
    let text = match crate::markers::split(file_text) {
        Ok(Some(s)) => format!("{}{}", s.before, s.after),
        _ => file_text.to_string(),
    };
    match crate::load::load_from_source(&text, display_path) {
        Ok(loaded) => {
            let json = crate::load::config_to_json(&loaded.config);
            if json.is_null() {
                serde_json::json!({})
            } else {
                json
            }
        }
        Err(_) => serde_json::json!({}),
    }
}

/// 特殊结构项的强类型表单值。空字符串/None 表示「跟随默认」。
#[derive(Debug, Clone, PartialEq)]
pub struct FormState {
    /// 注册表驱动的顶层标量项
    pub scalars: SettingsForm,
    pub font_family: Option<String>,
    pub color_scheme: String,
    pub foreground: String,
    pub background: String,
    pub cursor_bg: String,
    pub padding_left: i64,
    pub padding_right: i64,
    pub padding_top: i64,
    pub padding_bottom: i64,
    pub default_prog: Option<Vec<String>>,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn i(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::String(st)) => st.trim_end_matches("px").trim().parse().unwrap_or(0.0) as i64,
        Some(x) => x.as_f64().unwrap_or(0.0) as i64,
        None => 0,
    }
}

/// 从快照的 `font` 字段解出首个字体族。兼容三种形状：
/// - wezterm 原生 TextStyle：`{"font": [{"family": ...}]}`（`Config.font` 经 ToDynamic 的实际形状）
/// - 字体属性数组：`[{"family": ...}]`
/// - 单个字体属性对象：`{"family": ...}`
fn font_family_of(v: &Value) -> Option<String> {
    let attrs = match v {
        Value::Array(items) => items.first(),
        obj @ Value::Object(_) => match obj.get("font") {
            Some(Value::Array(items)) => items.first(),
            Some(inner) => Some(inner),
            // FontAttributes 对象本身没有 "font" 键，直接取 family
            None => Some(obj),
        },
        _ => None,
    };
    attrs
        .and_then(|e| e.get("family"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

impl FormState {
    /// 从生效配置快照填充表单；`defaults` 为空配置快照（用于区分「用户设了」与「默认值」，本任务先只做取值）。
    pub fn from_snapshot(snap: &Value, _defaults: &Value) -> Self {
        let font_family = snap.get("font").and_then(font_family_of);
        let prog: Option<Vec<String>> = snap.get("default_prog").and_then(|p| {
            p.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        });
        Self {
            scalars: SettingsForm::from_snapshot(snap),
            font_family,
            color_scheme: s(snap, "color_scheme").to_string(),
            foreground: s(snap.get("colors").unwrap_or(&Value::Null), "foreground").to_string(),
            background: s(snap.get("colors").unwrap_or(&Value::Null), "background").to_string(),
            cursor_bg: s(
                snap.get("colors").unwrap_or(&Value::Null),
                "cursor_bg",
            )
            .to_string(),
            padding_left: i(snap.get("window_padding").unwrap_or(&Value::Null), "left"),
            padding_right: i(snap.get("window_padding").unwrap_or(&Value::Null), "right"),
            padding_top: i(snap.get("window_padding").unwrap_or(&Value::Null), "top"),
            padding_bottom: i(snap.get("window_padding").unwrap_or(&Value::Null), "bottom"),
            default_prog: prog,
        }
    }

    fn str_eq(d: &Value, key: &str, v: &str) -> bool {
        s(d, key) == v
    }

    /// 生成 Lua 文本（不含标记行）：先发射注册表标量项的 diff，再发射特殊结构项。
    pub fn to_lua(&self, d: &Value) -> String {
        let mut out = self.scalars.to_lua(d);

        if let Some(fam) = &self.font_family {
            let def_fam = d.get("font").and_then(font_family_of);
            if def_fam.as_deref() != Some(fam.as_str()) {
                out.push_str(&format!(
                    "local wezterm = require 'wezterm'\nconfig.font = wezterm.font({})\n",
                    lua_quote(fam)
                ));
            }
        }

        // 特殊字符串项（不进注册表）：换方案即清空派生色的语义在 GUI 层
        if !self.color_scheme.is_empty() && !Self::str_eq(d, "color_scheme", &self.color_scheme) {
            out.push_str(&format!(
                "config.color_scheme = {}\n",
                lua_quote(&self.color_scheme)
            ));
        }

        // 配色：colors 三项任一不同则整表发射
        let colors = d.get("colors").unwrap_or(&Value::Null);
        let colors_differ = self.foreground != s(colors, "foreground")
            || self.background != s(colors, "background")
            || self.cursor_bg != s(colors, "cursor_bg");
        if colors_differ {
            let mut parts = vec![];
            if !self.foreground.is_empty() {
                parts.push(format!("foreground = {}", lua_quote(&self.foreground)));
            }
            if !self.background.is_empty() {
                parts.push(format!("background = {}", lua_quote(&self.background)));
            }
            if !self.cursor_bg.is_empty() {
                parts.push(format!("cursor_bg = {}", lua_quote(&self.cursor_bg)));
            }
            if !parts.is_empty() {
                out.push_str(&format!("config.colors = {{ {} }}\n", parts.join(", ")));
            }
        }

        // 窗口内边距：仅发射与默认不同的子项（未发射子项由加载端按自身默认补全，
        // 避免把基于 cell 的默认值误写成 0px——快照里默认形如 "1cell"，解析为 0）
        let dpad = d.get("window_padding").unwrap_or(&Value::Null);
        {
            let mut parts = vec![];
            if self.padding_left != i(dpad, "left") {
                parts.push(format!("left = {}", self.padding_left));
            }
            if self.padding_right != i(dpad, "right") {
                parts.push(format!("right = {}", self.padding_right));
            }
            if self.padding_top != i(dpad, "top") {
                parts.push(format!("top = {}", self.padding_top));
            }
            if self.padding_bottom != i(dpad, "bottom") {
                parts.push(format!("bottom = {}", self.padding_bottom));
            }
            if !parts.is_empty() {
                out.push_str(&format!("config.window_padding = {{ {} }}\n", parts.join(", ")));
            }
        }

        // 启动程序
        if let Some(prog) = &self.default_prog {
            let def_prog: Option<Vec<String>> = d
                .get("default_prog")
                .and_then(|p| p.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
            if def_prog.as_deref() != Some(prog.as_slice()) {
                let items: Vec<String> = prog.iter().map(|p| lua_quote(p)).collect();
                out.push_str(&format!("config.default_prog = {{ {} }}\n", items.join(", ")));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 空配置加载为默认快照，供 to_lua 相关测试共用（加载开销较大，用 OnceLock 缓存）。
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
    fn from_snapshot_reads_specials() {
        let snap = json!({
            "font": {"font": [{"family": "JetBrains Mono"}]},
            "color_scheme": "Catppuccin Mocha",
            "colors": {"foreground": "#ffffff", "background": "#000000", "cursor_bg": "#ffcc00"},
            "window_padding": {"left": "8px", "right": "8px", "top": "4px", "bottom": "4px"},
            "default_prog": ["powershell.exe", "-NoLogo"],
        });
        let fm = FormState::from_snapshot(&snap, &Value::Null);
        assert_eq!(fm.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(fm.color_scheme, "Catppuccin Mocha");
        assert_eq!(fm.foreground, "#ffffff");
        assert_eq!(fm.padding_top, 4);
        assert_eq!(fm.padding_left, 8);
        assert_eq!(
            fm.default_prog,
            Some(vec!["powershell.exe".into(), "-NoLogo".into()])
        );
    }

    #[test]
    fn missing_keys_do_not_panic() {
        let fm = FormState::from_snapshot(&json!({}), &Value::Null);
        assert_eq!(fm.font_family, None);
        assert_eq!(fm.default_prog, None);
        assert_eq!(fm.color_scheme, "");
    }

    /// `Config.font` 经 ToDynamic 的真实形状是 TextStyle 包一层 font 数组，
    /// 而非测试上方手写的 `[{family}]`；形状不认会丢字体并导致整文件重写时静默删行。
    #[test]
    fn from_snapshot_reads_textstyle_font_shape() {
        let snap = json!({
            "font": {"font": [{"family": "MesloLGS NF"}], "foreground": null}
        });
        let fm = FormState::from_snapshot(&snap, &Value::Null);
        assert_eq!(fm.font_family.as_deref(), Some("MesloLGS NF"));
    }

    /// 基线同为 TextStyle 形状时，to_lua 不应把与默认一致的字体误发射。
    #[test]
    fn to_lua_diffs_font_against_textstyle_baseline() {
        let defaults = json!({"font": {"font": [{"family": "JetBrains Mono"}]}});
        let mut fm = FormState::from_snapshot(&defaults, &Value::Null);
        assert!(fm.to_lua(&defaults).is_empty(), "与基线一致不得发射 font");
        fm.font_family = Some("MesloLGS NF".into());
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("wezterm.font('MesloLGS NF')"), "{lua}");
    }

    #[test]
    fn to_lua_emits_only_non_default_fields() {
        let defaults = load_defaults();
        let mut fm = FormState::from_snapshot(&defaults, &defaults);
        fm.scalars.set(
            crate::settings::index("font_size").unwrap(),
            crate::settings::SettingValue::Float(16.0),
        );
        fm.scalars.set(
            crate::settings::index("enable_tab_bar").unwrap(),
            crate::settings::SettingValue::Bool(false),
        );
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("config.font_size = 16"), "{lua}");
        assert!(lua.contains("config.enable_tab_bar = false"), "{lua}");
        assert!(!lua.contains("scrollback_lines"), "{lua}");
        assert!(!lua.contains("color_scheme"), "{lua}");
    }

    #[test]
    fn to_lua_handles_strings_colors_and_lists() {
        let defaults = load_defaults();
        let mut fm = FormState::from_snapshot(&defaults, &defaults);
        fm.color_scheme = "Catppuccin Mocha".into();
        fm.foreground = "#ffffff".into();
        // Windows 平台默认即 TITLE|RESIZE，改用其他值确保触发发射
        fm.scalars.set(
            crate::settings::index("window_decorations").unwrap(),
            crate::settings::SettingValue::Str("RESIZE".into()),
        );
        fm.padding_left = 12;
        fm.default_prog = Some(vec!["powershell.exe".into(), "-NoLogo".into()]);
        let lua = fm.to_lua(&defaults);
        assert!(
            lua.contains("config.color_scheme = 'Catppuccin Mocha'"),
            "{lua}"
        );
        assert!(lua.contains("foreground = '#ffffff'"), "{lua}");
        assert!(lua.contains("config.window_decorations = 'RESIZE'"), "{lua}");
        assert!(lua.contains("left = 12"), "{lua}");
        assert!(
            lua.contains(r#"config.default_prog = { 'powershell.exe', '-NoLogo' }"#),
            "{lua}"
        );
    }

    #[test]
    fn emitted_lua_roundtrips_through_loader() {
        let defaults = load_defaults();
        let mut fm = FormState::from_snapshot(&defaults, &defaults);
        fm.scalars.set(
            crate::settings::index("cursor_blink_rate").unwrap(),
            crate::settings::SettingValue::Int(500),
        );
        fm.background = "#101010".into();
        fm.scalars.set(
            crate::settings::index("tab_bar_at_bottom").unwrap(),
            crate::settings::SettingValue::Bool(true),
        );
        fm.scalars.set(
            crate::settings::index("scrollback_lines").unwrap(),
            crate::settings::SettingValue::Int(9000),
        );
        // 默认 padding 为 cell 单位（解析为 0），12 必然有差异；验证部分表发射经真实加载仍生效
        fm.padding_left = 12;
        let lua = fm.to_lua(&defaults);
        let src = format!("local config = {{}}\n{lua}return config\n");
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let back = FormState::from_snapshot(&snap, &defaults);
        assert_eq!(back.scalars.int_at(crate::settings::index("cursor_blink_rate").unwrap()), 500);
        assert_eq!(back.background, "#101010");
        assert!(back.scalars.bool_at(crate::settings::index("tab_bar_at_bottom").unwrap()));
        assert_eq!(back.scalars.int_at(crate::settings::index("scrollback_lines").unwrap()), 9000);
        assert_eq!(back.padding_left, 12);
    }

    #[test]
    fn padding_partial_emission_omits_untouched_sides() {
        let defaults = load_defaults();
        let mut fm = FormState::from_snapshot(&defaults, &defaults);
        fm.padding_left = 12;
        let lua = fm.to_lua(&defaults);
        assert!(lua.contains("left = 12"), "{lua}");
        assert!(!lua.contains("right ="), "{lua}");
        assert!(!lua.contains("top ="), "{lua}");
        assert!(!lua.contains("bottom ="), "{lua}");
    }

    #[test]
    fn backslash_paths_survive_roundtrip() {
        let defaults = load_defaults();
        let mut fm = FormState::from_snapshot(&defaults, &defaults);
        fm.scalars.set(
            crate::settings::index("default_cwd").unwrap(),
            crate::settings::SettingValue::Str(r"C:\Users\testuser\notes dir".into()),
        );
        let lua = fm.to_lua(&defaults);
        // 反斜杠必须被翻倍，否则 \U、\n 在 Lua 里是非法/错误转义
        assert!(
            lua.contains(r"'C:\\Users\\testuser\\notes dir'"),
            "{lua}"
        );
        let src = format!("local config = {{}}\n{lua}return config\n");
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let back = FormState::from_snapshot(&snap, &defaults);
        assert_eq!(
            back.scalars.str_at(crate::settings::index("default_cwd").unwrap()),
            r"C:\Users\testuser\notes dir"
        );
    }

    #[test]
    fn baseline_snapshot_resolves_out_of_marker_values() {
        let text = concat!(
            "local config = {}\n",
            "-- <orca-gui-config-start>\n",
            "config.font_size = 16\n",
            "-- <orca-gui-config-end>\n",
            "config.font_size = 14\n",
            "return config\n",
        );
        let base = baseline_snapshot(text, std::path::Path::new("t.lua"));
        assert_eq!(base.get("font_size").and_then(|v| v.as_f64()), Some(14.0), "标记区内设置不得进入基线");
    }

    #[test]
    fn baseline_snapshot_degrades_to_empty_on_load_error() {
        let base = baseline_snapshot("local c = )\n", std::path::Path::new("t.lua"));
        assert_eq!(base, serde_json::json!({}));
    }

    #[test]
    fn scheme_switch_does_not_pin_reference_palette() {
        let reference = serde_json::json!({
            "color_scheme": "A",
            "colors": {
                "foreground": "#aaaaaa",
                "background": "#111111",
                "cursor_bg": "#222222"
            }
        });
        let mut fm = FormState::from_snapshot(&reference, &Value::Null);
        fm.color_scheme = "B".into();
        let lua = fm.to_lua(&reference);
        assert!(lua.contains("config.color_scheme = 'B'"), "{lua}");
        assert!(!lua.contains("colors"), "{lua}");
    }
}
