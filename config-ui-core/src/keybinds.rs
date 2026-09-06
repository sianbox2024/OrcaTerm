//! 键绑定模型：快照 `keys` ↔ Binding 列表、Lua 发射、冲突检测。
//! 默认键位表为 Windows 常用子集（只读展示用），完整权威定义在
//! wezterm-gui inputmap.rs / commands.rs，此处不重复全量。

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub mods: String,
    pub key: String,
    /// KeyAssignment 变体名，如 CopyTo / ReloadConfiguration
    pub action_name: String,
    /// 变体参数的字符串形式；无参为空。复杂参数（表）存原始 JSON 文本，GUI 只读。
    pub action_arg: String,
}

/// 修饰键规范化：大写、去空、"SHIFT|CTRL" 与 "CTRL+Shift" 归一。
pub fn norm_mods(m: &str) -> String {
    let mut parts: Vec<&str> = m
        .split(['|', '+'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    parts.sort_unstable();
    parts.join("|").to_uppercase()
}

impl Binding {
    pub fn combo(&self) -> (String, String) {
        (norm_mods(&self.mods), self.key.to_lowercase())
    }
}

/// 从生效配置快照解析用户键绑定。
pub fn parse_bindings(snap: &Value) -> Vec<Binding> {
    let mut out = vec![];
    for k in snap
        .get("keys")
        .and_then(|k| k.as_array())
        .into_iter()
        .flatten()
    {
        // action 序列化为 {VariantName: arg}；无参变体可能是纯字符串
        let action = k.get("action").unwrap_or(&Value::Null);
        let (name, arg) = match action {
            Value::Object(map) if map.len() == 1 => {
                let (n, v) = map.iter().next().unwrap();
                let arg = match v {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    other => other.to_string(),
                };
                (n.clone(), arg)
            }
            Value::String(s) => (s.clone(), String::new()),
            _ => ("UNKNOWN".into(), String::new()),
        };
        out.push(Binding {
            mods: k.get("mods").and_then(|m| m.as_str()).unwrap_or("").to_string(),
            key: k.get("key").and_then(|m| m.as_str()).unwrap_or("").to_string(),
            action_name: name,
            action_arg: arg,
        });
    }
    out
}

/// Lua 单引号字面量（与 form::quote 同规则）
fn quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn action_lua(name: &str, arg: &str) -> String {
    if arg.is_empty() {
        format!("wezterm.action.{name}")
    } else if arg.parse::<f64>().is_ok() {
        format!("wezterm.action.{name}({arg})")
    } else {
        format!("wezterm.action.{name}({})", quote(arg))
    }
}

/// 发射整组 config.keys 赋值；列表为空返回空串。
pub fn emit_bindings(bindings: &[Binding]) -> String {
    if bindings.is_empty() {
        return String::new();
    }
    let items: Vec<String> = bindings
        .iter()
        .map(|b| {
            format!(
                "{{ key={}, mods={}, action={} }}",
                quote(&b.key),
                quote(&b.mods),
                action_lua(&b.action_name, &b.action_arg)
            )
        })
        .collect();
    format!(
        "local wezterm = require 'wezterm'\nconfig.keys = {{ {} }}\n",
        items.join(", ")
    )
}

/// 用户绑定内部两两冲突（同组合键出现多次）。返回重复的组合键集合。
pub fn find_conflicts(bindings: &[Binding]) -> Vec<(String, String)> {
    use std::collections::HashMap;
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut dupes = Vec::new();
    for b in bindings {
        let c = b.combo();
        match seen.get(&c) {
            Some(_) => {
                if !dupes.contains(&c) {
                    dupes.push(c);
                }
            }
            None => {
                seen.insert(c, 1);
            }
        }
    }
    dupes
}

/// Windows/Linux 常用默认键位（curated subset——完整默认表由 wezterm-gui
/// 在运行时按 CommandDef 展开，GUI 只读展示常用项）。
pub const DEFAULT_BINDINGS: &[(&str, &str, &str)] = &[
    ("CTRL|SHIFT", "c", "CopyTo 'Clipboard'"),
    ("CTRL|INSERT", "Insert", "CopyTo 'PrimarySelection'"),
    ("SHIFT|INSERT", "Insert", "PasteFrom 'PrimarySelection'"),
    ("CTRL|SHIFT", "v", "PasteFrom 'Clipboard'"),
    ("CTRL|SHIFT", "t", "SpawnTab"),
    ("CTRL|SHIFT", "n", "SpawnWindow"),
    ("CTRL|SHIFT", "w", "CloseCurrentTab"),
    ("CTRL|SHIFT", "r", "ReloadConfiguration"),
    ("CTRL|SHIFT", "l", "ShowDebugOverlay"),
    ("CTRL|SHIFT", "p", "ActivateCommandPalette"),
    ("CTRL|SHIFT", "x", "ActivateCopyMode"),
    ("CTRL|SHIFT", "u", "CharSelect"),
    ("CTRL|SHIFT", "Space", "QuickSelect"),
    ("CTRL|SHIFT", "f", "Search"),
    ("CTRL|SHIFT", "k", "ClearScrollback"),
    ("CTRL|SHIFT", "z", "TogglePaneZoomState"),
    ("ALT|ENTER", "Enter", "ToggleFullScreen"),
    ("CTRL", "-", "DecreaseFontSize"),
    ("CTRL", "=", "IncreaseFontSize"),
    ("CTRL", "0", "ResetFontSize"),
    ("CTRL", "PageUp", "ActivateTabRelative(-1)"),
    ("CTRL", "PageDown", "ActivateTabRelative(1)"),
    ("CTRL|SHIFT", "PageUp", "MoveTabRelative(-1)"),
    ("CTRL|SHIFT", "PageDown", "MoveTabRelative(1)"),
    ("SHIFT", "PageUp", "ScrollByPage(-1)"),
    ("SHIFT", "PageDown", "ScrollByPage(1)"),
    ("CTRL|SHIFT", "LeftArrow", "ActivatePaneDirection 'Left'"),
    ("CTRL|SHIFT", "RightArrow", "ActivatePaneDirection 'Right'"),
    ("CTRL|SHIFT", "UpArrow", "ActivatePaneDirection 'Up'"),
    ("CTRL|SHIFT", "DownArrow", "ActivatePaneDirection 'Down'"),
];

/// 某组合键是否命中默认键位（用于 GUI 冲突高亮）。
pub fn hits_default(mods: &str, key: &str) -> bool {
    let m = norm_mods(mods);
    let k = key.to_lowercase();
    DEFAULT_BINDINGS
        .iter()
        .any(|(dm, dk, _)| norm_mods(dm) == m && *dk == k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_string_number_and_complex_args() {
        let snap = json!({"keys": [
            {"key": "c", "mods": "SHIFT|CTRL", "action": {"CopyTo": "ClipboardAndPrimarySelection"}},
            {"key": "1", "mods": "ALT", "action": {"ActivateTab": 0}},
            {"key": "r", "mods": "CTRL|SHIFT", "action": "ReloadConfiguration"},
            {"key": "q", "mods": "LEADER", "action": {"QuickSelectArgs": {"alphabet": "abc"}}}
        ]});
        let b = parse_bindings(&snap);
        assert_eq!(b.len(), 4);
        assert_eq!(b[0].action_name, "CopyTo");
        assert_eq!(b[0].action_arg, "ClipboardAndPrimarySelection");
        assert_eq!(b[0].mods, "SHIFT|CTRL");
        assert_eq!(b[1].action_arg, "0");
        assert_eq!(b[2].action_name, "ReloadConfiguration");
        assert_eq!(b[2].action_arg, "");
        assert_eq!(b[3].action_name, "QuickSelectArgs");
        assert_eq!(b[3].action_arg, r#"{"alphabet":"abc"}"#);
    }

    #[test]
    fn missing_keys_yields_empty() {
        assert!(parse_bindings(&json!({})).is_empty());
        assert!(parse_bindings(&json!({"keys": null})).is_empty());
    }

    #[test]
    fn emission_roundtrips_through_loader() {
        let bindings = vec![
            Binding { mods: "CTRL|SHIFT".into(), key: "c".into(), action_name: "CopyTo".into(), action_arg: "Clipboard".into() },
            Binding { mods: "ALT".into(), key: "1".into(), action_name: "ActivateTab".into(), action_arg: "0".into() },
            Binding { mods: "CTRL".into(), key: "r".into(), action_name: "ReloadConfiguration".into(), action_arg: "".into() },
        ];
        let lua = emit_bindings(&bindings);
        assert!(lua.contains("config.keys"));
        // 构造完整脚本走真实读链路再解析回来
        let src = format!("local config = {{}}\n{lua}return config\n");
        let loaded = crate::load::load_from_source(&src, std::path::Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let back = parse_bindings(&snap);
        assert_eq!(back.len(), 3);
        // 加载端会把 mods 重排为规范顺序（如 CTRL|SHIFT → SHIFT|CTRL），按组合键语义比较
        for (a, b) in back.iter().zip(bindings.iter()) {
            assert_eq!(a.combo(), b.combo());
            assert_eq!(a.action_name, b.action_name);
            assert_eq!(a.action_arg, b.action_arg);
        }
    }

    #[test]
    fn empty_list_emits_nothing() {
        assert_eq!(emit_bindings(&[]), "");
    }

    #[test]
    fn detects_duplicates_and_normalizes_mods() {
        let bs = vec![
            Binding { mods: "SHIFT|CTRL".into(), key: "C".into(), action_name: "CopyTo".into(), action_arg: "Clipboard".into() },
            Binding { mods: "CTRL|SHIFT".into(), key: "c".into(), action_name: "PasteFrom".into(), action_arg: "Clipboard".into() },
            Binding { mods: "ALT".into(), key: "x".into(), action_name: "ReloadConfiguration".into(), action_arg: "".into() },
        ];
        let dupes = find_conflicts(&bs);
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0], (norm_mods("CTRL|SHIFT"), "c".to_string()));
        assert_eq!(norm_mods("ctrl + shift"), "CTRL|SHIFT");
    }

    #[test]
    fn default_table_hits_expected_combos() {
        assert!(hits_default("CTRL|SHIFT", "c"));
        assert!(!hits_default("CTRL|SHIFT", "y"));
        assert!(!hits_default("ALT|SHIFT", "c"));
    }
}
