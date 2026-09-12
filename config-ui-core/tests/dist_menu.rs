//! 验证 dist/orca-config.lua 新 launch_menu 结构经真实读链路正确解析。
//! 该文件是测试现场件（build.ps1 不生成、用户同步后会清掉 dist），
//! 缺失时跳过而非编译失败——include_str! 的编译期依赖曾三次让测试被环境问题卡死。
use config_ui_core::load::{config_to_json, load_from_source};
use config_ui_core::ssh::parse_ssh_domains;
use std::path::Path;
use std::path::PathBuf;

fn dist_config_src() -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/orca-config.lua");
    std::fs::read_to_string(path).ok()
}

#[test]
fn dist_config_launch_menu_has_admin_entries_and_ssh() {
    let Some(src) = dist_config_src() else {
        eprintln!("dist/orca-config.lua 不存在（用户同步后清空 dist），跳过");
        return;
    };
    let loaded = load_from_source(&src, Path::new("orca-config.lua")).unwrap();
    let json = config_to_json(&loaded.config);

    let menu = json["launch_menu"].as_array().unwrap();
    let labels: Vec<&str> = menu
        .iter()
        .filter_map(|i| i.get("label").and_then(|l| l.as_str()))
        .collect();
    assert_eq!(
        labels,
        [
            "新CMD窗口",
            "新PowerShell 7窗口",
            "新管理员CMD窗口",
            "新管理员PowerShell 7窗口",
            "MSI"
        ],
        "菜单应为 4 固定 shell + SSH 顺延"
    );

    // 默认 shell 指向本机 pwsh 7
    assert_eq!(
        json["default_prog"],
        serde_json::json!(["D:/Tools/PowerShell/7/pwsh.exe", "-NoLogo"])
    );
    // PowerShell 菜单项同样指向 pwsh 7
    assert_eq!(
        menu[1]["args"],
        serde_json::json!(["D:/Tools/PowerShell/7/pwsh.exe", "-NoLogo"])
    );

    // 管理员两项通过原生 elevate=true 提权（UAC → 提权新 GUI 实例）
    assert_eq!(
        menu[2]["args"],
        serde_json::json!(["cmd.exe"])
    );
    assert_eq!(menu[2]["elevate"], serde_json::json!(true));
    assert_eq!(
        menu[3]["args"],
        serde_json::json!(["D:/Tools/PowerShell/7/pwsh.exe", "-NoLogo"])
    );
    assert_eq!(menu[3]["elevate"], serde_json::json!(true));

    // SSH 项走 domain 引用，且 ssh_domains 里有对应域
    assert_eq!(
        menu[4]["domain"]["DomainName"],
        serde_json::json!("MSI")
    );
    let conns = parse_ssh_domains(&json);
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].host, "192.168.0.164");
    assert_eq!(conns[0].username, "sb");

    // format-tab-title 回调存在且含管理员后缀逻辑
    assert!(
        src.contains("wezterm.on('format-tab-title'"),
        "缺少 format-tab-title 回调"
    );
    assert!(src.contains("(管理员)"), "缺少管理员后缀");

    // 加载零警告（无未知字段）
    assert!(
        loaded.warnings.is_empty(),
        "警告: {:?}",
        loaded.warnings
    );
}
