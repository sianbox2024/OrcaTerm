//! Phase 8 性能验收基准：读链路 + 发射链路耗时。
//! 验收线：保存（to_lua 发射）< 100ms；读快照 < 2s（GUI 冷启动预算内）。
//! 滚动 60fps 与窗口冷启动需真机人工观察，此处覆盖可自动化的部分。

use std::path::Path;
use std::time::Instant;

use config_ui_core::load::{config_to_json, load_from_source};
use config_ui_core::form::FormState;

fn load_defaults() -> serde_json::Value {
    let loaded = load_from_source("return {}\n", Path::new("defaults.lua")).unwrap();
    config_to_json(&loaded.config)
}

#[test]
fn emit_latency_under_100ms() {
    let defaults = load_defaults();
    let mut fm = FormState::from_snapshot(&defaults, &defaults);
    fm.scalars.set(config_ui_core::settings::index("font_size").unwrap(), config_ui_core::settings::SettingValue::Float(16.0));
    fm.color_scheme = "Catppuccin Mocha".into();
    fm.scalars.set(config_ui_core::settings::index("scrollback_lines").unwrap(), config_ui_core::settings::SettingValue::Int(9000));

    let start = Instant::now();
    let iters = 1000;
    for _ in 0..iters {
        let _lua = fm.to_lua(&defaults);
    }
    let per_call = start.elapsed() / iters;
    assert!(
        per_call.as_millis() < 100,
        "单次发射 {per_call:?} 超过 100ms 验收线"
    );
}

#[test]
fn snapshot_load_under_2s() {
    let src = r#"
local wezterm = require 'wezterm'
local config = {}
config.font_size = 14.0
config.color_scheme = 'Catppuccin Mocha'
config.keys = {
  { key = 'c', mods = 'CTRL|SHIFT', action = wezterm.action.CopyTo 'Clipboard' },
}
return config
"#;
    for _ in 0..3 {
        let start = Instant::now();
        let loaded = load_from_source(src, Path::new("perf.lua")).unwrap();
        let snap = config_to_json(&loaded.config);
        let elapsed = start.elapsed();
        assert!(!snap.is_null());
        assert!(elapsed.as_secs() < 2, "读快照 {elapsed:?} 超过 2s 验收线");
    }
}
