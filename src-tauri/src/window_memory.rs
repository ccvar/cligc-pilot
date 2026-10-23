//! 记住主窗口的大小、位置和是否最大化：下次启动（包括开发时每次重新编译重启）恢复原样。
//!
//! 移动 / 缩放后约半秒写一次 `<数据目录>/window.json`（不只在退出时写：开发重启、强退时来不及走退出流程）。
//! 存的是逻辑像素，换了缩放比的屏幕也能对上；保存的位置已经不在任何屏幕上（外接屏拔了）时只恢复大小。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindow};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub maximized: bool,
}

const MIN_W: f64 = 820.0;
const MIN_H: f64 = 560.0;

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("window.json")
}

fn load(data_dir: &Path) -> Option<Saved> {
    let s: Saved = serde_json::from_slice(&std::fs::read(file(data_dir)).ok()?).ok()?;
    (s.width.is_finite() && s.height.is_finite() && s.x.is_finite() && s.y.is_finite()).then_some(s)
}

/// 两个矩形是否有足够的重叠（窗口顶部至少露出 80×40 才算还在这块屏幕上，能拖得回来）。
pub fn visible_on(s: &Saved, mx: f64, my: f64, mw: f64, mh: f64) -> bool {
    let ox = (s.x + s.width).min(mx + mw) - s.x.max(mx);
    let oy = (s.y + 40.0).min(my + mh) - s.y.max(my);
    ox >= 80.0 && oy >= 40.0
}

/// 启动时恢复（在窗口显示之前调用）。
pub fn restore(win: &WebviewWindow, data_dir: &Path) {
    let Some(s) = load(data_dir) else { return };
    let _ = win.set_size(LogicalSize::new(s.width.max(MIN_W), s.height.max(MIN_H)));
    let on_screen = win.available_monitors().unwrap_or_default().iter().any(|m| {
        let f = m.scale_factor();
        let (p, z) = (m.position(), m.size());
        visible_on(&s, p.x as f64 / f, p.y as f64 / f, z.width as f64 / f, z.height as f64 / f)
    });
    if on_screen {
        let _ = win.set_position(LogicalPosition::new(s.x, s.y));
    }
    if s.maximized {
        let _ = win.maximize();
    }
}

fn snapshot(win: &WebviewWindow) -> Option<Saved> {
    let f = win.scale_factor().ok()?;
    let maximized = win.is_maximized().unwrap_or(false);
    // 最大化 / 最小化时不覆盖「正常大小」，只记一下是否最大化
    if win.is_minimized().unwrap_or(false) {
        return None;
    }
    let (p, z) = (win.outer_position().ok()?, win.inner_size().ok()?);
    Some(Saved { x: p.x as f64 / f, y: p.y as f64 / f, width: z.width as f64 / f, height: z.height as f64 / f, maximized })
}

static GEN: AtomicU64 = AtomicU64::new(0);

/// 移动 / 缩放事件里调用：停下半秒后才写盘，拖动过程中不反复写。
pub fn schedule_save(win: &WebviewWindow) {
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let win = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if GEN.load(Ordering::SeqCst) != g {
            return;
        }
        let Ok(dir) = win.app_handle().path().app_data_dir() else { return };
        let Some(mut s) = snapshot(&win) else { return };
        if s.maximized {
            // 最大化时保留上次的正常大小和位置，下次先按它恢复再最大化
            if let Some(prev) = load(&dir) {
                s = Saved { maximized: true, ..prev };
            }
        }
        if let Ok(raw) = serde_json::to_vec(&s) {
            let _ = std::fs::write(file(&dir), raw);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_left_on_an_unplugged_screen_is_not_restored_there() {
        let s = Saved { x: 2000.0, y: 100.0, width: 1000.0, height: 700.0, maximized: false };
        assert!(!visible_on(&s, 0.0, 0.0, 1440.0 - 1000.0, 900.0), "只露出很窄一条不算");
        assert!(visible_on(&s, 1920.0, 0.0, 1920.0, 1080.0));
        assert!(!visible_on(&s, 0.0, 0.0, 1440.0, 900.0));
        let t = tempfile::tempdir().unwrap();
        std::fs::write(file(t.path()), "{\"x\":1,\"y\":2,\"width\":900,\"height\":600}").unwrap();
        assert_eq!(load(t.path()).unwrap().width, 900.0);
    }
}
