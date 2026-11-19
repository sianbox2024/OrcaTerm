//! 内置配色方案清单：与 `wezterm.color.get_builtin_schemes` 同源（config::COLOR_SCHEMES），
//! 供 GUI 组2 方案网格展示名称与色板。
//!
//! `config::COLOR_SCHEMES` 有上千个（大量 (Gogh)/(base16)/(terminal.sexy) 变体），
//! 直接按英文名平铺时辨识度很差。这里额外提供一份**精选目录**：每个家族只留一个
//! 视觉上最有特点的正主，并附中文名与一句话特点。底层方案一个不删，搜索仍能
//! 按原名找到全部别名，已有配置里的方案名也不会失效。

#[derive(Debug, Clone, PartialEq)]
pub struct SchemeInfo {
    /// 写入 `config.color_scheme` 的原始名称
    pub name: String,
    /// 界面显示名（精选方案为中文名，其余为原名）
    pub display_name: String,
    /// 一句话特点（非精选方案为空）
    pub note: String,
    /// 背景是否偏暗，供界面标记「深色/浅色」
    pub is_dark: bool,
    pub background: String,
    pub foreground: String,
    pub ansi: Vec<String>,
    pub brights: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CuratedScheme {
    pub name: &'static str,
    pub label: &'static str,
    pub note: &'static str,
}

/// 精选经典方案：一个家族只保留一个辨识度最高的版本。
/// 名称必须精确存在于 config::COLOR_SCHEMES（测试会校验）。
pub const CURATED_SCHEMES: &[CuratedScheme] = &[
    CuratedScheme { name: "nord", label: "Nord · 北欧冷蓝", note: "低饱和冷蓝灰，长时间阅读最舒适" },
    CuratedScheme { name: "Dracula (Official)", label: "Dracula · 德古拉紫", note: "紫粉霓虹，官方配色的代表作" },
    CuratedScheme { name: "Tokyo Night", label: "Tokyo Night · 东京夜", note: "深蓝底 + 青紫点缀，现代感强" },
    CuratedScheme { name: "Catppuccin Mocha", label: "Catppuccin Mocha · 猫咖棕", note: "柔和棕紫，粉彩高亮" },
    CuratedScheme { name: "Catppuccin Latte", label: "Catppuccin Latte · 猫咖牛奶", note: "同系列浅色版，暖白底" },
    CuratedScheme { name: "Gruvbox Dark (Gogh)", label: "Gruvbox Dark · 复古砖橙", note: "砖红与土黄，经典复古味" },
    CuratedScheme { name: "Gruvbox light, hard (base16)", label: "Gruvbox Light · 复古米黄", note: "米黄纸底，高对比浅色" },
    CuratedScheme { name: "Builtin Solarized Dark", label: "Solarized Dark · 日晒夜", note: "青蓝底，最经典的低对比方案" },
    CuratedScheme { name: "Builtin Solarized Light", label: "Solarized Light · 日晒昼", note: "米白底，日光下的柔和对照" },
    CuratedScheme { name: "One Dark (Gogh)", label: "One Dark · VS Code 深", note: "VS Code 同款，蓝灰克制" },
    CuratedScheme { name: "Monokai (base16)", label: "Monokai · 猴子绿", note: "绿黄高亮，编辑器老牌配色" },
    CuratedScheme { name: "Kanagawa Dragon (Gogh)", label: "Kanagawa · 神奈川龙", note: "墨蓝与樱粉，日式浮世绘" },
    CuratedScheme { name: "Everforest Dark (Gogh)", label: "Everforest · 森林墨绿", note: "护眼墨绿，柔和不刺眼" },
    CuratedScheme { name: "rose-pine", label: "Rosé Pine · 松针玫瑰夜", note: "灰紫底 + 玫瑰粉，文艺感" },
    CuratedScheme { name: "rose-pine-dawn", label: "Rosé Pine Dawn · 松针玫瑰昼", note: "同系列浅色，奶油粉底" },
    CuratedScheme { name: "Ayu Mirage", label: "Ayu Mirage · 沙漠幻影", note: "暗橙与灰蓝，温暖沉静" },
    CuratedScheme { name: "Night Owl (Gogh)", label: "Night Owl · 夜猫蓝", note: "深蓝紫，高饱和冷色调" },
    CuratedScheme { name: "Cobalt2", label: "Cobalt2 · 钴蓝", note: "亮蓝底 + 橙黄，辨识度极高" },
    CuratedScheme { name: "Zenburn", label: "Zenburn · 柔绿", note: "低对比灰绿，禅意护眼" },
    CuratedScheme { name: "MaterialOcean", label: "Material Ocean · 深海蓝", note: "深蓝 + 青绿，Material 代表作" },
    CuratedScheme { name: "3024 (base16)", label: "3024 · 炭灰", note: "中性炭灰，几乎无色彩偏向" },
    CuratedScheme { name: "Alabaster", label: "Alabaster · 白垩纸", note: "纯白底高对比，浅色标杆" },
    CuratedScheme { name: "AtomOneLight", label: "Atom One Light · 明亮代码", note: "白底亮色关键字，清爽" },
    CuratedScheme { name: "Tomorrow Night", label: "Tomorrow Night · 午夜蓝", note: "蓝黑底，经典 Tomorrow 夜版" },
    CuratedScheme { name: "synthwave", label: "Synthwave · 霓虹紫", note: "紫红霓虹， vaporwave 风格" },
    CuratedScheme { name: "Nord Light (Gogh)", label: "Nord Light · 北欧昼", note: "Nord 浅色版，冷白底" },
    CuratedScheme { name: "Material (Gogh)", label: "Material · 暗物质", note: "灰蓝底 + 亮橙，Material 经典" },
    CuratedScheme { name: "Tomorrow", label: "Tomorrow · 明日浅色", note: "白底柔和，Tomorrow 日版" },
];

fn background_is_dark(background: &str) -> bool {
    let hex = background.trim().trim_start_matches('#');
    if hex.len() < 6 {
        return true;
    }
    let Ok(r) = u8::from_str_radix(&hex[0..2], 16) else {
        return true;
    };
    let Ok(g) = u8::from_str_radix(&hex[2..4], 16) else {
        return true;
    };
    let Ok(b) = u8::from_str_radix(&hex[4..6], 16) else {
        return true;
    };
    (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 < 128
}

/// 按名称排序的全部内置方案（含所有变体，供搜索使用）。
pub fn builtin_schemes() -> Vec<SchemeInfo> {
    let mut list: Vec<_> = config::COLOR_SCHEMES
        .iter()
        .map(|(name, p)| {
            let background = p.background.as_ref().map(|c| c.to_rgb_string()).unwrap_or_default();
            SchemeInfo {
                name: name.clone(),
                display_name: name.clone(),
                note: String::new(),
                is_dark: background_is_dark(&background),
                background,
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
            }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// 精选方案目录，顺序即界面上的「精选顺序」。
pub fn curated_schemes() -> Vec<SchemeInfo> {
    CURATED_SCHEMES
        .iter()
        .filter_map(|c| {
            let p = config::COLOR_SCHEMES.get(c.name)?;
            let background = p.background.as_ref().map(|c| c.to_rgb_string()).unwrap_or_default();
            Some(SchemeInfo {
                name: c.name.to_string(),
                display_name: c.label.to_string(),
                note: c.note.to_string(),
                is_dark: background_is_dark(&background),
                background,
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
        })
        .collect()
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

    #[test]
    fn curated_schemes_exist_and_are_distinct() {
        let curated = curated_schemes();
        assert!(curated.len() >= 20, "精选方案太少：{}", curated.len());
        assert_eq!(curated.len(), CURATED_SCHEMES.len(), "有精选名称在内置方案中不存在");
        let mut labels = std::collections::HashSet::new();
        let mut palettes = std::collections::HashSet::new();
        for sc in &curated {
            assert!(labels.insert(sc.display_name.clone()), "重复显示名 {}", sc.display_name);
            assert!(!sc.note.is_empty(), "{} 缺特点说明", sc.name);
            let fp = format!("{}{}{:?}{:?}", sc.background, sc.foreground, sc.ansi, sc.brights);
            assert!(palettes.insert(fp), "精选方案调色板重复：{}", sc.name);
        }
    }
}
