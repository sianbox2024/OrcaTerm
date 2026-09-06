//! 标记区识别与切分：`<orca-gui-config-start>` / `<orca-gui-config-end>` 之间的内容由 GUI 管理，
//! 区外内容逐字保留。

/// 标记行（按整行 trim 后精确匹配识别）
pub const MARKER_START: &str = "-- <orca-gui-config-start>";
pub const MARKER_END: &str = "-- <orca-gui-config-end>";

#[derive(Debug, PartialEq, Eq)]
pub enum SplitError {
    /// 出现起始标记但找不到结束标记
    UnclosedMarker,
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitError::UnclosedMarker => write!(
                f,
                "找到 {MARKER_START} 但缺少对应的 {MARKER_END}，请补全标记或手动删除起始标记"
            ),
        }
    }
}

impl std::error::Error for SplitError {}

/// 切分结果。`gui` 为标记区内部内容（不含标记行本身）。
#[derive(Debug, PartialEq, Eq)]
pub struct Sections {
    pub before: String,
    pub gui: String,
    pub after: String,
}

/// 按标记切分文本。无标记时返回 `Ok(None)`（整个文件视为自定义区）。
pub fn split(text: &str) -> Result<Option<Sections>, SplitError> {
    // split_inclusive 保留行尾换行符，拼接即可逐字节还原原文
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let start = match lines.iter().position(|l| l.trim() == MARKER_START) {
        Some(i) => i,
        None => return Ok(None),
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim() == MARKER_END)
        .map(|off| start + 1 + off);
    match end {
        Some(end) => Ok(Some(Sections {
            before: lines[..start].concat(),
            gui: lines[start + 1..end].concat(),
            after: lines[end + 1..].concat(),
        })),
        None => Err(SplitError::UnclosedMarker),
    }
}

/// 用 `gui_block`（不含标记行的 Lua 内容）重生成标记区并替换原有标记区；
/// 原文无标记时追加到文件末尾。区外内容逐字节保留。
pub fn regenerate(text: &str, gui_block: &str) -> Result<String, SplitError> {
    let sections = split(text)?;
    let mut out = String::with_capacity(text.len() + gui_block.len() + MARKER_START.len() * 2 + 4);
    match &sections {
        Some(s) => out.push_str(&s.before),
        None => {
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out.push_str(MARKER_START);
    out.push('\n');
    if !gui_block.is_empty() {
        out.push_str(gui_block);
        if !gui_block.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(MARKER_END);
    out.push('\n');
    if let Some(s) = &sections {
        out.push_str(&s.after);
    }
    Ok(out)
}

/// 文件是否已含 GUI 管理标记区。
pub fn has_markers(text: &str) -> bool {
    matches!(split(text), Ok(Some(_)))
}

/// 引导式迁移：把 `gui_block` 以标记区形式插入到**顶层 return 之前**（保证赋值在
/// 返回前生效）。找不到顶层 return 时退化为追加到文件尾。已含标记时原样返回。
///
/// ponytail: 用"无缩进的 return 行"近似识别顶层 return，[[ ]] 长字符串内出现
/// 顶格 return 会误判——遇到再引入真正的 Lua 解析。
pub fn enable_gui_management(text: &str, gui_block: &str) -> Result<String, SplitError> {
    if has_markers(text) {
        return Ok(text.to_string());
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let insert_at = lines
        .iter()
        .position(|line| line.trim_end().starts_with("return "));
    match insert_at {
        Some(idx) => {
            if !declares_config_table(text) {
                // 文件没有 config 表（如 `return {}` / `return c`）：
                // 把顶层 return 改写为 `local config = <expr>`，标记区赋值才能生效
                return Ok(migrate_return_into_config(&lines, idx, gui_block));
            }
            let mut out = String::with_capacity(text.len() + gui_block.len() + 64);
            out.push_str(&lines[..idx].concat());
            out.push_str(MARKER_START);
            out.push('\n');
            if !gui_block.is_empty() {
                out.push_str(gui_block);
                if !gui_block.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push_str(MARKER_END);
            out.push('\n');
            out.push_str(&lines[idx..].concat());
            Ok(out)
        }
        None => regenerate(text, gui_block),
    }
}

/// 文件是否声明了名为 config 的配置表（`local config =` 或 `config =` 开头的行）。
fn declares_config_table(text: &str) -> bool {
    text.split_inclusive('\n').any(|line| {
        let t = line.trim_start();
        t.starts_with("local config") || t.starts_with("config =")
    })
}

/// 把顶层 `return <expr>` 改写为 `local config = <expr>` + 标记区 + `return config`。
/// Lua 中 return 必须是块内最后一条语句，其后只可能是注释/空白，故尾部内容原样保留是安全的。
fn migrate_return_into_config(lines: &[&str], idx: usize, gui_block: &str) -> String {
    let ret = lines[idx].trim_end().trim_end_matches(';').trim_end();
    let expr = ret["return ".len()..].trim();
    let mut out = String::with_capacity(lines.iter().map(|l| l.len()).sum::<usize>() + gui_block.len() + 64);
    out.push_str(&lines[..idx].concat());
    out.push_str(&format!("local config = {expr}\n"));
    out.push_str(MARKER_START);
    out.push('\n');
    if !gui_block.is_empty() {
        out.push_str(gui_block);
        if !gui_block.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(MARKER_END);
    out.push('\n');
    out.push_str("return config\n");
    // 原顶层 return 之后只剩注释/空白（Lua 语法保证），逐字保留
    out.push_str(&lines[idx + 1..].concat());
    out
}

/// 区外内容是否缺少顶层 `return`，需要脚手架（`local config = {}` … `return config`）才能成为合法 Lua。
pub fn needs_config_scaffold(outside_text: &str) -> bool {
    !outside_text
        .split_inclusive('\n')
        .any(|line| line.trim_end().starts_with("return "))
}

/// 把 GUI 块包进可独立加载的 Lua 片段：建表 + 赋值 + 返回。
pub fn scaffold_gui_block(gui_block: &str) -> String {
    let mut block = gui_block.to_string();
    if !block.is_empty() && !block.ends_with('\n') {
        block.push('\n');
    }
    format!("local config = {{}}\n{block}return config\n")
}

/// 生成保存用的完整文件内容：文件缺表或缺顶层 return 时自动补脚手架，
/// 避免保存出"只有赋值、加载即报 index nil"的坏配置。
pub fn build_marked_file(text: &str, gui_block: &str) -> Result<String, SplitError> {
    let outside = match split(text)? {
        Some(s) => format!("{}{}", s.before, s.after),
        None => text.to_string(),
    };
    let block = if needs_config_scaffold(&outside) {
        scaffold_gui_block(gui_block)
    } else {
        gui_block.to_string()
    };
    if has_markers(text) {
        regenerate(text, &block)
    } else {
        enable_gui_management(text, &block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// `return {}` 型文件迁移：必须改写成 `local config = {}` + 标记区 + `return config`，
    /// 否则标记区赋值落到全局 config 而丢失。
    #[test]
    fn bare_return_table_migrates_to_config_var() {
        let out = build_marked_file("return {}\n", "config.font_size = 14\n").unwrap();
        assert!(out.contains("local config = {}\n"), "{out}");
        assert!(out.contains("return config\n"), "{out}");
        let loaded = crate::load::load_from_source(&out, Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        assert_eq!(crate::form::FormState::from_snapshot(&snap, &serde_json::Value::Null).scalars.float_at(crate::settings::index("font_size").unwrap()), 14.0);
    }

    /// 变量名非 config 的文件（如 `return c`）同样迁移：config 别名到原表，原内容不丢。
    #[test]
    fn named_var_return_migrates_and_preserves_content() {
        let src = "local c = { font_size = 15 }\nlocal helper = function() return 42 end\nreturn c\n";
        let out = build_marked_file(src, "config.font_size = 14\n").unwrap();
        assert!(out.contains("local config = c\n"), "{out}");
        assert!(out.contains("local helper = function() return 42 end\n"), "{out}");
        let loaded = crate::load::load_from_source(&out, Path::new("t.lua")).unwrap();
        let snap = crate::load::config_to_json(&loaded.config);
        assert_eq!(crate::form::FormState::from_snapshot(&snap, &serde_json::Value::Null).scalars.float_at(crate::settings::index("font_size").unwrap()), 14.0);
    }

    /// 已声明 config 表的文件走既有路径，不受迁移逻辑影响。
    #[test]
    fn declared_config_file_unchanged_path() {
        let src = "local config = {}\nconfig.font_size = 12\nreturn config\n";
        let out = build_marked_file(src, "config.font_size = 14\n").unwrap();
        assert!(!out.contains("local config = {}\n-- <"), "不应重复建表: {out}");
    }

    #[test]
    fn split_no_markers_returns_none() {
        let text = "local config = {}\nreturn config\n";
        assert_eq!(split(text).unwrap(), None);
    }

    #[test]
    fn split_returns_sections() {
        let text = concat!(
            "local config = {}\n",
            "-- <orca-gui-config-start>\n",
            "config.font_size = 14\n",
            "-- <orca-gui-config-end>\n",
            "return config\n",
        );
        let s = split(text).unwrap().unwrap();
        assert_eq!(s.before, "local config = {}\n");
        assert_eq!(s.gui, "config.font_size = 14\n");
        assert_eq!(s.after, "return config\n");
    }

    #[test]
    fn split_unclosed_start_is_error() {
        let text = "-- <orca-gui-config-start>\nconfig.font_size = 14\n";
        assert_eq!(split(text), Err(SplitError::UnclosedMarker));
    }

    #[test]
    fn regenerate_preserves_outside_verbatim() {
        // 区外含注释、空行、行尾空格，必须逐字节保留
        let text = concat!(
            "-- my header  \n",
            "\n",
            "local wezterm = require 'wezterm'\n",
            "local config = {}\n",
            "-- <orca-gui-config-start>\n",
            "config.old = true\n",
            "-- <orca-gui-config-end>\n",
            "\n",
            "-- trailing comment\n",
            "return config\n",
        );
        let out = regenerate(text, "config.font_size = 12\n").unwrap();
        assert!(out.starts_with(
            "-- my header  \n\nlocal wezterm = require 'wezterm'\nlocal config = {}\n"
        ));
        assert!(out.ends_with("\n\n-- trailing comment\nreturn config\n"));
        assert!(out.contains(
            "-- <orca-gui-config-start>\nconfig.font_size = 12\n-- <orca-gui-config-end>\n"
        ));
        assert!(!out.contains("config.old"));
    }

    #[test]
    fn regenerate_appends_when_no_markers() {
        let text = "local config = {}\nreturn config\n";
        let out = regenerate(text, "config.font_size = 12\n").unwrap();
        assert_eq!(
            out,
            concat!(
                "local config = {}\n",
                "return config\n",
                "-- <orca-gui-config-start>\n",
                "config.font_size = 12\n",
                "-- <orca-gui-config-end>\n",
            )
        );
    }

    #[test]
    fn regenerate_is_idempotent() {
        let text = "local config = {}\n-- <orca-gui-config-start>\nconfig.a = 1\n-- <orca-gui-config-end>\nreturn config\n";
        let once = regenerate(text, "config.b = 2\n").unwrap();
        let twice = regenerate(&once, "config.b = 2\n").unwrap();
        assert_eq!(once, twice);
        assert!(once.contains("config.b = 2"));
        assert!(!once.contains("config.a"));
    }

    #[test]
    fn regenerate_empty_block_keeps_valid_markers() {
        let text = "x\n-- <orca-gui-config-start>\nold\n-- <orca-gui-config-end>\ny\n";
        let out = regenerate(text, "").unwrap();
        assert_eq!(
            out,
            "x\n-- <orca-gui-config-start>\n-- <orca-gui-config-end>\ny\n"
        );
    }

    #[test]
    fn has_markers_detects_presence() {
        assert!(!has_markers("local c = {}\nreturn c\n"));
        assert!(has_markers(
            "-- <orca-gui-config-start>\n-- <orca-gui-config-end>\n"
        ));
    }

    #[test]
    fn enable_inserts_before_top_level_return() {
        let text = "local config = {}\nconfig.font_size = 10\nreturn config\n";
        let out = enable_gui_management(text, "config.font_size = 12\n").unwrap();
        let marker_pos = out.find(MARKER_START).unwrap();
        let return_pos = out.find("return config").unwrap();
        assert!(
            marker_pos < return_pos,
            "标记区必须在顶层 return 之前:\n{out}"
        );
        assert!(out.contains("config.font_size = 12"));
    }

    #[test]
    fn enable_ignores_indented_returns() {
        // 函数体内的 return 不是顶层 return，不得误判
        let text = concat!(
            "local function helper(x)\n",
            "  if x then return nil end\n",
            "end\n",
            "local config = {}\n",
            "return config\n",
        );
        let out = enable_gui_management(text, "").unwrap();
        let marker_pos = out.find(MARKER_START).unwrap();
        let top_return = out.rfind("return config").unwrap();
        let nested = out.find("return nil").unwrap();
        assert!(nested < marker_pos);
        assert!(marker_pos < top_return);
    }

    #[test]
    fn enable_appends_when_no_top_level_return() {
        let text = "print('side effect only')\n";
        let out = enable_gui_management(text, "config.a = 1\n").unwrap();
        assert!(out.starts_with("print('side effect only')\n"));
        assert!(out.contains(MARKER_START));
    }

    #[test]
    fn enable_is_noop_when_already_marked() {
        let text = "local c = {}\n-- <orca-gui-config-start>\nc.a = 1\n-- <orca-gui-config-end>\nreturn c\n";
        assert_eq!(enable_gui_management(text, "c.b = 2\n").unwrap(), text);
    }

    #[test]
    fn enabled_config_actually_takes_effect() {
        // 集成验证：迁移后的文件经读链路加载，GUI 值必须生效（后赋值覆盖先赋值）
        let text = "local config = {}\nconfig.font_size = 10\nreturn config\n";
        let migrated = enable_gui_management(text, "config.font_size = 12.0\n").unwrap();
        let loaded = crate::load::load_from_source(&migrated, Path::new("m.lua")).unwrap();
        assert_eq!(loaded.config.font_size, 12.0);
    }

    #[test]
    fn build_scaffolds_empty_file_and_output_loads() {
        let out = build_marked_file("", "config.font_size = 13.0\n").unwrap();
        let loaded = crate::load::load_from_source(&out, Path::new("m.lua")).unwrap();
        assert_eq!(loaded.config.font_size, 13.0);
    }

    #[test]
    fn build_repairs_bare_marker_block() {
        // 用户磁盘上的坏模式：只有标记区内的裸赋值，没有建表也没有 return
        let text = concat!(
            "-- <orca-gui-config-start>\n",
            "config.font_size = 13.0\n",
            "-- <orca-gui-config-end>\n",
        );
        let out = build_marked_file(text, "config.font_size = 13.0\n").unwrap();
        let loaded = crate::load::load_from_source(&out, Path::new("m.lua")).unwrap();
        assert_eq!(loaded.config.font_size, 13.0);
    }

    #[test]
    fn build_does_not_double_scaffold_proper_file() {
        let text = "local config = {}\nreturn config\n";
        let out = build_marked_file(text, "config.font_size = 12.0\n").unwrap();
        let marker_pos = out.find(MARKER_START).unwrap();
        let end_pos = out.find(MARKER_END).unwrap();
        assert!(
            !out[marker_pos..end_pos].contains("local config"),
            "已有脚手架的文件不得再注入 local config:\n{out}"
        );
        assert!(crate::load::load_from_source(&out, Path::new("m.lua")).is_ok());
    }
}
