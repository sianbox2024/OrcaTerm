fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // 同 wezterm-gui/build.rs：必须按目标平台判断。Linux 交叉编译时宿主不是
    // Windows，#[cfg(windows)] 恒假会静默跳过资源嵌入，导致 exe 缺
    // activeCodePage=UTF-8 等 manifest 声明。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        use std::io::Write;
        use std::path::Path;

        let repo_dir = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.parent().map(|p| p.to_path_buf()))
            .unwrap();
        let windows_dir = repo_dir.join("assets").join("windows");

        let rcfile_name = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("resource.rc");
        let mut rcfile = std::fs::File::create(&rcfile_name).unwrap();
        write!(
            rcfile,
            r#"
#include <winres.h>
1 RT_MANIFEST "{win}/console.manifest"
"#,
            win = windows_dir.display().to_string().replace('\\', "/"),
        )
        .unwrap();
        drop(rcfile);

        // Obtain MSVC environment so that the rc compiler can find the right headers.
        // https://github.com/nabijaczleweli/rust-embed-resource/issues/11#issuecomment-603655972
        // 仅宿主为 Windows 时需要；Linux 宿主下 windres 自带 mingw 头文件。
        #[cfg(windows)]
        {
            let target = std::env::var("TARGET").unwrap();
            if let Some(tool) = cc::windows_registry::find_tool(target.as_str(), "cl.exe") {
                for (key, value) in tool.env() {
                    std::env::set_var(key, value);
                }
            }
        }
        embed_resource::compile(rcfile_name);
    }
}
