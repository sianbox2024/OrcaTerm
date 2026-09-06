//! M2 检查点集成测试：真实复杂度样本的读取与写入往返。

use config_ui_core::load::{config_to_json, load_from_source};
use config_ui_core::markers::{enable_gui_management, has_markers, regenerate, split};
use std::path::Path;

const SAMPLE: &str = include_str!("fixtures/sample-wezterm.lua");
const GUI_BLOCK: &str = "config.font_size = 13.0\n";

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

#[test]
fn migration_preserves_everything_outside_marker_block() {
    let migrated = enable_gui_management(SAMPLE, GUI_BLOCK).unwrap();
    assert!(has_markers(&migrated));
    // 迁移 = 原文前缀 + 标记区 + 原文 return 尾巴，区外逐字节不变
    let sections = split(&migrated).unwrap().unwrap();
    let original_before_return = &SAMPLE[..SAMPLE.rfind("return config").unwrap()];
    let migrated_outside = format!("{}{}", sections.before, sections.after);
    assert!(
        migrated_outside.contains(original_before_return),
        "迁移后区外内容必须逐字保留"
    );
    // 加载验证：GUI 值覆盖用户值，其余保留
    let loaded = load_from_source(&migrated, Path::new("sample-wezterm.lua")).unwrap();
    assert_eq!(loaded.config.font_size, 13.0);
    assert_eq!(loaded.config.color_scheme, Some("Tokyo Night".to_string()));
    assert_eq!(loaded.config.keys.len(), 2);
}

#[test]
fn repeated_saves_keep_diff_empty_outside() {
    // 模拟两次保存：第二次保存后除标记区外 diff 必须为空
    let first = enable_gui_management(SAMPLE, GUI_BLOCK).unwrap();
    let second = regenerate(&first, "config.font_size = 14.0\n").unwrap();

    let s1 = split(&first).unwrap().unwrap();
    let s2 = split(&second).unwrap().unwrap();
    assert_eq!(s1.before, s2.before, "标记区之前的内容两次保存必须一致");
    assert_eq!(s1.after, s2.after, "标记区之后的内容两次保存必须一致");

    let loaded = load_from_source(&second, Path::new("sample-wezterm.lua")).unwrap();
    assert_eq!(loaded.config.font_size, 14.0);
}
