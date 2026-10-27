//! 后台运行：托管、监测这些定时任务只有 Pilot 开着才会跑。这里管三件事：
//! - 开机自动启动（macOS 用户级 LaunchAgent、Windows 注册表 Run、Linux ~/.config/autostart），开机时不弹窗口、只在菜单栏 / 托盘常驻；
//! - 不让电脑自动睡眠：运营作业在跑时一直挡着（作业跑一半睡过去会被打断），设置打开后托管期间一直挡着。
//!   只挡「闲置自动睡眠」，屏幕照样会关；合上笔记本盖子（没接电源和显示器）照样会睡；
//! - 记下后台停了多久：调度每 10 分钟心跳一次，两次心跳隔得太久就记一段「停了」（客户端没开 / 电脑睡着了），
//!   站点列表和托管页据此提醒。
//!
//! 状态存在数据目录的 background.json。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// 开机自动启动时带的参数：带它启动就不显示主窗口
pub const BACKGROUND_ARG: &str = "--background";
/// 两次心跳隔了这么久（调度每 10 分钟一次）才算「停过」
const GAP_MIN: u64 = 40 * 60;
/// 停了这么久以上才在列表上提醒
const NOTICE_MIN: u64 = 3600;
const DAY: u64 = 86_400;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Gap {
    pub from: u64,
    pub to: u64,
    /// closed（客户端没开）| asleep（客户端开着，但电脑睡着了）
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct State {
    #[serde(default)]
    keep_awake: bool,
    #[serde(default)]
    last_tick: u64,
    /// 最近 30 天停过的时段（旧的在前）
    #[serde(default)]
    gaps: Vec<Gap>,
    /// 提醒关掉时那一段的结束时间：同一段不再提醒
    #[serde(default)]
    seen_gap: u64,
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("background.json")
}
fn load(data_dir: &Path) -> State {
    crate::store_file::read_or_quarantine(&path(data_dir))
}
fn save(data_dir: &Path, s: &State) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let tmp = path(data_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, path(data_dir));
        }
    }
}

fn now() -> u64 {
    crate::now_secs()
}

/// 这个进程里还没心跳过：第一次心跳前的空档是「客户端没开」
static FIRST_TICK: AtomicBool = AtomicBool::new(true);

/// 把一次心跳记进状态：离上次太久就记一段停了的时段。抽出来便于测试。
fn beat(s: &mut State, now: u64, first: bool) {
    if s.last_tick > 0 && now.saturating_sub(s.last_tick) >= GAP_MIN {
        s.gaps.push(Gap { from: s.last_tick, to: now, kind: if first { "closed" } else { "asleep" }.into() });
    }
    s.last_tick = now;
    s.gaps.retain(|g| g.to + 30 * DAY >= now);
}

/// 调度每一轮调用：心跳、按设置挡 / 不挡睡眠。managed_active＝有托管中的站点（没暂停）。
pub fn tick(data_dir: &Path, managed_active: bool) {
    let mut s = load(data_dir);
    beat(&mut s, now(), FIRST_TICK.swap(false, Ordering::SeqCst));
    save(data_dir, &s);
    WANT.store(s.keep_awake && managed_active, Ordering::SeqCst);
    reconcile();
}

#[derive(Serialize)]
pub struct Status {
    pub launch_at_login: bool,
    /// 这台电脑能不能设开机启动 / 挡睡眠（macOS、Windows 能；其他系统只有开机启动）
    pub can_keep_awake: bool,
    pub keep_awake: bool,
    /// 现在正挡着睡眠（作业在跑，或设置打开且有托管站点）
    pub awake_now: bool,
    /// 最近 30 天停过的时段（新的在前，最多 20 段）
    pub gaps: Vec<Gap>,
    /// 最近 7 天一共停了多少秒
    pub stopped_week: u64,
    /// 要在列表上提醒的那一段：最近 24 小时里结束、停了 1 小时以上、还没关掉提醒
    pub notice: Option<Gap>,
    /// 开机启动指向的程序（开发版会指向调试程序，提示一下）
    pub exe: String,
}

pub fn status(data_dir: &Path) -> Status {
    let s = load(data_dir);
    let t = now();
    let mut gaps = s.gaps.clone();
    gaps.reverse();
    let stopped_week = s.gaps.iter().map(|g| g.to.min(t).saturating_sub(g.from.max(t.saturating_sub(7 * DAY)))).sum();
    let notice = gaps.iter().find(|g| g.to + DAY >= t && g.to - g.from >= NOTICE_MIN && g.to > s.seen_gap).cloned();
    gaps.truncate(20);
    Status {
        launch_at_login: autostart::enabled(),
        can_keep_awake: cfg!(any(target_os = "macos", target_os = "windows")),
        keep_awake: s.keep_awake,
        awake_now: HELD.lock().map(|h| h.is_some()).unwrap_or(false),
        gaps,
        stopped_week,
        notice,
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

pub fn set_keep_awake(data_dir: &Path, on: bool, managed_active: bool) {
    let mut s = load(data_dir);
    s.keep_awake = on;
    save(data_dir, &s);
    WANT.store(on && managed_active, Ordering::SeqCst);
    reconcile();
}

pub fn dismiss_notice(data_dir: &Path) {
    let mut s = load(data_dir);
    s.seen_gap = s.gaps.last().map(|g| g.to).unwrap_or(0);
    save(data_dir, &s);
}

// ───────────────────────── 命令 ─────────────────────────

fn managed_active(data_dir: &Path) -> bool {
    let m = crate::managed::load(data_dir);
    !m.paused && m.groups.iter().any(|g| g.sites.iter().any(|s| g.active_for(s)))
}

#[tauri::command]
pub fn background_status(state: tauri::State<'_, crate::AppState>) -> Status {
    status(&state.data_dir)
}

#[tauri::command]
pub fn background_set(state: tauri::State<'_, crate::AppState>, launch_at_login: Option<bool>, keep_awake: Option<bool>) -> Result<Status, String> {
    if let Some(on) = launch_at_login {
        autostart::set(on)?;
    }
    if let Some(on) = keep_awake {
        set_keep_awake(&state.data_dir, on, managed_active(&state.data_dir));
    }
    Ok(status(&state.data_dir))
}

#[tauri::command]
pub fn background_dismiss_notice(state: tauri::State<'_, crate::AppState>) {
    dismiss_notice(&state.data_dir);
}

// ───────────────────────── 挡睡眠 ─────────────────────────

/// 设置打开且有托管站点
static WANT: AtomicBool = AtomicBool::new(false);
/// 正在跑的运营作业数
static JOBS: AtomicUsize = AtomicUsize::new(0);
static HELD: Mutex<Option<Holder>> = Mutex::new(None);

/// 运营作业持有：活着的时候不让电脑自动睡眠
pub struct AwakeGuard;
impl Drop for AwakeGuard {
    fn drop(&mut self) {
        JOBS.fetch_sub(1, Ordering::SeqCst);
        reconcile();
    }
}
pub fn job_guard() -> AwakeGuard {
    JOBS.fetch_add(1, Ordering::SeqCst);
    reconcile();
    AwakeGuard
}

fn reconcile() {
    let want = WANT.load(Ordering::SeqCst) || JOBS.load(Ordering::SeqCst) > 0;
    let Ok(mut held) = HELD.lock() else { return };
    match (want, held.is_some()) {
        (true, false) => *held = Holder::start(),
        (false, true) => {
            if let Some(h) = held.take() {
                h.stop();
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
struct Holder(std::process::Child);
#[cfg(target_os = "macos")]
impl Holder {
    /// `caffeinate -i`：挡闲置睡眠（屏幕照常关）；`-w`：Pilot 退出（包括崩溃）它就跟着退
    fn start() -> Option<Self> {
        std::process::Command::new("/usr/bin/caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
            .map(Holder)
    }
    fn stop(mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(target_os = "windows")]
struct Holder(std::sync::mpsc::Sender<()>);
#[cfg(target_os = "windows")]
impl Holder {
    /// SetThreadExecutionState 按线程生效：开一个线程挡着，收到停止就恢复
    fn start() -> Option<Self> {
        use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            let _ = rx.recv();
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        });
        Some(Holder(tx))
    }
    fn stop(self) {
        let _ = self.0.send(());
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct Holder;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Holder {
    fn start() -> Option<Self> {
        None
    }
    fn stop(self) {}
}

// ───────────────────────── 开机启动 ─────────────────────────

pub mod autostart {
    #[cfg(target_os = "macos")]
    fn plist() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Library/LaunchAgents/com.cligc.pilot.autostart.plist"))
    }

    /// 装好的 .app：`open -g -a`（已经开着就什么都不做，不抢前台）；开发版没有 .app，直接跑程序
    #[cfg(target_os = "macos")]
    fn program_args() -> Result<Vec<String>, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let app = exe.ancestors().find(|p| p.extension().is_some_and(|x| x == "app"));
        Ok(match app {
            Some(app) => vec!["/usr/bin/open".into(), "-g".into(), "-a".into(), app.to_string_lossy().into_owned(), "--args".into(), super::BACKGROUND_ARG.into()],
            None => vec![exe.to_string_lossy().into_owned(), super::BACKGROUND_ARG.into()],
        })
    }

    #[cfg(target_os = "macos")]
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    #[cfg(target_os = "macos")]
    pub fn enabled() -> bool {
        plist().is_some_and(|p| p.is_file())
    }

    #[cfg(target_os = "macos")]
    pub fn set(on: bool) -> Result<(), String> {
        let p = plist().ok_or("找不到用户目录")?;
        if !on {
            return match std::fs::remove_file(&p) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("关不掉开机启动：{e}")),
            };
        }
        let args: String = program_args()?.iter().map(|a| format!("\n    <string>{}</string>", xml_escape(a))).collect();
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>com.cligc.pilot.autostart</string>\n  <key>ProgramArguments</key>\n  <array>{args}\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n"
        );
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("设不了开机启动：{e}"))?;
        }
        std::fs::write(&p, body).map_err(|e| format!("设不了开机启动：{e}"))
    }

    #[cfg(target_os = "windows")]
    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    #[cfg(target_os = "windows")]
    fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output()
    }
    #[cfg(target_os = "windows")]
    pub fn enabled() -> bool {
        reg(&["query", RUN_KEY, "/v", "CligcPilot"]).is_ok_and(|o| o.status.success())
    }
    #[cfg(target_os = "windows")]
    pub fn set(on: bool) -> Result<(), String> {
        let out = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let cmd = format!("\"{}\" {}", exe.display(), super::BACKGROUND_ARG);
            reg(&["add", RUN_KEY, "/v", "CligcPilot", "/t", "REG_SZ", "/d", &cmd, "/f"])
        } else {
            if !enabled() {
                return Ok(());
            }
            reg(&["delete", RUN_KEY, "/v", "CligcPilot", "/f"])
        }
        .map_err(|e| format!("改不了开机启动：{e}"))?;
        if out.status.success() { Ok(()) } else { Err(format!("改不了开机启动：{}", String::from_utf8_lossy(&out.stderr).trim())) }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn desktop() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config/autostart/cligc-pilot.desktop"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn enabled() -> bool {
        desktop().is_some_and(|p| p.is_file())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn set(on: bool) -> Result<(), String> {
        let p = desktop().ok_or("找不到用户目录")?;
        if !on {
            let _ = std::fs::remove_file(&p);
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, format!("[Desktop Entry]\nType=Application\nName=Cligc Pilot\nExec=\"{}\" {}\nX-GNOME-Autostart-enabled=true\n", exe.display(), super::BACKGROUND_ARG)).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_silence_between_beats_is_recorded_as_a_gap() {
        let mut s = State::default();
        beat(&mut s, 1_000_000, true); // 第一次：没有上次，不算
        assert!(s.gaps.is_empty());
        beat(&mut s, 1_000_600, false); // 正常的 10 分钟
        assert!(s.gaps.is_empty());
        beat(&mut s, 1_000_600 + 3 * 3600, false); // 睡了 3 小时
        assert_eq!(s.gaps, vec![Gap { from: 1_000_600, to: 1_000_600 + 3 * 3600, kind: "asleep".into() }]);
        beat(&mut s, 1_000_600 + 13 * 3600, true); // 重启前没开 10 小时
        assert_eq!(s.gaps[1].kind, "closed");
        // 30 天前的时段会清掉
        beat(&mut s, 1_000_600 + 40 * DAY, true);
        assert_eq!(s.gaps.len(), 1);
    }
}
