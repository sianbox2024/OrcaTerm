//! 读链路：无头执行用户 Lua 配置 → `Config`（含默认值合并）→ JSON 快照。
//!
//! 复刻 `config::Config::try_load` 的核心流程（不含 CLI overrides 与全局环境变量副作用），
//! 因此可安全地在测试与 GUI 进程中调用。

use config::lua::make_lua_context;
use config::Config;
use mlua::{FromLua, Value as LuaValue};
use std::path::Path;
use wezterm_dynamic::ToDynamic;

/// 加载失败信息：`message` 为面向用户的完整描述，`line` 为能从 Lua 错误中解析出的源码行号。
#[derive(Debug, Clone)]
pub struct LoadError {
    pub message: String,
    pub line: Option<usize>,
    pub warnings: Vec<String>,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LoadError {}

pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded").finish_non_exhaustive()
    }
}

/// 无头执行配置源码并返回生效的 `Config`（默认值已合并）。
pub fn load_from_source(src: &str, display_path: &Path) -> Result<Loaded, LoadError> {
    let (result, warnings) =
        wezterm_dynamic::Error::capture_warnings(move || -> anyhow::Result<Config> {
            let lua = make_lua_context(display_path)?;
            let chunk = src.trim_start_matches('\u{FEFF}');
            let chunk = if chunk.trim().is_empty() {
                "return {}"
            } else {
                chunk
            };
            let value = smol::block_on(
                lua.load(chunk)
                    .set_name(display_path.to_string_lossy().to_string())
                    .eval_async(),
            )?;
            if matches!(value, LuaValue::Nil) {
                return Ok(Config::default_config());
            }
            let cfg = Config::from_lua(value, &lua)?;
            cfg.check_consistency()?;
            // 提前物化键绑定，让绑定相关错误在加载阶段暴露（与上游 try_load 一致）
            let _ = cfg.key_bindings();
            Ok(cfg.compute_extra_defaults(Some(display_path)))
        });
    match result {
        Ok(config) => Ok(Loaded { config, warnings }),
        Err(err) => {
            let message = format!("{err:#}");
            let line = extract_line(&message);
            Err(LoadError {
                message,
                line,
                warnings,
            })
        }
    }
}

/// 将生效 `Config` 转为 GUI 可消费的 JSON（全部字段，按需取用）。
pub fn config_to_json(config: &Config) -> serde_json::Value {
    dynamic_to_json(&config.to_dynamic())
}

fn dynamic_to_json(value: &wezterm_dynamic::Value) -> serde_json::Value {
    use wezterm_dynamic::Value as V;
    match value {
        V::Null => serde_json::Value::Null,
        V::Bool(b) => (*b).into(),
        V::String(s) => s.clone().into(),
        V::Array(items) => serde_json::Value::Array(items.iter().map(dynamic_to_json).collect()),
        V::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj.iter() {
                let key = match k {
                    V::String(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                map.insert(key, dynamic_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        V::U64(u) => (*u).into(),
        V::I64(i) => (*i).into(),
        V::F64(f) => (f.0).into(),
    }
}

/// 从 Lua 错误文本中提取行号（形如 `[string "x.lua"]:12: ...` 或 `x.lua:12: ...`）。
fn extract_line(message: &str) -> Option<usize> {
    if let Some(pos) = message.find("]:") {
        return parse_line_at(&message[pos + 2..]);
    }
    for (i, c) in message.char_indices() {
        if c == ':' {
            if let Some(n) = parse_line_at(&message[i + 1..]) {
                return Some(n);
            }
        }
    }
    None
}

fn parse_line_at(s: &str) -> Option<usize> {
    let digits: usize = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || !s[digits..].starts_with(':') {
        return None;
    }
    s[..digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn load(src: &str) -> Result<Loaded, LoadError> {
        load_from_source(src, Path::new("test-config.lua"))
    }

    #[test]
    fn simple_config_loads() {
        let loaded = load("local c = {}\nc.font_size = 14\nreturn c\n").unwrap();
        assert_eq!(loaded.config.font_size, 14.0);
    }

    #[test]
    fn unset_options_fall_back_to_defaults() {
        let loaded = load("return {}\n").unwrap();
        assert_eq!(
            loaded.config.scrollback_lines,
            Config::default_config().scrollback_lines
        );
    }

    #[test]
    fn syntax_error_reports_line() {
        // Lua 对未闭合结构会把错误报到 EOF 所在行；此处用非法符号确保错误落在第 2 行
        let err = load("-- line1\nlocal c = )\n").unwrap_err();
        assert_eq!(err.line, Some(2), "message: {}", err.message);
    }

    #[test]
    fn runtime_error_reports_line() {
        let src = "-- line1\nnosuchfunction()\nreturn {}\n";
        let err = load(src).unwrap_err();
        assert_eq!(err.line, Some(2), "message: {}", err.message);
    }

    #[test]
    fn unknown_field_error_names_field() {
        let src = "local c = {}\nc.definitely_not_an_option = 1\nreturn c\n";
        match load(src) {
            Err(err) => assert!(
                err.message.contains("definitely_not_an_option"),
                "message: {}",
                err.message
            ),
            Ok(loaded) => {
                println!("WARNINGS: {:?}", loaded.warnings);
                assert!(
                    loaded
                        .warnings
                        .iter()
                        .any(|w| w.contains("definitely_not_an_option")),
                    "未知字段既未报错也未出现在警告中: {:?}",
                    loaded.warnings
                );
            }
        }
    }

    #[test]
    fn action_and_keys_load() {
        let src = concat!(
            "local wezterm = require 'wezterm'\n",
            "local c = {}\n",
            "c.keys = { { key='c', mods='CTRL', action=wezterm.action.CopyTo 'Clipboard' } }\n",
            "return c\n",
        );
        let loaded = load(src).unwrap();
        assert_eq!(loaded.config.keys.len(), 1);
    }

    #[test]
    fn json_snapshot_carries_values() {
        let loaded = load("local c = {}\nc.font_size = 14\nreturn c\n").unwrap();
        let json = config_to_json(&loaded.config);
        assert_eq!(json["font_size"], json!(14.0));
        assert!(json["scrollback_lines"].is_u64());
    }

    #[test]
    fn extract_line_parses_chunk_prefix() {
        assert_eq!(
            extract_line(r#"[string "C:/x/.wezterm.lua"]:12: boom"#),
            Some(12)
        );
        assert_eq!(extract_line("no line info here"), None);
    }

    #[test]
    fn empty_source_falls_back_to_default_config() {
        let loaded = load("").unwrap();
        assert_eq!(
            loaded.config.scrollback_lines,
            Config::default_config().scrollback_lines
        );
    }

    #[test]
    fn comments_only_source_falls_back_to_default_config() {
        let loaded = load("-- only a comment\n-- another one\n").unwrap();
        assert_eq!(
            loaded.config.font_size,
            Config::default_config().font_size
        );
    }

    #[test]
    fn nil_returning_source_falls_back_to_default_config() {
        let loaded = load("local x = nil\nreturn x\n").unwrap();
        assert_eq!(
            loaded.config.scrollback_lines,
            Config::default_config().scrollback_lines
        );
    }
}
