//! 回归：模拟 GUI 保存链路（load → FormState::from_snapshot → 改字体 → to_lua），
//! 验证「只改字体保存」不会把磁盘上的 default_prog 冲掉。
//! 事故根因曾是 config-ui sync_inputs 里对 form.default_prog 的硬编码覆写
//! （powershell.exe stomp），本测试在 core 层钉住 form 侧语义。

use config_ui_core::form::FormState;
use config_ui_core::load::{config_to_json, load_from_source};
use serde_json::json;
use std::path::Path;

/// 与 dist/orca-config.lua 等价的用户现场：pwsh 7 作为默认 shell。
const DISK_CONFIG: &str = r#"
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
config.font = wezterm.font('Cascadia Code')
config.default_prog = { 'D:/Tools/PowerShell/7/pwsh.exe', '-NoLogo' }
return config
"#;

#[test]
fn font_only_save_preserves_default_prog() {
    let loaded = load_from_source(DISK_CONFIG, Path::new("orca-config.lua")).unwrap();
    let snapshot = config_to_json(&loaded.config);
    let baseline = json!({}); // GUI 的 diff 基线 = 出厂默认（空配置）

    // form 必须从磁盘快照装到 pwsh 7（曾经被 sync_inputs 硬编码 stomp 成 powershell.exe）
    let mut form = FormState::from_snapshot(&snapshot, &baseline);
    assert_eq!(
        form.default_prog,
        Some(vec![
            "D:/Tools/PowerShell/7/pwsh.exe".to_string(),
            "-NoLogo".to_string()
        ]),
        "form.default_prog 应来自磁盘配置，而非硬编码默认"
    );

    // 用户只改字体后保存：default_prog 原样发射
    form.font_family = Some("MesloLGS NF".to_string());
    let lua = form.to_lua(&baseline);
    assert!(
        lua.contains("config.font = wezterm.font('MesloLGS NF')"),
        "{lua}"
    );
    assert!(
        lua.contains("config.default_prog = { 'D:/Tools/PowerShell/7/pwsh.exe', '-NoLogo' }"),
        "保存不应改写 default_prog：{lua}"
    );
}

#[test]
fn empty_default_prog_follows_runtime_default() {
    // 空模板（GUI 首启生成的 orca-config.lua）：加载后 default_prog 物化为
    // 主程序的平台默认（本机 pwsh 探测结果）。form 会持有该值，但 diff 基线
    // 同样物化为同一默认 → to_lua 判定「与默认一致」不发射，配置文件保持
    // 不写 default_prog，主程序下次启动仍自由跟随探测结果。
    let loaded = load_from_source(
        "local wezterm = require 'wezterm'\nlocal config = wezterm.config_builder()\nreturn config\n",
        Path::new("orca-config.lua"),
    )
    .unwrap();
    let snapshot = config_to_json(&loaded.config);

    // GUI 真实基线 = 同一默认物化链（load_default_snapshot 加载 return {}）
    let baseline_loaded =
        load_from_source("return {}\n", Path::new("defaults.lua")).unwrap();
    let baseline = config_to_json(&baseline_loaded.config);

    let form = FormState::from_snapshot(&snapshot, &baseline);
    // 物化默认可能与空模板字面一致；关键是 to_lua 对「form == baseline」不发射
    let lua = form.to_lua(&baseline);
    assert!(
        !lua.contains("default_prog"),
        "空模板下 form 与基线一致，不得发射 default_prog：{lua}（form={:?} baseline={:?}）",
        form.default_prog,
        baseline.get("default_prog")
    );
}
