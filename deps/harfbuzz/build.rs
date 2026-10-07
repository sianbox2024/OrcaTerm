use std::env;
use std::path::{Path, PathBuf};

fn harfbuzz() {
    use std::fs;

    if !Path::new("harfbuzz/.git").exists() {
        git_submodule_update();
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut cfg = cc::Build::new();
    cfg.warnings(false);
    cfg.cpp(true);
    cfg.flag_if_supported("-fno-rtti");
    cfg.flag_if_supported("-fno-exceptions");
    cfg.flag_if_supported("-fno-threadsafe-statics");
    cfg.flag_if_supported("-std=c++11");
    cfg.flag_if_supported("-fno-stack-check");
    cfg.flag_if_supported("-Wno-format-overflow");
    // MinGW 的 gas 汇编器在 debug 构建（海量模板实例化 + 调试节）下会超出
    // COFF 节数上限（"too many sections"/"file too big"），改用 big-obj
    // 目标格式；不支持该 flag 的工具链（ELF gas、MSVC）会被自动跳过。
    cfg.flag_if_supported("-Wa,-mbig-obj");

    let build_dir = out_dir.join("harfbuzz-build");
    fs::create_dir_all(&build_dir).unwrap();
    cfg.out_dir(&build_dir);

    let target = env::var("TARGET").unwrap();

    cfg.file("harfbuzz/src/harfbuzz.cc");
    cfg.define("HB_NO_MT", None);

    if !target.contains("windows") {
        cfg.define("HAVE_UNISTD_H", None);
        cfg.define("HAVE_SYS_MMAN_H", None);
    }

    // We know that these are present in our vendored freetype
    cfg.define("HAVE_FREETYPE", Some("1"));

    cfg.define("HAVE_FT_GET_VAR_BLEND_COORDINATES", Some("1"));
    cfg.define("HAVE_FT_SET_VAR_BLEND_COORDINATES", Some("1"));
    cfg.define("HAVE_FT_DONE_MM_VAR", Some("1"));
    cfg.define("HAVE_FT_GET_TRANSFORM", Some("1"));

    // Import the include dirs exported from deps/freetype/build.rs
    for inc in std::env::var("DEP_FREETYPE_INCLUDE").unwrap().split(';') {
        cfg.include(inc);
    }

    println!(
        "cargo:rustc-link-search={}",
        std::env::var("DEP_FREETYPE_LIB").unwrap()
    );
    println!("cargo:rustc-link-lib=freetype");
    println!("cargo:rustc-link-lib=png");
    println!("cargo:rustc-link-lib=z");

    cfg.compile("harfbuzz");
}

fn git_submodule_update() {
    let _ = std::process::Command::new("git")
        .args(&["submodule", "update", "--init"])
        .status();
}

fn main() {
    harfbuzz();
    let out_dir = env::var("OUT_DIR").unwrap();
    println!("cargo:outdir={}", out_dir);
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=10.12");
}
