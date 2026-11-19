//! 配色方案的偏好状态（收藏 + 最近使用）。
//!
//! 与终端配置分离：存到 exe 同目录的 `orca-ui-state.json`（便携模式约束，
//! 不依赖 HOME/XDG）。保存用「写临时文件 + 原子 rename」，避免中途崩溃损坏状态。
//! 该状态不触发配置 dirty，纯粹是配置界面自身的交互记忆。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemePrefs {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub recent: Vec<String>,
}

impl SchemePrefs {
    pub const MAX_RECENT: usize = 12;

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        // Windows 的 rename 不允许覆盖已存在目标，先删再改名。
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path)
    }

    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorites.iter().any(|n| n == name)
    }

    pub fn toggle_favorite(&mut self, name: &str) {
        if let Some(i) = self.favorites.iter().position(|n| n == name) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(name.to_string());
        }
    }

    /// 记录一次使用：去重后置顶，上限 MAX_RECENT。
    pub fn touch(&mut self, name: &str) {
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(Self::MAX_RECENT);
    }

    /// 排序键：收藏（置顶）→ 最近使用 → 其余。
    /// 元组可直接用于 `sort_by_key`。
    pub fn rank(&self, name: &str) -> (u8, usize) {
        if let Some(i) = self.favorites.iter().position(|n| n == name) {
            (0, i)
        } else if let Some(i) = self.recent.iter().position(|n| n == name) {
            (1, i)
        } else {
            (2, 0)
        }
    }
}

/// 默认状态文件路径：exe 同目录。
pub fn default_state_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("orca-ui-state.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_and_touch() {
        let mut p = SchemePrefs::default();
        p.toggle_favorite("A");
        assert!(p.is_favorite("A"));
        p.toggle_favorite("A");
        assert!(!p.is_favorite("A"));

        p.touch("A");
        p.touch("B");
        p.touch("A");
        assert_eq!(p.recent, vec!["A", "B"]);
    }

    #[test]
    fn rank_puts_favorites_then_recent() {
        let mut p = SchemePrefs::default();
        p.touch("B");
        p.touch("A");
        p.toggle_favorite("A");
        assert_eq!(p.rank("A"), (0, 0));
        // recent = ["A","B"]，B 在其中的下标是 1
        assert_eq!(p.rank("B"), (1, 1));
        assert_eq!(p.rank("X"), (2, 0));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("orca-scheme-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("orca-ui-state.json");
        let mut p = SchemePrefs::default();
        p.toggle_favorite("Nord");
        p.touch("Nord");
        p.save(&path).unwrap();
        let loaded = SchemePrefs::load(&path);
        assert!(loaded.is_favorite("Nord"));
        assert_eq!(loaded.recent, vec!["Nord"]);

        // 覆盖写（模拟第二次保存）必须成功且读到新内容
        p.toggle_favorite("Nord");
        p.touch("Dracula (Official)");
        p.save(&path).unwrap();
        let loaded = SchemePrefs::load(&path);
        assert!(!loaded.is_favorite("Nord"));
        assert_eq!(loaded.recent, vec!["Dracula (Official)", "Nord"]);

        std::fs::remove_dir_all(&dir).ok();
    }
}
