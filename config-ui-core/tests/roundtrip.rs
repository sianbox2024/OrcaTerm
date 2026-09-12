//! 集成测试：真实复杂度样本的读取（平台条件逻辑、font_rules 等 loader 覆盖）。

use config_ui_core::load::{config_to_json, load_from_source};
use std::path::Path;

const SAMPLE: &str = include_str!("fixtures/sample-wezterm.lua");

#[test]
fn real_world_sample_loads() {
    let loaded = load_from_source(SAMPLE, Path::new("sample-wezterm.lua")).unwrap();
    // 用户显式值生效
    assert_eq!(loaded.config.font_size, 11.5);
    assert!(loaded.config.enable_tab_bar);
    // 平台条件逻辑已执行（headless 环境下 target_triple 为空走 else 分支，
    // 断言"二选一生效"即可，不依赖构建期注入）
    let prog = &loaded.config.default_prog.as_ref().unwrap();
    assert!(prog[0] == "pwsh.exe" || prog[0] == "/bin/zsh");
}

#[test]
fn real_world_sample_json_snapshot() {
    let loaded = load_from_source(SAMPLE, Path::new("sample-wezterm.lua")).unwrap();
    let json = config_to_json(&loaded.config);
    assert_eq!(json["font_size"], 11.5);
    assert_eq!(json["color_scheme"], "Tokyo Night");
    assert!(json["font_rules"].as_array().unwrap().len() >= 1);
}
