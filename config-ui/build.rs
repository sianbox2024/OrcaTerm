fn main() {
    // 「关于」页版本标识（与主程序 wezterm-gui 同源）：GUI 包版本取自
    // ../wezterm-gui/Cargo.toml 的 package version，构建号取自
    // ../build-id.txt（build.sh/build.ps1 构建前写入，缺失时回退 dev），
    // 均编译期注入为环境变量；主界面标签栏右端不再显示版本号，
    // 统一收拢到设置窗体「关于」页（config-ui/src/main.rs 的 render_about_tab）。
    // 与下方图标嵌入共用同一 repo_dir 推导：config-ui 是独立 workspace，
    // build.rs 的 cwd 即 config-ui/。
    if let Some(repo_dir) = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.parent().map(|p| p.to_path_buf()))
    {
        println!("cargo:rerun-if-changed=../wezterm-gui/Cargo.toml");
        println!("cargo:rerun-if-changed=../build-id.txt");

        // 最小解析 [package] 段的 version 行，不引入 toml 依赖
        let mut gui_version = "0.0.0".to_string();
        let manifest = repo_dir.join("wezterm-gui").join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            let mut in_package = false;
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_package = trimmed == "[package]";
                    continue;
                }
                if in_package {
                    if let Some(value) = trimmed
                        .strip_prefix("version")
                        .and_then(|rest| rest.trim_start().strip_prefix('='))
                        .map(|v| v.trim().trim_matches('"'))
                    {
                        if !value.is_empty() {
                            gui_version = value.to_string();
                            break;
                        }
                    }
                }
            }
        }
        println!("cargo:rustc-env=ORCA_GUI_VERSION={gui_version}");

        let build_id = std::fs::read_to_string(repo_dir.join("build-id.txt"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "dev".to_string());
        println!("cargo:rustc-env=ORCA_BUILD_ID={build_id}");
    }

    // Windows 下把应用图标嵌入 exe 资源（资源 ID 0x101 与主程序
    // wezterm-gui/build.rs 保持一致），explorer/任务栏才能显示品牌图标。
    // 必须按目标平台判断：交叉编译时宿主不是 Windows，#[cfg(windows)] 恒假会
    // 静默跳过，导致 config-ui 的 exe 缺图标与版本信息（同 wezterm-gui/build.rs）。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        use std::io::Write;
        use std::path::Path;

        // config-ui 是独立 workspace，build.rs 的 cwd 即 config-ui/，
        // 图标复用主项目的 assets/windows/terminal.ico
        let repo_dir = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.parent().map(|p| p.to_path_buf()))
            .unwrap();
        let ico_path = repo_dir.join("assets").join("windows").join("terminal.ico");

        let rcfile_name = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("resource.rc");
        let mut rcfile = std::fs::File::create(&rcfile_name).unwrap();
        println!(
            "cargo:rerun-if-changed={}",
            ico_path.display().to_string().replace('\\', "/")
        );
        write!(
            rcfile,
            r#"
#include <winres.h>
#define IDI_ICON 0x101
IDI_ICON ICON "{}"
VS_VERSION_INFO VERSIONINFO
FILEVERSION     0,1,0,0
PRODUCTVERSION  0,1,0,0
FILEFLAGSMASK   VS_FFI_FILEFLAGSMASK
FILEFLAGS       0
FILEOS          VOS__WINDOWS32
FILETYPE        VFT_APP
FILESUBTYPE     VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904E4"
        BEGIN
            VALUE "CompanyName",      "SianBox\0"
            VALUE "FileDescription",  "OrcaTerm Settings\0"
            VALUE "FileVersion",      "0.1\0"
            VALUE "LegalCopyright",   "SianBox, MIT licensed\0"
            VALUE "InternalName",     "\0"
            VALUE "OriginalFilename", "\0"
            VALUE "ProductName",      "OrcaTerm\0"
            VALUE "ProductVersion",   "0.1\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1252
    END
END
"#,
            ico_path.display().to_string().replace('\\', "/")
        )
        .unwrap();
        drop(rcfile);

        // Obtain MSVC environment so that the rc compiler can find the right headers.
        // 仅宿主为 Windows 时需要；Linux 宿主下 windres 自带 mingw 头文件。
        #[cfg(windows)]
        {
            let target = std::env::var("TARGET").unwrap();
            if let Some(tool) = cc::windows_registry::find_tool(target.as_str(), "cl.exe") {
                for (key, value) in tool.env() {
                    // edition 2024 要求 set_var 显式 unsafe（此处为构建脚本早期、
                    // 单线程阶段，符合其安全约定）
                    unsafe { std::env::set_var(key, value) };
                }
            }
        }
        embed_resource::compile(rcfile_name);
    }
}
