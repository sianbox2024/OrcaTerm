//! 标签栏「发送指令」按钮（U+E6E4）弹层的指令列表。结构化列表数据不走
//! settings 注册表（与 ssh_domains 同一取舍——注册表只管标量）：读链路
//! 从配置快照 JSON 解析，写链路保存时专项发射 `config.tab_button_commands`，
//! 整文件重写语义下「清空列表保存 = 从配置文件移除该键」。

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct TabButtonCommand {
    /// 列表显示文本；空 = GUI 弹层回退显示 command
    pub label: String,
    /// 选中后向当前终端发送的指令文本（GUI 自动补回车执行）
    pub command: String,
}

/// 从生效配置快照解析用户指令列表；缺键/非数组/元素缺字段或 null
/// 一律容忍为空串（ToDynamic 会把空 Option 序列化为 null）。
/// 若新列表为空但存在旧版非空 `tab_button_command` 标量字符串，自动兼容包装为单条条目。
pub fn parse_tab_button_commands(snap: &Value) -> Vec<TabButtonCommand> {
    let mut cmds: Vec<TabButtonCommand> = snap
        .get("tab_button_commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    Some(TabButtonCommand {
                        label: obj
                            .get("label")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        command: obj
                            .get("command")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    if cmds.is_empty() {
        if let Some(cmd_str) = snap.get("tab_button_command").and_then(|x| x.as_str()) {
            let trimmed = cmd_str.trim();
            if !trimmed.is_empty() {
                cmds.push(TabButtonCommand {
                    label: String::new(),
                    command: trimmed.to_string(),
                });
            }
        }
    }

    cmds
}

/// 发射 `config.tab_button_commands = { { label=.., command=.. }, .. }`；
/// 空列表返回空串（由调用方拼入保存正文，整文件重写后该键即被移除，
/// 与 emit_ssh_domains 同语义）。
pub fn emit_tab_button_commands(cmds: &[TabButtonCommand]) -> String {
    if cmds.is_empty() {
        return String::new();
    }
    let mut out = String::from("config.tab_button_commands = {\n");
    for c in cmds {
        out.push_str(&format!(
            "  {{ label={}, command={} }},\n",
            crate::settings::lua_quote(c.label.trim()),
            crate::settings::lua_quote(c.command.trim())
        ));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn parse_tolerates_missing_and_null_fields() {
        let snap = json!({
            "tab_button_commands": [
                {"label": "清理", "command": "clear"},
                {"label": null, "command": "htop"},
                {"command": "ls -la"},
                {"label": "broken"}
            ]
        });
        let cmds = parse_tab_button_commands(&snap);
        assert_eq!(cmds.len(), 4);
        assert_eq!(
            cmds[0],
            TabButtonCommand {
                label: "清理".into(),
                command: "clear".into()
            }
        );
        assert_eq!(cmds[1].label, "");
        assert_eq!(cmds[1].command, "htop");
        assert_eq!(cmds[2].label, "");
        assert_eq!(cmds[2].command, "ls -la");
        assert_eq!(cmds[3].command, "");
    }

    #[test]
    fn parse_missing_key_and_non_array_are_empty() {
        assert!(parse_tab_button_commands(&json!({})).is_empty());
        assert!(parse_tab_button_commands(&json!({"tab_button_commands": "x"})).is_empty());
    }

    #[test]
    fn parse_migrates_legacy_single_command_when_commands_empty() {
        let snap = json!({
            "tab_button_command": "micro ~/桌面/1.txt"
        });
        let cmds = parse_tab_button_commands(&snap);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].label, "");
        assert_eq!(cmds[0].command, "micro ~/桌面/1.txt");

        // 若新键非空，优先以新键为准
        let snap_both = json!({
            "tab_button_command": "old_cmd",
            "tab_button_commands": [
                {"label": "new", "command": "new_cmd"}
            ]
        });
        let cmds_both = parse_tab_button_commands(&snap_both);
        assert_eq!(cmds_both.len(), 1);
        assert_eq!(cmds_both[0].label, "new");
        assert_eq!(cmds_both[0].command, "new_cmd");
    }

    /// 验证含有旧版 `config.tab_button_command` 的旧配置可经真实加载链无错加载
    #[test]
    fn legacy_config_loads_without_error() {
        let source = "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\nconfig.tab_button_command = 'micro ~/桌面/1.txt'\nreturn config\n";
        let loaded = crate::load::load_from_source(source, Path::new("legacy-test.lua")).expect("旧配置应顺利加载");
        let snap = crate::load::config_to_json(&loaded.config);
        let parsed = parse_tab_button_commands(&snap);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].command, "micro ~/桌面/1.txt");
    }

    /// 发射结果必须能被真实加载链（mlua + FromDynamic）原样读回：
    /// 覆盖中文、单引号与反斜杠转义、空 label（发射后仍为空串）。
    #[test]
    fn emit_roundtrips_through_real_load_chain() {
        let cmds = vec![
            TabButtonCommand {
                label: "清理屏幕".into(),
                command: "clear".into(),
            },
            TabButtonCommand {
                label: "quote'test".into(),
                command: "echo 'a\\b'".into(),
            },
            TabButtonCommand {
                label: "".into(),
                command: "htop".into(),
            },
        ];
        let body = emit_tab_button_commands(&cmds);
        let source = format!(
            "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\n{}return config\n",
            body
        );
        let loaded = crate::load::load_from_source(&source, Path::new("tabcmd-test.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        let parsed = parse_tab_button_commands(&snap);
        assert_eq!(parsed, cmds);
    }

    #[test]
    fn emit_empty_list_is_empty_string() {
        assert_eq!(emit_tab_button_commands(&[]), "");
    }
}
