//! 接收文件名的清洗与落盘路径生成。
//!
//! 安全约束：绝不使用发送方给的路径直接落盘；只取最后一段，按当前平台规则清洗，
//! 重名加序号，先写 `.part` 再 rename，避免半截文件污染接收目录。

use std::path::{Path, PathBuf};

use localsend::util::filename::{self, Rules};

/// 清洗发送方给的文件名（可含路径）。
pub fn sanitize_file_name(raw: &str) -> String {
    // 只取最后一段：发送方可能给出 "../../evil.exe" 或 "C:\x\y.pdf"
    let last = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim();

    if last.is_empty() || last == "." || last == ".." {
        return "untitled".to_string();
    }

    let cleaned = filename::sanitize(last, Rules::current());
    let cleaned = cleaned.trim().to_string();

    // `.` / `..` 之类的相对名在部分规则下不会被改写，这里兜底
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "untitled".to_string();
    }
    cleaned
}

/// 在 `dir` 下生成不覆盖已有文件的路径：`a.pdf` → `a (1).pdf`。
pub fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());

    for n in 1..10_000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    // 极端情况：加时间戳兜底
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("{stem}-{stamp}"))
}

/// 传输中的临时文件名：`a.pdf` → `a.pdf.part`。
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download")
        .to_string();
    name.push_str(".part");
    final_path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_path_components() {
        assert_eq!(sanitize_file_name("../../evil.exe"), "evil.exe");
        assert_eq!(sanitize_file_name(r"C:\Windows\a.txt"), "a.txt");
        assert_eq!(sanitize_file_name("/etc/passwd"), "passwd");
    }

    #[test]
    fn handles_dot_names() {
        assert_eq!(sanitize_file_name(".."), "untitled");
        assert_eq!(sanitize_file_name("."), "untitled");
        assert_eq!(sanitize_file_name(""), "untitled");
    }

    #[test]
    fn keeps_chinese_names() {
        assert_eq!(sanitize_file_name("报告.pdf"), "报告.pdf");
    }

    #[test]
    fn part_path_appends_suffix() {
        let p = part_path(Path::new("C:/x/a.pdf"));
        assert!(p.to_string_lossy().ends_with("a.pdf.part"));
    }

    #[test]
    fn unique_path_avoids_overwrite() {
        let dir = std::env::temp_dir().join(format!("rtodo_path_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();

        let p = unique_path(&dir, "a.txt");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "a (1).txt");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
