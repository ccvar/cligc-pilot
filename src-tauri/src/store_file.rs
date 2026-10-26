//! 读 Pilot 自己的 JSON 状态文件（连接 / 会话 / 站点列表 / 队列…）。
//!
//! 以前都是 `from_slice(..).unwrap_or_default()`：文件坏了（手改、字段变了、写坏一个字节）
//! 就当成空列表，下一次保存把空列表写回去——**所有连接 / 会话 / 站点就此丢光**，
//! 钥匙串里的密钥也成了没人认领的孤儿。现在解析失败先把原文件改名留底
//! （`xxx.corrupt-<时间戳>.json`），再按空处理：用户的数据还在磁盘上，可以手工救回。
use std::path::Path;

/// 读 JSON 文件：不存在 → 默认值；解析失败 → 原文件留底后返回默认值。
pub fn read_or_quarantine<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(_) => return T::default(),
    };
    match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => {
            quarantine(path, &e.to_string());
            T::default()
        }
    }
}

/// 把解析不了的文件改名留底；改名失败就只记日志（宁可留着坏文件也不覆盖它）。
pub fn quarantine(path: &Path, why: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("state");
    let backup = path.with_file_name(format!("{stem}.corrupt-{ts}.json"));
    match std::fs::rename(path, &backup) {
        Ok(()) => eprintln!("[pilot] {} 解析失败（{why}），已改名留底为 {}", path.display(), backup.display()),
        Err(e) => eprintln!("[pilot] {} 解析失败（{why}），留底失败：{e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_file_is_kept_aside_instead_of_being_treated_as_empty_forever() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("sites.json");
        std::fs::write(&p, b"[{\"id\": ").unwrap();
        let v: Vec<u32> = read_or_quarantine(&p);
        assert!(v.is_empty());
        assert!(!p.exists(), "坏文件不再占着原名，之后保存不会覆盖它");
        let kept: Vec<_> = std::fs::read_dir(t.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(kept.iter().any(|n| n.starts_with("sites.corrupt-")), "{kept:?}");
        // 正常文件和不存在的文件照旧
        std::fs::write(&p, b"[1,2]").unwrap();
        assert_eq!(read_or_quarantine::<Vec<u32>>(&p), vec![1, 2]);
        assert!(read_or_quarantine::<Vec<u32>>(&t.path().join("none.json")).is_empty());
    }
}
