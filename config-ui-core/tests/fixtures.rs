//! Phase 8 集成测试：真实配置样本库（社区常见 .wezterm.lua 风格）。
//! 每个样本必须满足：
//! 1. 读链路（luahelper 无头执行）可解析出非 Null 快照；
//! 2. 插入/重写标记区后，区外内容逐字节零丢失；
//! 3. 迁移后的完整文件仍可被读链路解析。

use std::fs;
use std::path::Path;

use config_ui_core::load::{config_to_json, load_from_source};
use config_ui_core::markers::{build_marked_file, split};

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn fixtures_load_and_survive_marker_roundtrip() {
    let mut count = 0;
    let mut names = Vec::new();
    for entry in fs::read_dir(fixture_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e != "lua").unwrap_or(true) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).unwrap();

        // 1) 读链路可解析
        let loaded = load_from_source(&text, &path)
            .unwrap_or_else(|e| panic!("{name}: 读链路失败: {e}"));
        let snap = config_to_json(&loaded.config);
        assert!(!snap.is_null(), "{name}: 快照为 Null");

        // 2) 标记区写入后区外内容保留：
        //    已声明 config 表（或无顶层 return）的文件逐字节一致；
        //    未声明 config 表的文件走 return 行改写迁移，仅允许改写那一行。
        let outside_before = match split(&text).unwrap_or_else(|e| panic!("{name}: 切分失败: {e}")) {
            Some(s) => format!("{}{}", s.before, s.after),
            None => text.clone(),
        };
        let marked = build_marked_file(&text, "config.font_size = 14\n")
            .unwrap_or_else(|e| panic!("{name}: 构建标记文件失败: {e}"));
        let sections = split(&marked)
            .unwrap_or_else(|e| panic!("{name}: 迁移后切分失败: {e}"))
            .expect("{name}: 迁移后必须含标记区");
        let outside_after = format!("{}{}", sections.before, sections.after);
        let declares_config = text.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with("local config") || t.starts_with("config =")
        });
        let has_top_return = text.lines().any(|l| l.trim_end().starts_with("return "));
        if declares_config || !has_top_return {
            assert_eq!(outside_after, outside_before, "{name}: 区外内容被改动");
        } else {
            for line in outside_before.lines() {
                if line.trim_start().starts_with("return ") {
                    continue;
                }
                assert!(
                    outside_after.contains(line),
                    "{name}: 区外行丢失: {line:?}"
                );
            }
        }

        // 3) 迁移后的文件仍可读链路解析
        load_from_source(&marked, &path)
            .unwrap_or_else(|e| panic!("{name}: 迁移后读链路失败: {e}"));

        count += 1;
        names.push(name);
    }
    assert!(count >= 20, "fixture 库应至少 20 个样本，实际 {count}: {names:?}");
}


