fn main() {
    // Windows 下把应用图标嵌入 exe 资源（资源 ID 0x101 与主程序
    // wezterm-gui/build.rs 保持一致），explorer/任务栏才能显示品牌图标。
    #[cfg(windows)]
    {
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
            ico_path.display().to_string().replace("\\", "\\\\")
        )
        .unwrap();
        drop(rcfile);

        // Obtain MSVC environment so that the rc compiler can find the right headers.
        let target = std::env::var("TARGET").unwrap();
        if let Some(tool) = cc::windows_registry::find_tool(target.as_str(), "cl.exe") {
            for (key, value) in tool.env() {
                // edition 2024 要求 set_var 显式 unsafe（此处为构建脚本早期、
                // 单线程阶段，符合其安全约定）
                unsafe { std::env::set_var(key, value) };
            }
        }
        embed_resource::compile(rcfile_name);
    }
}
