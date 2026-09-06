pub fn wezterm_version() -> &'static str {
    // orca-term 品牌化：保留上游版本号便于对齐问题，前缀标明分叉品牌
    concat!("orca-term ", env!("WEZTERM_CI_TAG"), " (based on WezTerm)")
}

pub fn wezterm_target_triple() -> &'static str {
    // See build.rs
    env!("WEZTERM_TARGET_TRIPLE")
}
