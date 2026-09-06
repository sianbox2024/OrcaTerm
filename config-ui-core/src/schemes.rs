//! 内置配色方案清单：与 `wezterm.color.get_builtin_schemes` 同源（config::COLOR_SCHEMES），
//! 供 GUI 组2 方案网格展示名称与色板。

#[derive(Debug, Clone, PartialEq)]
pub struct SchemeInfo {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub ansi: Vec<String>,
    pub brights: Vec<String>,
}

/// 按名称排序的全部内置方案。
pub fn builtin_schemes() -> Vec<SchemeInfo> {
    let mut list: Vec<_> = config::COLOR_SCHEMES
        .iter()
        .map(|(name, p)| SchemeInfo {
            name: name.clone(),
            background: p.background.as_ref().map(|c| c.to_rgb_string()).unwrap_or_default(),
            foreground: p.foreground.as_ref().map(|c| c.to_rgb_string()).unwrap_or_default(),
            ansi: p
                .ansi
                .as_ref()
                .map(|a| a.iter().map(|c| c.to_rgb_string()).collect())
                .unwrap_or_default(),
            brights: p
                .brights
                .as_ref()
                .map(|a| a.iter().map(|c| c.to_rgb_string()).collect())
                .unwrap_or_default(),
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_list_is_sorted_and_populated() {
        let schemes = builtin_schemes();
        assert!(schemes.len() > 100, "内置方案应有数百个，实际 {}", schemes.len());
        let mut sorted = schemes.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(schemes, sorted);
    }

    #[test]
    fn scheme_swatches_are_complete_hex_colors() {
        for sc in builtin_schemes() {
            assert_eq!(sc.ansi.len(), 8, "{} ansi", sc.name);
            assert_eq!(sc.brights.len(), 8, "{} brights", sc.name);
            for c in [&sc.background, &sc.foreground]
                .into_iter()
                .chain(sc.ansi.iter())
                .chain(sc.brights.iter())
            {
                assert!(c.starts_with('#') && c.len() >= 7, "{} 颜色 {} 非 hex", sc.name, c);
            }
        }
    }
}

