//! 集成测试：真实配置样本库（社区常见 .wezterm.lua 风格）。
//! 每个样本必须满足：读链路（luahelper 无头执行）可解析出非 Null 快照
//! 且零警告——守卫 loader 对真实世界配置的兼容性。

use std::fs;
use std::path::Path;

use config_ui_core::load::{config_to_json, load_from_source};

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn fixture_library_loads_cleanly() {
    let mut count = 0;
    let mut names = Vec::new();
    for entry in fs::read_dir(fixture_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e != "lua").unwrap_or(true) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).unwrap();

        // 读链路可解析且快照非 Null
        let loaded = load_from_source(&text, &path)
            .unwrap_or_else(|e| panic!("{name}: 读链路失败: {e}"));
        let snap = config_to_json(&loaded.config);
        assert!(!snap.is_null(), "{name}: 快照为 Null");
        assert!(
            loaded.warnings.is_empty(),
            "{name}: 加载警告: {:?}",
            loaded.warnings
        );

        count += 1;
        names.push(name);
    }
    assert!(count >= 20, "fixture 库应至少 20 个样本，实际 {count}: {names:?}");
}
