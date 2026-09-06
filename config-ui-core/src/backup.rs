//! 保存前自动备份：`.lua.bak` 最新，`.lua.bak.1`…`.lua.bak.{N-1}` 更旧，滚动保留。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn bak_path(path: &Path, n: u32) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    if n == 0 {
        s.push(".bak");
    } else {
        s.push(format!(".bak.{n}"));
    }
    PathBuf::from(s)
}

fn shift(from: PathBuf, to: PathBuf) -> io::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    // Windows 上 rename 目标存在时报错，先删
    match fs::remove_file(&to) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::rename(from, to)
}

/// 对 `path` 做一次滚动备份；文件不存在或 `keep == 0` 时为空操作。
/// 原文件保持不动（调用方随后自行写入新内容）。
pub fn rolling_backup(path: &Path, keep: u32) -> io::Result<()> {
    if keep == 0 || !path.exists() {
        return Ok(());
    }
    for k in (1..keep.saturating_sub(1)).rev() {
        shift(bak_path(path, k), bak_path(path, k + 1))?;
    }
    if keep >= 2 {
        shift(bak_path(path, 0), bak_path(path, 1))?;
    }
    fs::copy(path, bak_path(path, 0))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static DIR_SEQ: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "config-ui-core-test-{}-{}",
            tag,
            DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn backup_copies_current_content() {
        let dir = temp_dir("copy");
        let cfg = dir.join("c.lua");
        write(&cfg, "v1\n");
        rolling_backup(&cfg, 3).unwrap();
        assert_eq!(read(&cfg.with_extension("lua.bak")), "v1\n");
        assert_eq!(read(&cfg), "v1\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rolls_keeping_n_backups() {
        let dir = temp_dir("roll");
        let cfg = dir.join("c.lua");
        for v in 1..=5u8 {
            write(&cfg, &format!("v{v}\n"));
            rolling_backup(&cfg, 3).unwrap();
        }
        assert_eq!(read(&cfg.with_extension("lua.bak")), "v5\n");
        assert_eq!(read(&cfg.with_extension("lua.bak.1")), "v4\n");
        assert_eq!(read(&cfg.with_extension("lua.bak.2")), "v3\n");
        assert!(!cfg.with_extension("lua.bak.3").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_source_is_noop() {
        let dir = temp_dir("noop");
        let cfg = dir.join("none.lua");
        rolling_backup(&cfg, 3).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keep_one_overwrites_single_slot() {
        let dir = temp_dir("one");
        let cfg = dir.join("c.lua");
        write(&cfg, "a\n");
        rolling_backup(&cfg, 1).unwrap();
        write(&cfg, "b\n");
        rolling_backup(&cfg, 1).unwrap();
        assert_eq!(read(&cfg.with_extension("lua.bak")), "b\n");
        assert!(!cfg.with_extension("lua.bak.1").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keep_zero_is_noop() {
        let dir = temp_dir("zero");
        let cfg = dir.join("c.lua");
        write(&cfg, "x\n");
        rolling_backup(&cfg, 0).unwrap();
        assert!(!cfg.with_extension("lua.bak").exists());
        fs::remove_dir_all(&dir).ok();
    }
}
