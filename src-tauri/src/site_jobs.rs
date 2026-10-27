//! 站点后台作业：部署、运营任务。每个站点同一时刻只跑一个作业。
//! 进度以 `site-job` 事件广播给所有窗口（作业快照），日志行走 `site-job-line`；
//! 窗口关了再开也能从 `snapshot` 取回当前状态，不依赖某个前端 Channel 活着。

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};

const LOG_TAIL: usize = 400;

#[derive(Clone, Serialize, Debug)]
pub struct StepState {
    pub key: String,
    pub label: String,
    /// pending | running | ok | failed | skipped
    pub status: String,
    pub detail: String,
}

/// env 里值为它的变量从子进程环境里去掉（而不是设成空串）。
pub const UNSET_ENV: &str = "\u{0}";

#[derive(Clone, Serialize, Debug)]
pub struct JobState {
    pub id: String,
    pub site_id: String,
    /// deploy | ops:<任务>
    pub kind: String,
    pub title: String,
    /// running | ok | failed | canceled
    pub status: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub steps: Vec<StepState>,
    pub log: VecDeque<String>,
    pub url: String,
    pub error: String,
}

#[derive(Clone, Serialize)]
struct LineEvent<'a> {
    site_id: &'a str,
    job_id: &'a str,
    line: &'a str,
}

struct Slot {
    state: JobState,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    /// 这个作业当前正在跑的 AI 会话（运营任务交给模型那一步）；取消作业时连它一起停。
    conv: Arc<Mutex<String>>,
}

#[derive(Clone, Default)]
pub struct Jobs {
    inner: Arc<Mutex<HashMap<String, Slot>>>,
}

/// 一个正在跑的作业的句柄：更新步骤、写日志、跑子进程。Drop 不会自动结束作业，必须调 finish。
#[derive(Clone)]
pub struct Job {
    jobs: Jobs,
    app: Option<tauri::AppHandle>,
    pub site_id: String,
    pub id: String,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    secrets: Arc<Mutex<Vec<String>>>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Jobs {
    /// 开一个作业；该站点已有作业在跑则拒绝。
    pub fn start(
        &self,
        app: Option<tauri::AppHandle>,
        site_id: &str,
        kind: &str,
        title: &str,
        steps: &[(&str, &str)],
    ) -> Result<Job, String> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.get(site_id) {
            if s.state.status == "running" {
                return Err(format!("这个站点正在{}，请等它结束", s.state.title));
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let pid = Arc::new(AtomicU32::new(0));
        let state = JobState {
            id: id.clone(),
            site_id: site_id.into(),
            kind: kind.into(),
            title: title.into(),
            status: "running".into(),
            started_at: now(),
            finished_at: 0,
            steps: steps
                .iter()
                .map(|(k, l)| StepState { key: (*k).into(), label: (*l).into(), status: "pending".into(), detail: String::new() })
                .collect(),
            log: VecDeque::new(),
            url: String::new(),
            error: String::new(),
        };
        g.insert(site_id.into(), Slot { state: state.clone(), cancel: cancel.clone(), pid: pid.clone(), conv: Default::default() });
        drop(g);
        let job = Job { jobs: self.clone(), app, site_id: site_id.into(), id, cancel, pid, secrets: Default::default() };
        job.broadcast(&state);
        Ok(job)
    }

    pub fn snapshot(&self, site_id: &str) -> Option<JobState> {
        self.inner.lock().unwrap().get(site_id).map(|s| s.state.clone())
    }

    /// 正在跑的作业数（所有站点）
    pub fn running_count(&self) -> usize {
        self.inner.lock().unwrap().values().filter(|s| s.state.status == "running").count()
    }

    pub fn running(&self, site_id: &str) -> bool {
        self.snapshot(site_id).is_some_and(|s| s.status == "running")
    }

    /// 用户取消：记下取消标记，停掉正在跑的子进程和 AI 回合。作业本身由它的协程发现标记后收尾（状态变 canceled）。
    pub fn cancel(&self, site_id: &str) -> bool {
        self.stop(site_id, true)
    }

    /// 只停掉正在跑的东西（子进程、AI 回合），不打取消标记：超时这类由 Pilot 自己叫停的情形，
    /// 作业要按「失败 + 原因」收尾，不是「已取消」。
    pub fn kill(&self, site_id: &str) -> bool {
        self.stop(site_id, false)
    }

    fn stop(&self, site_id: &str, mark: bool) -> bool {
        let conv = {
            let g = self.inner.lock().unwrap();
            let Some(s) = g.get(site_id) else { return false };
            if s.state.status != "running" {
                return false;
            }
            if mark {
                s.cancel.store(true, Ordering::SeqCst);
            }
            let pid = s.pid.load(Ordering::SeqCst);
            if pid != 0 {
                crate::agent::kill_tree(Some(pid));
            }
            let conv = s.conv.lock().unwrap().clone();
            conv
        };
        // 正在跑的 AI 回合：通过运行注册表停掉，不然要等它自己结束作业才算完、站点一直被占着
        if !conv.is_empty() {
            if let Some(app) = crate::APP.get() {
                use tauri::Manager;
                app.state::<crate::AppState>().runs.cancel(&conv);
            }
        }
        true
    }

    /// 这个站点正在跑的作业有没有被用户取消。
    pub fn is_canceled(&self, site_id: &str) -> bool {
        let g = self.inner.lock().unwrap();
        g.get(site_id).is_some_and(|s| s.state.status == "running" && s.cancel.load(Ordering::SeqCst))
    }

    /// 记下这个站点的作业当前交给了哪条 AI 会话（跑完传空串清掉）。
    pub fn set_conversation(&self, site_id: &str, conv_id: &str) {
        let g = self.inner.lock().unwrap();
        if let Some(s) = g.get(site_id) {
            if s.state.status == "running" {
                *s.conv.lock().unwrap() = conv_id.to_string();
            }
        }
    }
}

impl Job {
    fn broadcast(&self, state: &JobState) {
        if let Some(app) = &self.app {
            let _ = app.emit("site-job", state);
        }
    }

    fn with<F: FnOnce(&mut JobState)>(&self, f: F) {
        let snap = {
            let mut g = self.jobs.inner.lock().unwrap();
            let Some(slot) = g.get_mut(&self.site_id) else { return };
            if slot.state.id != self.id {
                return;
            }
            f(&mut slot.state);
            slot.state.clone()
        };
        self.broadcast(&snap);
    }

    /// 输出里要抹掉的值（Token、构建变量的值）。
    pub fn add_secret(&self, s: &str) {
        if s.len() >= 6 {
            self.secrets.lock().unwrap().push(s.to_string());
        }
    }

    fn scrub(&self, line: &str) -> String {
        let mut out = line.to_string();
        for s in self.secrets.lock().unwrap().iter() {
            out = out.replace(s.as_str(), "[已隐藏]");
        }
        out
    }

    pub fn canceled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn step(&self, key: &str, status: &str, detail: &str) {
        let detail = self.scrub(detail);
        self.with(|s| {
            if let Some(st) = s.steps.iter_mut().find(|x| x.key == key) {
                st.status = status.into();
                st.detail = detail;
            }
        });
    }

    pub fn line(&self, line: &str) {
        let line = self.scrub(line);
        let line: String = line.chars().take(2000).collect();
        {
            let mut g = self.jobs.inner.lock().unwrap();
            if let Some(slot) = g.get_mut(&self.site_id) {
                if slot.state.id == self.id {
                    slot.state.log.push_back(line.clone());
                    while slot.state.log.len() > LOG_TAIL {
                        slot.state.log.pop_front();
                    }
                }
            }
        }
        if let Some(app) = &self.app {
            let _ = app.emit("site-job-line", LineEvent { site_id: &self.site_id, job_id: &self.id, line: &line });
        }
    }

    pub fn set_url(&self, url: &str) {
        let url = url.to_string();
        self.with(|s| s.url = url);
    }

    pub fn finish(&self, result: &Result<(), String>) {
        let canceled = self.canceled();
        let err = result.as_ref().err().map(|e| self.scrub(e)).unwrap_or_default();
        self.with(|s| {
            s.status = if canceled { "canceled" } else if result.is_ok() { "ok" } else { "failed" }.into();
            s.error = if canceled { "已取消".into() } else { err };
            s.finished_at = now();
            for st in s.steps.iter_mut() {
                if st.status == "running" {
                    st.status = if result.is_ok() { "ok" } else { "failed" }.into();
                }
            }
        });
    }

    pub fn log_text(&self) -> String {
        self.jobs
            .snapshot(&self.site_id)
            .map(|s| s.log.into_iter().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    /// 跑一条用户配置的 shell 命令（构建 / 检查 / 安装），输出逐行进日志。失败返回错误（含最后几行输出）。
    pub async fn shell(&self, cmd: &str, dir: &Path, env: &[(String, String)], timeout: Duration) -> Result<(), String> {
        #[cfg(windows)]
        let (prog, args) = ("cmd".to_string(), vec!["/C".to_string(), cmd.to_string()]);
        #[cfg(not(windows))]
        let (prog, args) = ("sh".to_string(), vec!["-c".to_string(), cmd.to_string()]);
        self.line(&format!("$ {cmd}"));
        self.exec(&prog, &args, dir, env, timeout).await
    }

    /// 直接跑一个程序（不经 shell，参数原样传递）。
    pub async fn exec(&self, prog: &str, args: &[String], dir: &Path, env: &[(String, String)], timeout: Duration) -> Result<(), String> {
        if self.canceled() {
            return Err("已取消".into());
        }
        let mut c = tokio::process::Command::new(prog);
        c.args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("CI", "1")
            .env("FORCE_COLOR", "0")
            .env("NO_COLOR", "1")
            .kill_on_drop(true);
        for (k, v) in env {
            if v == UNSET_ENV {
                c.env_remove(k);
            } else {
                c.env(k, v);
            }
        }
        #[cfg(unix)]
        c.process_group(0);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        let mut child = c.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("找不到命令 {prog}")
            } else {
                format!("启动 {prog} 失败：{e}")
            }
        })?;
        self.pid.store(child.id().unwrap_or(0), Ordering::SeqCst);
        let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut err = BufReader::new(child.stderr.take().unwrap()).lines();
        let tail: Arc<Mutex<VecDeque<String>>> = Default::default();
        let push = |l: String, tail: &Arc<Mutex<VecDeque<String>>>| {
            let clean = strip_ansi(&l);
            self.line(&clean);
            let mut t = tail.lock().unwrap();
            t.push_back(self.scrub(&clean));
            if t.len() > 12 {
                t.pop_front();
            }
        };
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let (mut out_done, mut err_done) = (false, false);
        let status = loop {
            tokio::select! {
                l = out.next_line(), if !out_done => match l {
                    Ok(Some(l)) => push(l, &tail),
                    _ => out_done = true,
                },
                l = err.next_line(), if !err_done => match l {
                    Ok(Some(l)) => push(l, &tail),
                    _ => err_done = true,
                },
                s = child.wait(), if out_done && err_done => break s,
                _ = &mut deadline => {
                    crate::agent::kill_tree(child.id());
                    let _ = child.kill().await;
                    self.pid.store(0, Ordering::SeqCst);
                    return Err(format!("超过 {} 分钟没有结束，已停止", timeout.as_secs() / 60));
                }
            }
        };
        self.pid.store(0, Ordering::SeqCst);
        if self.canceled() {
            return Err("已取消".into());
        }
        let status = status.map_err(|e| format!("等待进程失败：{e}"))?;
        if status.success() {
            Ok(())
        } else {
            let t: Vec<String> = tail.lock().unwrap().iter().cloned().collect();
            Err(format!("退出码 {}\n{}", status.code().unwrap_or(-1), t.join("\n")))
        }
    }
}

/// 去掉终端颜色控制序列（wrangler / astro 即使设了 NO_COLOR 也偶尔会带）。
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_codes_are_removed() {
        assert_eq!(strip_ansi("\u{1b}[32m✓\u{1b}[0m done\r"), "✓ done");
    }

    #[tokio::test]
    async fn one_job_per_site_and_output_is_scrubbed() {
        let jobs = Jobs::default();
        let job = jobs.start(None, "s1", "deploy", "部署", &[("build", "构建")]).unwrap();
        assert!(jobs.start(None, "s1", "deploy", "部署", &[]).is_err());
        assert!(jobs.start(None, "s2", "deploy", "部署", &[]).is_ok());
        job.add_secret("supersecret");
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            job.shell("echo token=supersecret; echo oops >&2; exit 3", dir.path(), &[], Duration::from_secs(10))
                .await
                .unwrap_err();
            let log = job.log_text();
            assert!(log.contains("token=[已隐藏]"), "{log}");
            assert!(log.contains("oops"));
            assert!(!log.contains("supersecret"));
        }
        job.finish(&Ok(()));
        assert_eq!(jobs.snapshot("s1").unwrap().status, "ok");
        assert!(jobs.start(None, "s1", "deploy", "部署", &[]).is_ok());
    }

    #[test]
    fn cancel_marks_the_job_but_kill_does_not() {
        let jobs = Jobs::default();
        let job = jobs.start(None, "s1", "ops:work", "改动", &[]).unwrap();
        jobs.set_conversation("s1", "conv-1");
        assert!(!jobs.is_canceled("s1"));
        // 超时叫停：不打取消标记，作业按失败 + 原因收尾
        assert!(jobs.kill("s1"));
        assert!(!job.canceled() && !jobs.is_canceled("s1"));
        job.finish(&Err("超过 3 小时还没结束".into()));
        assert_eq!(jobs.snapshot("s1").unwrap().status, "failed");
        assert!(!jobs.cancel("s1"), "结束了的作业不能再取消");
        // 用户取消：打标记，收尾时状态是 canceled
        let job = jobs.start(None, "s1", "ops:work", "改动", &[]).unwrap();
        assert!(jobs.cancel("s1"));
        assert!(job.canceled() && jobs.is_canceled("s1"));
        job.finish(&Err("已取消".into()));
        assert_eq!(jobs.snapshot("s1").unwrap().status, "canceled");
    }
}
