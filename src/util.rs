//! 文件名安全与重名处理。纯函数、可单测。

use std::path::{Path, PathBuf};

/// Windows 保留设备名（不含扩展名部分匹配）
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 清洗上传文件名：取 basename、替换非法字符、防路径穿越、防 Windows 保留名。
pub fn sanitize_filename(raw: &str) -> String {
    // 只取最后一段，路径分隔符本身也会被替换，杜绝穿越
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("file");
    let mut s: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    // Windows 不允许文件名以点或空格结尾
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let stem = s.split('.').next().unwrap_or("");
    if RESERVED.contains(&stem.to_uppercase().as_str()) {
        s = format!("_{s}");
    }
    if s.is_empty() {
        s = "file".into();
    }
    s.chars().take(150).collect()
}

/// 目标路径已存在时追加 (1)、(2)… 序号
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let ext = p
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    for i in 1..10000u32 {
        let named = if ext.is_empty() {
            format!("{stem}({i})")
        } else {
            format!("{stem}({i}).{ext}")
        };
        let candidate = dir.join(named);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{name}.{}", std::process::id()))
}

/// 下载路径安全校验：只允许单段文件名（拒绝子目录与穿越），且必须真实存在于 dir 内。
pub fn safe_join(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return None;
    }
    let p = dir.join(name);
    let dir_canon = dir.canonicalize().ok()?;
    let file_canon = p.canonicalize().ok()?;
    if file_canon.starts_with(&dir_canon) && file_canon.is_file() {
        Some(p)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_paths_and_forbidden_chars() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a/b\\c.txt"), "c.txt");
        assert_eq!(
            sanitize_filename("照片: 最终<版>.png"),
            "照片_ 最终_版_.png"
        );
        assert_eq!(sanitize_filename("bad|name?.mp4"), "bad_name_.mp4");
    }

    #[test]
    fn handles_dots_and_reserved_names() {
        assert_eq!(sanitize_filename(".."), "file");
        assert_eq!(sanitize_filename("..."), "file");
        assert_eq!(sanitize_filename("name...   "), "name");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("com1"), "_com1");
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename(""), "file");
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let dir = std::env::temp_dir();
        assert!(safe_join(&dir, "ok.txt").is_none()); // 不存在也是 None
        assert!(safe_join(&dir, "../x").is_none());
        assert!(safe_join(&dir, "a/b").is_none());
        assert!(safe_join(&dir, "a\\b").is_none());
        assert!(safe_join(&dir, "").is_none());
    }

    #[test]
    fn safe_join_accepts_real_file_under_dir() {
        let dir = std::env::temp_dir();
        let f = dir.join(format!("cmd-share-test-{}.txt", std::process::id()));
        std::fs::write(&f, "hi").unwrap();
        assert!(safe_join(&dir, f.file_name().unwrap().to_str().unwrap()).is_some());
        let _ = std::fs::remove_file(&f);
    }
}
