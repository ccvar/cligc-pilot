//! macOS 菜单栏图标：有作业在跑时「呼吸」（两道斜杠的透明度在 100% 和 40% 之间缓慢起伏，1.6 秒一个周期），
//! 悬停提示列出正在跑的作业；没作业时恢复静止。定时器只在有作业时转，每秒 10 帧。
//! 图标是单色模板图（见 lib.rs setup_tray），只改透明度就能保持系统自动染色。
//! Windows / Linux 的托盘用彩色应用图标，不做这个处理（只更新悬停提示）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "main";
/// 一个呼吸周期的帧数（每帧 100 毫秒）
const FRAMES: usize = 16;
const FRAME_MS: u64 = 100;
/// 最淡的时候留多少
const FLOOR: f32 = 0.4;

static ANIMATING: AtomicBool = AtomicBool::new(false);
static LAST_TIP: Mutex<String> = Mutex::new(String::new());
static GLYPHS: OnceLock<(tauri::image::Image<'static>, Vec<tauri::image::Image<'static>>)> = OnceLock::new();

/// 启动时算好所有帧：基础图 + 按透明度缩过的各帧（macOS 的托盘建好后调一次）
pub fn prepare(glyph: &tauri::image::Image<'_>) {
    let base = own(glyph);
    let frames = (0..FRAMES).map(|i| scaled(glyph, breath(i))).collect();
    let _ = GLYPHS.set((base, frames));
}

/// 第 i 帧的不透明度：余弦起伏，从 1 落到 FLOOR 再回来
pub fn breath(i: usize) -> f32 {
    let t = i as f32 / FRAMES as f32 * std::f32::consts::TAU;
    FLOOR + (1.0 - FLOOR) * (0.5 + 0.5 * t.cos())
}

fn own(img: &tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    tauri::image::Image::new_owned(img.rgba().to_vec(), img.width(), img.height())
}

/// 只缩 alpha 通道：模板图的颜色由系统定，透明度决定深浅
pub fn scaled(img: &tauri::image::Image<'_>, k: f32) -> tauri::image::Image<'static> {
    let mut buf = img.rgba().to_vec();
    for px in buf.chunks_exact_mut(4) {
        px[3] = (px[3] as f32 * k).round().clamp(0.0, 255.0) as u8;
    }
    tauri::image::Image::new_owned(buf, img.width(), img.height())
}

/// 正在跑的作业：(站点名, 作业标题)
fn running(app: &AppHandle) -> Vec<(String, String)> {
    let st = app.state::<crate::AppState>();
    st.site_jobs.running_titles().into_iter().map(|(id, title)| (st.sites.get(&id).map(|s| s.name).unwrap_or(id), title)).collect()
}

/// 悬停提示的文字
pub fn tooltip(running: &[(String, String)]) -> String {
    if running.is_empty() {
        return "Cligc Pilot".into();
    }
    let mut lines = vec![format!("Cligc Pilot · 正在运行 {} 个作业", running.len())];
    lines.extend(running.iter().map(|(s, t)| format!("{s} · {t}")));
    lines.join("\n")
}

/// 作业状态有变化时调：更新悬停提示；有作业在跑且还没在动就开始呼吸。调用很频繁（每条日志都会），所以先比一下
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let jobs = running(app);
    let tip = tooltip(&jobs);
    {
        let mut last = LAST_TIP.lock().unwrap();
        if *last != tip {
            *last = tip.clone();
            let _ = tray.set_tooltip(Some(&tip));
        }
    }
    if jobs.is_empty() || GLYPHS.get().is_none() {
        return;
    }
    if ANIMATING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some((base, frames)) = GLYPHS.get() else { return };
        let mut i = 0usize;
        loop {
            let Some(tray) = app.tray_by_id(TRAY_ID) else { break };
            if app.state::<crate::AppState>().site_jobs.running_titles().is_empty() {
                let _ = tray.set_icon(Some(base.clone()));
                let _ = tray.set_icon_as_template(true);
                break;
            }
            let _ = tray.set_icon(Some(frames[i % FRAMES].clone()));
            let _ = tray.set_icon_as_template(true);
            i += 1;
            tokio::time::sleep(std::time::Duration::from_millis(FRAME_MS)).await;
        }
        ANIMATING.store(false, Ordering::SeqCst);
        // 停下来的一瞬间又起了新作业：再看一眼
        refresh(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breath_goes_from_full_down_to_floor_and_back() {
        assert!((breath(0) - 1.0).abs() < 1e-6);
        assert!((breath(FRAMES / 2) - FLOOR).abs() < 1e-6);
        assert!(breath(FRAMES / 4) < breath(0) && breath(FRAMES / 4) > breath(FRAMES / 2));
        assert!((breath(FRAMES) - 1.0).abs() < 1e-6, "一圈回到原点");
    }

    #[test]
    fn only_alpha_is_scaled() {
        let img = tauri::image::Image::new(&[0, 0, 0, 255, 0, 0, 0, 100, 9, 9, 9, 0], 3, 1);
        let out = scaled(&img, 0.5);
        assert_eq!(out.rgba(), &[0, 0, 0, 128, 0, 0, 0, 50, 9, 9, 9, 0]);
        assert_eq!((out.width(), out.height()), (3, 1));
    }

    #[test]
    fn tooltip_lists_running_jobs() {
        assert_eq!(tooltip(&[]), "Cligc Pilot");
        assert_eq!(tooltip(&[("gategas.com".into(), "站点体检".into()), ("okexc.com".into(), "机会扫描".into())]), "Cligc Pilot · 正在运行 2 个作业\ngategas.com · 站点体检\nokexc.com · 机会扫描");
    }
}
