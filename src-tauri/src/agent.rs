//! 对话轮次执行器：把一轮用户消息交给本地 claude / codex 跑，
//! 边跑边把助手文本增量、工具调用经 Channel 推给前端，收尾返回本轮结果。
//! 多轮机制（已在真机验证）：
//!   claude —— 首轮 `--session-id <uuid>`，续轮 `--resume <uuid>`；
//!   codex  —— 优先复用常驻 app-server 的 thread/turn；实验接口不可用时自动降级到
//!             `exec` / `exec resume <id>`。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::convo::ToolCall;
use crate::keychain;
use crate::conns::Connection;
use crate::permit::{self, PermMode};

/// Codex CLI 启动前的本地兼容性预检。
///
/// Codex 的 models_cache.json 由 CLI 自己维护，旧版 CLI 读取新版缓存时会在真正
/// 启动前报 `unknown variant max`，导致用户看到“对话不可用”。这里不联网升级、不
/// 修改 config，只在发现缓存明确来自更新版本，或旧版无法识别的 effort 值时，把缓存
/// 改名为可恢复的备份，让当前 CLI 在下一次启动时重新生成。
pub(crate) async fn prepare_codex_cache() -> Result<(), String> {
    let bin = crate::brains::resolve_bin("codex");
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(&bin).arg("--version").output(),
    )
    .await
    .map_err(|_| "检测 Codex 版本超时".to_string())?
    .map_err(|e| format!("检测 Codex 版本失败：{e}"))?;
    if !output.status.success() {
        return Err("无法读取 Codex 版本，请先安装或修复 Codex CLI".into());
    }
    let version_text = String::from_utf8_lossy(&output.stdout);
    let Some(cli_version) = parse_codex_version(&version_text) else {
        return Err("无法识别 Codex CLI 版本，请先更新 Codex CLI".into());
    };

    let Some(home) = codex_home() else {
        return Ok(());
    };
    let cache = home.join("models_cache.json");
    if !cache.is_file() {
        return Ok(());
    }
    let raw = fs::read(&cache).map_err(|e| format!("读取 Codex 模型缓存失败：{e}"))?;
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return quarantine_codex_cache(&cache);
    };

    let cache_version = value
        .get("client_version")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_version_triplet);
    let cache_is_newer = cache_version.is_some_and(|v| v > cli_version);
    // 0.144 及更早版本不认识 max/ultra；这些值出现在新版缓存时会直接阻断启动。
    let old_cli_has_new_effort = cli_version < (0, 145, 0) && contains_new_effort(&value);
    // 0.144 app-server 比 exec 更严格：缺少该字段会在每次 thread/start 都解析失败并
    // 重新刷新缓存。隔离一次让 CLI 生成完整缓存，避免每轮重复付出这段固定开销。
    let app_server_cache_incomplete = cli_version >= (0, 144, 0)
        && codex_models_missing_field(&value, "supports_reasoning_summaries");
    if cache_is_newer || old_cli_has_new_effort || app_server_cache_incomplete {
        quarantine_codex_cache(&cache)?;
    }
    Ok(())
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".codex"))
        })
}

fn parse_codex_version(text: &str) -> Option<(u32, u32, u32)> {
    text.split_whitespace().find_map(parse_version_triplet)
}

pub(crate) fn parse_version_triplet(text: &str) -> Option<(u32, u32, u32)> {
    let trimmed = text.trim_start_matches(['v', 'V']);
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .and_then(|part| part.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

fn contains_new_effort(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s == "max" || s == "ultra",
        serde_json::Value::Array(items) => items.iter().any(contains_new_effort),
        serde_json::Value::Object(items) => items.values().any(contains_new_effort),
        _ => false,
    }
}

fn codex_models_missing_field(value: &serde_json::Value, field: &str) -> bool {
    value
        .get("models")
        .and_then(serde_json::Value::as_array)
        .filter(|models| !models.is_empty())
        .is_some_and(|models| models.iter().any(|model| model.get(field).is_none()))
}

fn quarantine_codex_cache(cache: &std::path::Path) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let backup = cache.with_file_name(format!("models_cache.json.bak-{stamp}"));
    fs::rename(cache, &backup)
        .map_err(|e| format!("Codex 模型缓存与当前 CLI 不兼容，自动修复失败：{e}"))
}

/// 本轮的权限设置：档位 + 钩子资产落盘位置（claude 用）。
struct PermSpec {
    mode: PermMode,
    conv_id: String,
    gen_dir: PathBuf,
    pending_dir: PathBuf,
    /// AI 桥脚本路径：钩子要靠它认出「桥命令」并放行（桥自己会弹卡），见 permit::is_bridge_cmd。
    ssh_js: PathBuf,
    /// fast 模式（Claude 的 `fastMode` settings 键）。跟着 `--settings` 一起下去，
    /// 因为那个槽只有一个、必须和权限钩子合并 —— 见 permit::claude_flags。
    fast: bool,
}

/// Claude 的多行系统提示不能直接作为 `.cmd/.bat` 参数传递（Windows 会拒绝 CR/LF）。
/// 文件必须活到子进程完全退出后再删，避免 Claude 尚未打开它就被提前清理。
struct ScopedPromptFile {
    path: PathBuf,
}

impl ScopedPromptFile {
    fn create(dir: &std::path::Path, content: &str) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("创建 Claude 提示词目录失败：{e}"))?;
        let path = dir.join(format!("claude-system-prompt-{}.txt", uuid::Uuid::new_v4()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|e| format!("创建 Claude 系统提示词失败：{e}"))?;
        if let Err(e) = std::io::Write::write_all(&mut file, content.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(format!("写入 Claude 系统提示词失败：{e}"));
        }
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for ScopedPromptFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    Delta {
        text: String,
    },
    /// Restore the cumulative text before a discarded API response. Previous
    /// completed replies and tool activity are retained by the live UI.
    TextReset {
        text: String,
    },
    Tool {
        label: String,
        detail: String,
    },
    /// 仅供本轮实时界面消费的执行活动。它把三家 CLI/ACP 各自的工具生命周期
    /// 归一成同一套状态，不写入 Conversation，也不替模型编造业务进度。
    Activity {
        activity_id: String,
        label: String,
        detail: String,
        error: String,
        status: String,
        /// 三家执行器共用的语义类别。没有真实类别时为空，不由界面猜测。
        kind: String,
        /// 执行器明确给出的阶段文案；为空时界面回退到 label/detail。
        phase: String,
        /// 只有执行器给出真实总量时才填写，避免把工具调用次数冒充业务进度。
        current: Option<u64>,
        total: Option<u64>,
    },
    ContextCompacted {
        pre_tokens: u64,
    },
    Done {
        ok: bool,
        error: String,
    },
}

pub struct TurnResult {
    pub ok: bool,
    pub text: String,
    pub tools: Vec<ToolCall>,
    pub error: String,
    pub session_ref: String,
    /// 本轮 token 用量（从 CLI 流里抽出）；用于「会话大小/累计用量」。拿不到则 None。
    pub usage: Option<TurnUsage>,
    /// 本轮因订阅额度/限流失败：Some(恢复时间戳秒)，拿不到恢复时间为 Some(0)；非限额错误 None。
    pub limit_reset: Option<i64>,
}

/// 识别「订阅额度耗尽/限流」类报错。claude 触顶输出形如
/// "Claude AI usage limit reached|1736600400"（重置时间戳可选）；codex/OpenAI 是
/// 429 / usage limit / rate limit 一族措辞；grok 欠费/触顶是 402 Payment Required +
/// "personal-team-blocked:spending-limit: You have run out …"（实测 0.2.101）。
/// 只对失败回合的错误材料调用（正文里聊到 "rate limit" 不会误判——ok 回合不进这里）。
pub(crate) fn detect_usage_limit(material: &str) -> Option<i64> {
    let low = material.to_ascii_lowercase();
    let hit = [
        "usage limit",
        "rate limit",
        "limit reached",
        "too many requests",
        "insufficient_quota",
        "quota exceeded",
        "resets at",
        "spending-limit",
        "payment required",
    ]
    .iter()
    .any(|p| low.contains(p))
        || low.contains("(429")
        || low.contains(" 429")
        || low.contains("status 429")
        || low.contains("429 ");
    if !hit {
        return None;
    }
    // claude 风格：…limit reached|<epoch秒>
    if let Some(i) = low.find("limit reached|") {
        let rest = &material[i + "limit reached|".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(ts) = digits.parse::<i64>() {
            // 秒级时间戳合理区间（2020–2100），毫秒级的除以 1000
            if (1_577_836_800..4_102_444_800).contains(&ts) {
                return Some(ts);
            }
            if (1_577_836_800_000..4_102_444_800_000).contains(&ts) {
                return Some(ts / 1000);
            }
        }
    }
    Some(0)
}

/// 一轮的 token 用量。input+cache_read+cache_create ≈ 模型这轮读入的整段上下文（＝当前会话大小）。
#[derive(Clone, Default)]
pub struct TurnUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

// ---- 取消注册表（按 turn id）----

#[derive(Clone, Default)]
pub struct RunRegistry {
    inner: Arc<Mutex<HashMap<String, (Arc<AtomicBool>, Option<tokio::sync::oneshot::Sender<()>>)>>>,
}

impl RunRegistry {
    pub(crate) fn register(
        &self,
        id: &str,
    ) -> (Arc<AtomicBool>, tokio::sync::oneshot::Receiver<()>) {
        let canceled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.inner
            .lock()
            .unwrap()
            .insert(id.to_string(), (canceled.clone(), Some(tx)));
        (canceled, rx)
    }
    pub(crate) fn unregister(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
    pub fn cancel(&self, id: &str) -> bool {
        if let Some((flag, tx)) = self.inner.lock().unwrap().get_mut(id) {
            flag.store(true, Ordering::SeqCst);
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
            true
        } else {
            false
        }
    }
    pub(crate) fn is_canceled(&self, id: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|(f, _)| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

// ---- Codex app-server（按连接环境常驻，exec 仍是兼容降级路径）----

type RpcReply = Result<serde_json::Value, String>;

#[derive(Clone)]
struct CodexTurnSink {
    channel: Channel<TurnEvent>,
    collect: Arc<Mutex<Collect>>,
    completed: Arc<Mutex<Option<oneshot::Sender<serde_json::Value>>>>,
    streamed_messages: Arc<Mutex<HashSet<String>>>,
}

struct CodexAppServer {
    tx: mpsc::UnboundedSender<serde_json::Value>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<RpcReply>>>>,
    sinks: Arc<Mutex<HashMap<String, CodexTurnSink>>>,
    loaded_threads: Mutex<HashSet<String>>,
    next_id: AtomicU64,
    dead: Arc<AtomicBool>,
    // 持有 Child 才能保证 Pilot 生命周期内复用同一个 app-server；Drop 会终止它。
    _child: Arc<tokio::sync::Mutex<Child>>,
}

static CODEX_APP_SERVERS: OnceLock<tokio::sync::Mutex<HashMap<String, Arc<CodexAppServer>>>> =
    OnceLock::new();
// app-server 的启动和环境切换必须串行。短时解锁会改变子进程环境；若新进程在
// Windows 旧进程真正退出前 resume 同一 thread，Codex 会拒绝第二个 writer。
static CODEX_APP_SERVER_TRANSITION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

enum CodexAppServerAttempt {
    Completed(TurnResult),
    Unavailable(String),
    ThreadBusy(String),
}

impl CodexAppServer {
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: std::time::Duration,
    ) -> RpcReply {
        if self.dead.load(Ordering::SeqCst) {
            return Err("Codex app-server 已退出".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if self
            .tx
            .send(serde_json::json!({"id": id, "method": method, "params": params}))
            .is_err()
        {
            self.pending.lock().unwrap().remove(&id);
            return Err("Codex app-server 写入通道已关闭".into());
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(format!("Codex app-server 请求中断：{method}")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("Codex app-server 请求超时：{method}"))
            }
        }
    }

    fn notify(&self, method: &str, params: Option<serde_json::Value>) -> Result<(), String> {
        let mut message = serde_json::json!({"method": method});
        if let Some(params) = params {
            message["params"] = params;
        }
        self.tx
            .send(message)
            .map_err(|_| "Codex app-server 写入通道已关闭".to_string())
    }

    fn attach(&self, thread_id: &str, sink: CodexTurnSink) {
        self.sinks
            .lock()
            .unwrap()
            .insert(thread_id.to_string(), sink);
    }

    fn detach(&self, thread_id: &str) {
        self.sinks.lock().unwrap().remove(thread_id);
    }

    async fn shutdown(&self) {
        self.dead.store(true, Ordering::SeqCst);
        let mut child = self._child.lock().await;
        kill_tree(child.id());
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

fn codex_server_environment_key(
    conn: &Connection,
    work_dir: &str,
    api_key: &str,
    lease: Option<&crate::bridge::Lease>,
) -> String {
    use sha2::{Digest, Sha256};
    let work_hash = {
        let digest = Sha256::digest(work_dir.as_bytes());
        format!("{:x}", digest)[..16].to_string()
    };
    let mut hasher = Sha256::new();
    hasher.update(conn.id.as_bytes());
    hasher.update([0]);
    hasher.update(work_dir.as_bytes());
    hasher.update([0]);
    hasher.update(conn.kind.as_bytes());
    hasher.update([0]);
    hasher.update(api_key.as_bytes());
    if let Some(lease) = lease {
        let mut env = lease.env();
        env.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in env {
            hasher.update([0]);
            hasher.update(key.as_bytes());
            hasher.update([0]);
            hasher.update(value.as_bytes());
        }
    }
    format!("{}:{work_hash}:{:x}", conn.id, hasher.finalize())
}

fn codex_initialize_params() -> serde_json::Value {
    serde_json::json!({
        "clientInfo": {
            "name": "cligc-pilot",
            "title": "Cligc Pilot",
            "version": env!("CARGO_PKG_VERSION")
        },
        // thread/resume.excludeTurns is deliberately used below so a persistent
        // app-server can reattach without replaying a very large rollout into Pilot.
        // Codex rejects that field unless the client negotiates experimentalApi.
        "capabilities": {
            "experimentalApi": true
        }
    })
}

async fn codex_app_server(
    conn: &Connection,
    work_dir: &str,
    api_key: &str,
    lease: Option<&crate::bridge::Lease>,
) -> Result<Arc<CodexAppServer>, String> {
    let key = codex_server_environment_key(conn, work_dir, api_key, lease);
    let scope = key
        .rsplit_once(':')
        .map(|(scope, _)| format!("{scope}:"))
        .unwrap_or_default();
    let transition = CODEX_APP_SERVER_TRANSITION.get_or_init(|| tokio::sync::Mutex::new(()));
    let _transition_guard = transition.lock().await;
    let servers = CODEX_APP_SERVERS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));

    // 环境（如 SSH 桥租约）变化时，先把同连接、同工作目录下已经空闲的
    // 旧服务完整关停并 wait，再启动新服务。仅从 HashMap 移除会让 kill_on_drop 与
    // 新 thread/resume 竞速，在 Windows 上尤其容易留下短暂的 active writer。
    let stale_servers = {
        let mut guard = servers.lock().await;
        let stale_keys = guard
            .iter()
            .filter_map(|(existing_key, server)| {
                (existing_key != &key
                    && existing_key.starts_with(&scope)
                    && server.sinks.lock().unwrap().is_empty())
                .then_some(existing_key.clone())
            })
            .collect::<Vec<_>>();
        stale_keys
            .into_iter()
            .filter_map(|stale_key| guard.remove(&stale_key))
            .collect::<Vec<_>>()
    };
    for stale in stale_servers {
        stale.shutdown().await;
    }
    {
        let guard = servers.lock().await;
        if let Some(server) = guard.get(&key) {
            if !server.dead.load(Ordering::SeqCst) {
                return Ok(server.clone());
            }
        }
    }

    let mut cmd = Command::new(crate::brains::resolve_bin("codex"));
    cmd.args(["app-server", "--stdio"]);
    apply_env_cwd(&mut cmd, conn, work_dir, api_key, lease);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 Codex app-server 失败：{e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Codex app-server stdin 不可用".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex app-server stdout 不可用".to_string())?;
    let stderr = child.stderr.take();
    let child = Arc::new(tokio::sync::Mutex::new(child));
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let pending = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<RpcReply>>::new()));
    let sinks = Arc::new(Mutex::new(HashMap::<String, CodexTurnSink>::new()));
    let dead = Arc::new(AtomicBool::new(false));

    tauri::async_runtime::spawn(async move {
        let mut stdin = stdin;
        while let Some(message) = write_rx.recv().await {
            let Ok(mut line) = serde_json::to_vec(&message) else {
                continue;
            };
            line.push(b'\n');
            if stdin.write_all(&line).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
    });

    let read_pending = pending.clone();
    let read_sinks = sinks.clone();
    let read_dead = dead.clone();
    let response_tx = write_tx.clone();
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line).await;
            if !matches!(read, Ok(n) if n > 0) {
                break;
            }
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(id_value) = value.get("id") {
                if value.get("method").is_some() {
                    respond_to_codex_server_request(&response_tx, id_value.clone(), &value);
                    continue;
                }
                let Some(id) = id_value.as_u64() else {
                    continue;
                };
                if let Some(sender) = read_pending.lock().unwrap().remove(&id) {
                    let reply = if let Some(error) = value.get("error") {
                        Err(rpc_error_message(error))
                    } else {
                        Ok(value.get("result").cloned().unwrap_or_default())
                    };
                    let _ = sender.send(reply);
                }
                continue;
            }
            let Some(method) = value.get("method").and_then(|value| value.as_str()) else {
                continue;
            };
            let params = value.get("params").cloned().unwrap_or_default();
            let Some(thread_id) = params.get("threadId").and_then(|value| value.as_str()) else {
                continue;
            };
            if let Some(sink) = read_sinks.lock().unwrap().get(thread_id).cloned() {
                handle_codex_app_notification(method, &params, &sink);
            }
        }
        read_dead.store(true, Ordering::SeqCst);
        for (_, sender) in read_pending.lock().unwrap().drain() {
            let _ = sender.send(Err("Codex app-server 已退出".into()));
        }
        for sink in read_sinks.lock().unwrap().values() {
            if let Some(sender) = sink.completed.lock().unwrap().take() {
                let _ = sender.send(serde_json::json!({
                    "status": "failed",
                    "error": {"message": "Codex app-server 已退出"}
                }));
            }
        }
    });
    if let Some(stderr) = stderr {
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                let message = line.trim();
                if !message.is_empty() {
                    eprintln!("[codex-app-server] {message}");
                }
                line.clear();
            }
        });
    }

    let server = Arc::new(CodexAppServer {
        tx: write_tx,
        pending,
        sinks,
        loaded_threads: Mutex::new(HashSet::new()),
        next_id: AtomicU64::new(1),
        dead,
        _child: child,
    });
    server
        .request(
            "initialize",
            codex_initialize_params(),
            std::time::Duration::from_secs(15),
        )
        .await?;
    server.notify("initialized", None)?;
    let mut guard = servers.lock().await;
    // 仍有其它对话在执行的旧环境服务不能强杀；它会在后续调用进入本函数时，
    // sinks 变空后由上面的显式 shutdown 回收。
    guard.insert(key, server.clone());
    Ok(server)
}

fn rpc_error_message(error: &serde_json::Value) -> String {
    error
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| error.as_str())
        .unwrap_or("Codex app-server 请求失败")
        .chars()
        .take(500)
        .collect()
}

fn codex_thread_has_active_writer(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("active writer")
        || (normalized.contains("thread/resume") && normalized.contains("already has an active"))
}

fn codex_thread_busy_message() -> String {
    "上一轮仍在收尾，底层会话暂时被占用。Pilot 已停止重复争抢；请稍候几秒后重试。若持续出现，请完全退出其它 Pilot 或 Codex 窗口后再打开。".into()
}

async fn resume_codex_thread(
    server: &CodexAppServer,
    params: serde_json::Value,
) -> Result<(), String> {
    // Windows 结束旧进程和释放 Codex thread writer 之间可能有很短的延迟。
    // 只对这一种可恢复冲突退避；其它协议错误应立即走既有兼容路径。
    const RETRY_DELAYS_MS: [u64; 5] = [150, 300, 600, 1_200, 2_400];
    for (attempt, delay_ms) in RETRY_DELAYS_MS.into_iter().enumerate() {
        match server
            .request(
                "thread/resume",
                params.clone(),
                std::time::Duration::from_secs(60),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(error)
                if codex_thread_has_active_writer(&error)
                    && attempt + 1 < RETRY_DELAYS_MS.len() =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("resume retry loop always returns")
}

fn respond_to_codex_server_request(
    tx: &mpsc::UnboundedSender<serde_json::Value>,
    id: serde_json::Value,
    request: &serde_json::Value,
) {
    let method = request
        .get("method")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let result = match method {
        "item/commandExecution/requestApproval" => serde_json::json!({"decision": "decline"}),
        "item/fileChange/requestApproval" => serde_json::json!({"decision": "decline"}),
        "item/tool/requestUserInput" => serde_json::json!({"answers": {}}),
        _ => {
            let _ = tx.send(serde_json::json!({
                "id": id,
                "error": {"code": -32601, "message": "Pilot 不支持此交互请求"}
            }));
            return;
        }
    };
    let _ = tx.send(serde_json::json!({"id": id, "result": result}));
}

fn app_item_type(item: &serde_json::Value) -> &str {
    item.get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("")
}

fn app_item_status(item: &serde_json::Value, fallback: &str) -> String {
    match item.get("status").and_then(|value| value.as_str()) {
        Some("failed") | Some("declined") => "failed".into(),
        Some("completed") => "completed".into(),
        Some("inProgress") => "running".into(),
        _ => fallback.into(),
    }
}

fn send_codex_app_activity(
    item: &serde_json::Value,
    fallback_status: &str,
    ch: &Channel<TurnEvent>,
) {
    let item_type = app_item_type(item);
    let id = item
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if id.is_empty() {
        return;
    }
    let (label, detail) = match item_type {
        "reasoning" => ("reasoning", String::new()),
        // 原始命令仍会进入上方可折叠的执行记录；活动卡只接收业务语义，
        // 避免把 shell、路径和参数直接推给普通用户。
        "commandExecution" => ("exec", String::new()),
        "fileChange" => ("file_change", "正在更新文件".into()),
        "mcpToolCall" => (
            item.get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("tool"),
            item.get("server")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(120)
                .collect(),
        ),
        "dynamicToolCall" => (
            item.get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("tool"),
            item.get("namespace")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(120)
                .collect(),
        ),
        "imageView" => ("view_image", "正在检查图片".into()),
        "imageGeneration" => ("imagegen", "正在生成图片".into()),
        "webSearch" => ("web_search", "正在搜索资料".into()),
        "contextCompaction" => ("context_compaction", "正在整理会话上下文".into()),
        _ => return,
    };
    let status = app_item_status(item, fallback_status);
    let phase = if item_type == "commandExecution" {
        native_command_phase(item)
    } else {
        ""
    };
    let _ = ch.send(TurnEvent::Activity {
        activity_id: format!("codex:{id}"),
        label: label.into(),
        detail,
        error: if status == "failed" {
            failure_detail(item)
        } else {
            String::new()
        },
        status,
        kind: activity_kind(label).into(),
        phase: phase.into(),
        current: None,
        total: None,
    });
}

fn handle_codex_app_notification(method: &str, params: &serde_json::Value, sink: &CodexTurnSink) {
    {
        let mut images = Vec::new();
        extract_generated_images(params, &mut images);
        if !images.is_empty() {
            let mut collect = sink.collect.lock().unwrap();
            for image in images {
                if !collect.images.contains(&image) {
                    collect.images.push(image);
                }
            }
        }
    }
    if let Some(usage) = parse_usage(&params["tokenUsage"]["last"])
        .or_else(|| parse_usage(&params["tokenUsage"]["total"]))
        .or_else(|| parse_usage(&params["tokenUsage"]))
        .or_else(|| parse_usage(&params["usage"]))
        .or_else(|| parse_usage(&params["turn"]["usage"]))
    {
        sink.collect.lock().unwrap().usage = Some(usage);
    }

    match method {
        "item/agentMessage/delta" => {
            let delta = params
                .get("delta")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if delta.is_empty() {
                return;
            }
            if let Some(item_id) = params.get("itemId").and_then(|value| value.as_str()) {
                sink.streamed_messages
                    .lock()
                    .unwrap()
                    .insert(item_id.to_string());
            }
            sink.collect.lock().unwrap().text.push_str(delta);
            let _ = sink.channel.send(TurnEvent::Delta {
                text: delta.to_string(),
            });
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            let item_id = params
                .get("itemId")
                .and_then(|value| value.as_str())
                .unwrap_or("reasoning");
            let detail = params
                .get("delta")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(240)
                .collect();
            let _ = sink.channel.send(TurnEvent::Activity {
                activity_id: format!("codex:{item_id}"),
                label: "reasoning".into(),
                detail,
                error: String::new(),
                status: "running".into(),
                kind: "reasoning".into(),
                phase: String::new(),
                current: None,
                total: None,
            });
        }
        "item/started" => {
            let item = &params["item"];
            if app_item_type(item) == "agentMessage"
                && !sink.collect.lock().unwrap().text.is_empty()
            {
                sink.collect.lock().unwrap().text.push_str("\n\n");
                let _ = sink.channel.send(TurnEvent::Delta {
                    text: "\n\n".into(),
                });
            }
            send_codex_app_activity(item, "running", &sink.channel);
        }
        "item/completed" => {
            let item = &params["item"];
            send_codex_app_activity(item, "completed", &sink.channel);
            match app_item_type(item) {
                "agentMessage" => {
                    let item_id = item
                        .get("id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    if !sink.streamed_messages.lock().unwrap().contains(item_id) {
                        if let Some(text) = item.get("text").and_then(|value| value.as_str()) {
                            sink.collect.lock().unwrap().text.push_str(text);
                            let _ = sink.channel.send(TurnEvent::Delta {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                "commandExecution" => {
                    let detail = item
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(200)
                        .collect::<String>();
                    {
                        let mut g = sink.collect.lock().unwrap();
                        let at = crate::convo::utf16_len(&g.text);
                        g.tools.push(ToolCall { label: "exec".into(), detail: detail.clone(), at });
                    }
                    let _ = sink.channel.send(TurnEvent::Tool {
                        label: "exec".into(),
                        detail,
                    });
                }
                "mcpToolCall" | "dynamicToolCall" => {
                    let label = item
                        .get("tool")
                        .and_then(|value| value.as_str())
                        .unwrap_or("tool")
                        .to_string();
                    {
                        let mut g = sink.collect.lock().unwrap();
                        let at = crate::convo::utf16_len(&g.text);
                        g.tools.push(ToolCall { label: label.clone(), detail: String::new(), at });
                    }
                    let _ = sink.channel.send(TurnEvent::Tool {
                        label,
                        detail: String::new(),
                    });
                }
                _ => {}
            }
        }
        "thread/compacted" => {
            let _ = sink.channel.send(TurnEvent::ContextCompacted {
                pre_tokens: params
                    .get("preTokens")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
            });
        }
        "error" => {
            if params.get("willRetry").and_then(|value| value.as_bool()) != Some(true) {
                sink.collect.lock().unwrap().is_error = true;
            }
        }
        "turn/completed" => {
            let turn = params.get("turn").cloned().unwrap_or_default();
            if turn.get("status").and_then(|value| value.as_str()) == Some("failed") {
                sink.collect.lock().unwrap().is_error = true;
            }
            if let Some(sender) = sink.completed.lock().unwrap().take() {
                let _ = sender.send(turn);
            }
        }
        _ => {}
    }
}

fn codex_sandbox(mode: PermMode) -> &'static str {
    match mode {
        PermMode::Plan => "read-only",
        PermMode::Full => "danger-full-access",
        PermMode::Ask | PermMode::Auto => "workspace-write",
    }
}

fn validate_codex_model(model: &str) -> Result<Option<String>, String> {
    let model = model.trim();
    if model.is_empty() {
        Ok(None)
    } else if model.starts_with('-') || model.contains(char::is_whitespace) {
        Err(format!("无效的模型标识: {model}"))
    } else {
        Ok(Some(model.to_string()))
    }
}

fn codex_effort<'a>(model: &str, effort: &'a str) -> &'a str {
    // A conversation can retain Sol/Astra's effort when the user switches to Luna.
    // Luna's highest subscription-channel tier is max, not ultra.
    if model.trim() == "gpt-6-luna" && effort == "ultra" {
        "max"
    } else {
        effort
    }
}

#[allow(clippy::too_many_arguments)]
async fn try_run_codex_app_server(
    registry: RunRegistry,
    conn: &Connection,
    work_dir: &str,
    model: &str,
    effort: &str,
    mode: PermMode,
    lease: Option<&crate::bridge::Lease>,
    api_key: &str,
    session_ref: &str,
    is_first: bool,
    system: Option<&str>,
    message: &str,
    turn_id: &str,
    channel: Channel<TurnEvent>,
) -> CodexAppServerAttempt {
    let model = match validate_codex_model(model) {
        Ok(model) => model,
        Err(error) => {
            return CodexAppServerAttempt::Completed(failed_turn_result(
                session_ref,
                error,
                &channel,
            ));
        }
    };
    let server = match codex_app_server(conn, work_dir, api_key, lease).await {
        Ok(server) => server,
        Err(error) => return CodexAppServerAttempt::Unavailable(error),
    };
    let sandbox = codex_sandbox(mode);
    let config = serde_json::json!({"sandbox_workspace_write": {"network_access": true}});
    let mut thread_id = session_ref.to_string();

    if is_first || thread_id.trim().is_empty() {
        let mut params = serde_json::json!({
            "cwd": work_dir,
            "approvalPolicy": "never",
            "sandbox": sandbox,
            "config": config,
            "developerInstructions": system,
            "ephemeral": false
        });
        if let Some(model) = model.as_ref() {
            params["model"] = serde_json::Value::String(model.clone());
        }
        let result = match server
            .request("thread/start", params, std::time::Duration::from_secs(60))
            .await
        {
            Ok(result) => result,
            Err(error) => return CodexAppServerAttempt::Unavailable(error),
        };
        thread_id = result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if thread_id.is_empty() {
            return CodexAppServerAttempt::Unavailable("Codex app-server 未返回 thread id".into());
        }
        server
            .loaded_threads
            .lock()
            .unwrap()
            .insert(thread_id.clone());
    } else if !server.loaded_threads.lock().unwrap().contains(&thread_id) {
        let mut params = serde_json::json!({
            "threadId": thread_id,
            "excludeTurns": true,
            "cwd": work_dir,
            "approvalPolicy": "never",
            "sandbox": sandbox,
            "config": config
        });
        if let Some(model) = model.as_ref() {
            params["model"] = serde_json::Value::String(model.clone());
        }
        if let Err(error) = resume_codex_thread(&server, params).await {
            return if codex_thread_has_active_writer(&error) {
                CodexAppServerAttempt::ThreadBusy(codex_thread_busy_message())
            } else {
                CodexAppServerAttempt::Unavailable(error)
            };
        }
        server
            .loaded_threads
            .lock()
            .unwrap()
            .insert(thread_id.clone());
    }

    let collect = Arc::new(Mutex::new(Collect {
        text: String::new(),
        tools: Vec::new(),
        session_ref: thread_id.clone(),
        is_error: false,
        usage: None,
        raw_tail: String::new(),
        images: Vec::new(),
    }));
    let (completed_tx, completed_rx) = oneshot::channel();
    let sink = CodexTurnSink {
        channel: channel.clone(),
        collect: collect.clone(),
        completed: Arc::new(Mutex::new(Some(completed_tx))),
        streamed_messages: Arc::new(Mutex::new(HashSet::new())),
    };
    server.attach(&thread_id, sink);
    let (_canceled, mut kill_rx) = registry.register(turn_id);

    let mut params = serde_json::json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": message, "text_elements": []}],
        "cwd": work_dir,
        "approvalPolicy": "never"
    });
    if let Some(model) = model.as_ref() {
        params["model"] = serde_json::Value::String(model.clone());
    }
    let effort = codex_effort(model.as_deref().unwrap_or_default(), effort);
    if matches!(
        effort,
        "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    ) {
        params["effort"] = serde_json::Value::String(effort.to_string());
    }
    let turn = match server
        .request("turn/start", params, std::time::Duration::from_secs(30))
        .await
    {
        Ok(result) => result.get("turn").cloned().unwrap_or_default(),
        Err(error) => {
            server.detach(&thread_id);
            registry.unregister(turn_id);
            return if codex_thread_has_active_writer(&error) {
                CodexAppServerAttempt::ThreadBusy(codex_thread_busy_message())
            } else {
                CodexAppServerAttempt::Unavailable(error)
            };
        }
    };
    let server_turn_id = turn
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if server_turn_id.is_empty() {
        server.detach(&thread_id);
        registry.unregister(turn_id);
        return CodexAppServerAttempt::Unavailable("Codex app-server 未返回 turn id".into());
    }

    let completed_turn = tokio::select! {
        result = completed_rx => result.unwrap_or_else(|_| serde_json::json!({
            "status": "failed",
            "error": {"message": "Codex app-server 事件流已中断"}
        })),
        _ = &mut kill_rx => {
            if server.request(
                "turn/interrupt",
                serde_json::json!({"threadId": thread_id, "turnId": server_turn_id}),
                std::time::Duration::from_secs(10),
            ).await.is_err() {
                // 中断确认失败时宁可终止共享 app-server，也不能让带凭据的模型在 UI
                // 已显示“停止”后继续写站点；其它对话会自动走 exec 或重建常驻服务。
                server.shutdown().await;
            }
            serde_json::json!({"status": "interrupted"})
        }
    };
    let canceled = registry.is_canceled(turn_id);
    registry.unregister(turn_id);
    server.detach(&thread_id);

    let collect = collect.lock().unwrap().clone();
    let status = completed_turn
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("failed");
    let ok = status == "completed" && !collect.is_error && !canceled;
    let error = if canceled || status == "interrupted" {
        "已停止".to_string()
    } else if !ok {
        completed_turn
            .get("error")
            .map(rpc_error_message)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "Codex 本轮执行失败".into())
    } else {
        String::new()
    };
    CodexAppServerAttempt::Completed(finish_codex_app_turn(collect, ok, error, &channel))
}

fn failed_turn_result(
    session_ref: &str,
    error: String,
    channel: &Channel<TurnEvent>,
) -> TurnResult {
    let _ = channel.send(TurnEvent::Done {
        ok: false,
        error: error.clone(),
    });
    TurnResult {
        ok: false,
        text: String::new(),
        tools: Vec::new(),
        error,
        session_ref: session_ref.to_string(),
        usage: None,
        limit_reset: None,
    }
}

fn finish_codex_app_turn(
    collect: Collect,
    ok: bool,
    error: String,
    channel: &Channel<TurnEvent>,
) -> TurnResult {
    let error = error.to_string();
    let persisted_text = collect.text.to_string();
    let clean_text = persisted_text.trim().to_string();
    let clean_text = if ok && !collect.images.is_empty() {
        let existing = collect
            .images
            .iter()
            .filter(|path| Path::new(path.as_str()).is_file())
            .cloned()
            .collect::<Vec<_>>();
        append_generated_images(&clean_text, &existing)
    } else {
        clean_text
    };
    let limit_reset = (!ok).then(|| detect_usage_limit(&error)).flatten();
    let _ = channel.send(TurnEvent::Done {
        ok,
        error: error.clone(),
    });
    TurnResult {
        ok,
        text: clean_text,
        tools: collect
            .tools
            .into_iter()
            .map(|mut tool| {
                tool.detail = tool.detail.to_string();
                tool
            })
            .collect(),
        error,
        session_ref: collect.session_ref,
        usage: collect.usage,
        limit_reset,
    }
}

// ---- 系统提示词（角色框架）----

/// 与任何连接、站点和通用技能都隔离的自由对话提示词。
/// cwd 只是一块可选本地工作区；不能因为 Pilot 当前选中了某条连接就读取或操作它。
pub fn workspace_system_prompt(has_folder: bool) -> String {
    let workspace = if has_folder {
        "用户已明确选择一个本地文件夹作为本会话工作目录。需要处理文件时先查看目录现状，再按用户要求操作；不要遍历或读取无关文件。"
    } else {
        "用户没有选择本地文件夹；当前目录是 Pilot 为本会话创建的空白隔离目录。除非用户明确要求生成文件，否则把本次任务当作纯对话。"
    };
    format!(
        "你是 Pilot 的通用 AI 助手。本会话是独立自由对话，不属于任何 Cloudflare 项目或远程服务器。\
不得查找、读取或使用 CLOUDFLARE_API_TOKEN 或当前 Pilot 连接信息，也不要假设存在目标站点或服务器。{workspace}\n\
先理解用户意图，必要时简短澄清；涉及文件修改时先说明影响并遵守当前权限档位。回答使用用户所用语言，简洁自然。"
    )
}

/// 普通对话的模型进程只在当前回合存活。Claude/Codex 自己启动的后台命令既不受
/// Pilot TaskStore 管理，也没有可供下一轮重新订阅的完成事件。因此每一轮都注入这条
/// 运行时约束（而不只放在首轮 system prompt），让既有 session 也立即生效。
///
/// SSH 会话例外：它有独立桥接租约和明确的远程运维提示，长编译等允许由用户通过日志
/// 分次查看；这里处理的是 Cloudflare/workspace 对话中“说稍后自动继续、实际无人
/// 接收结果”的假后台任务。
fn conversation_completion_policy(message: &str, connection_kind: &str) -> String {
    match runtime_policy(connection_kind) {
        None => message.to_string(),
        Some(policy) => format!("{message}\n\n<pilot-runtime-policy>\n{policy}\n</pilot-runtime-policy>"),
    }
}

/// 上面那条运行时约束的正文；SSH 会话没有（见 conversation_completion_policy）。
/// CLI 执行器每轮包在用户消息外面；原生 API 执行器放进 system 一份（见 api_agent_tools::instructions），
/// 不然每轮都往持久化的历史里多塞一份。
pub(crate) fn runtime_policy(connection_kind: &str) -> Option<&'static str> {
    if connection_kind == "ssh" {
        return None;
    }
    Some(
        "当前请求需要用工具采集、生成、检查或执行后才能回答时，必须在本回合内等到工具结束并返回真实结果。\
不要把这类工作留成无人接收的后台任务，也不要在结果尚未就绪时结束回合并声称“稍后会自动继续”。\
不得为这类工作使用 run_in_background、nohup、shell `&` 或其它脱离当前回合的方式；应以前台方式执行\
并等待 completed、failed 或 canceled 后再回复，不得让 pending/running 的任务跨出本回合。周期性或\
确实需要脱离当前对话运行的工作，向用户说明需要由用户自行安排，不要自行在后台启动。无法在本回合完成时，明确说明中断点和可恢复状态，不得假装\
仍在运行。",
    )
}

/// Cloudflare 建站助手的系统提示词。cwd＝项目目录；token 已注入 env。
///
/// `seeded`＝项目目录里已经有文件（引用了起始模板，或上一轮已经建过）。**必须告诉模型**：
/// 以前 use_template 把模板拷进去之后没人吭声，模型视角永远是「空目录、从白纸开始」，
/// 于是那份精心设计的起点直接被推倒重写 —— 内置模板等于白做。
pub fn cf_system_prompt(project: &str, account_id: &str, seeded: bool) -> String {
    let acct = if account_id.is_empty() {
        "(未指定)".to_string()
    } else {
        account_id.to_string()
    };
    let seed = if seeded {
        "【当前目录已有起点（重要）】项目目录里已经有文件了（多半是你选的起始模板，或上一轮建的）。\
**先 `ls` 看一眼、把 index.html 读完再动手**：它的 `:root` 里已经有整套设计变量、排版与间距刻度。\
你的活是**在它上面改**——换文案、按需增删区块、调色板（改 `:root` 的变量即可）。\
**别推倒重写、别新建一套并行的 CSS。**\n"
    } else {
        ""
    };
    format!(
        "你是用户的「建站 + 部署」助手，在当前目录里为用户搭建网站，并用 wrangler 部署到 Cloudflare。\n\
{seed}\
项目 slug：{project}（部署到 Cloudflare Pages 时用它当 --project-name）。Cloudflare 账号 id：{acct}\
（已通过 CLOUDFLARE_API_TOKEN / CLOUDFLARE_ACCOUNT_ID 环境变量注入 wrangler，无需登录）。\n\
【工作方式】以对话推进：先理解用户想要什么样的站（定位 / 风格 / 页面 / 是否要后端与数据库），\
给一个简短方案征得同意，再动手；边做边用一两句话说明你在做什么；每完成一步告诉用户结果。\n\
【技术约定】\n\
- 纯前端站：写静态文件或用轻量框架，构建产物放 ./dist；本地预览由 Pilot 用 `wrangler pages dev`（或你在 package.json 写的 dev 脚本）起。\n\
- 要后端 / 表单 / 数据库：用 Cloudflare Pages Functions 或 Worker + D1；`wrangler d1 create <name>` 建库，\
在 wrangler.toml 声明绑定，本地 `wrangler pages dev` 会自动模拟绑定。\n\
- 在项目根写一个 pilot.json 描述项目：{{\"dev\":\"本地预览命令\",\"port\":本地预览端口,\"build\":\"构建命令\",\"out\":\"构建产物目录\",\"bindings\":[\"d1:数据库名\"]}}；\
Pilot 用它来起预览 / 构建（没有 pilot.json 时默认 `wrangler pages dev .` 跑在 8788）。**若你的 dev 命令监听的端口不是 8788，务必在 pilot.json 里写对 \"port\"，否则预览窗打不开。**\n\
- 绝不要把任何密钥写进代码或文件；token 已在环境变量里。\n\
【设计底座（硬性）】视觉质量的下限靠纪律保证，不靠发挥。这几条任何时候都不许破：\n\
- 颜色/圆角**只从 `:root` 的设计变量取**（`--bg/--surface/--text/--muted/--accent/--radius`），正文里不许硬编码 hex；\
间距只用刻度值（4/8/12/16/24/40/64）；色板 = 中性色 + **一个**强调色；正文对比度 ≥4.5:1。\n\
- 字号成体系（14/16/20/28/40，别只有两档），正文行高 1.6-1.8，中文用系统字体栈，别引一堆字体。\n\
- 内容有最大宽度容器（680-1100px）居中；可点元素都有 hover 态；补 favicon 与 <title>。\n\
- 不确定怎么好看时，宁可更简单：减色、减框、加留白。\n\
**完整设计规范**（排版/组件/状态约定、成品自检清单、常见翻车点）见随附技能 `web-design`——\
**每次要写 CSS、调版式、改配色前先把它读一遍**，别凭记忆发挥。\n\
【视觉自检（硬性）】你写的页面必须**亲眼看过**才算完成——首次建完和每次大改版式后：\n\
1. 用随附截图工具截两张：桌面 `--width 1280` 和手机 `--width 390 --height 844`。\
本地预览在跑就截 `http://127.0.0.1:<端口>`；纯静态页可以直接截 `file://<项目绝对路径>/index.html`（不需要起服务）；\
有 Functions/D1 的动态页且预览没在跑，就请用户点一下「预览」再截。\n\
2. Read 打开截图逐项检查：横向溢出/滚动条、文字对比度、间距节奏是否均匀、字体是否真的生效、\
图片是否加载（裂图/占位）、按钮悬停态、手机端是否挤爆或字过小。\n\
3. 发现问题先修掉再截一轮确认（至少一轮），完成后把截图路径告诉用户并简述你修了什么。别把没看过的页面交给用户。\n\
【参考图】用户贴参考截图时：提取它的**布局结构、配色气质、字重对比、密度**来做设计，\
不要抄它的文字内容、logo 或品牌名；做完对照参考图自查气质是否接近。\n\
【部署】用户明确要部署时：纯前端 `wrangler pages deploy <产物目录> --project-name {project}`；\
要绑定自定义域名再配 DNS / 自定义域名。部署、改 DNS、写远端 D1 这类对线上生效的操作，\
Pilot 会让用户确认一次，你正常执行命令即可。\n\
回答用中文，简洁自然，先给方案别一上来就大动干戈。运行命令（Bash）时 description 字段一律用中文\
写清这条命令做什么、会影响什么——它会原样展示给非技术用户做批准决定。"
    )
}

// 远程连接 × Codex 的**真实边界**（用户拍板放开，UI 上有醒目警告）：
// - 走 AI 桥的命令**照样逐条弹卡**（闸在 bridge.rs，与厂商无关）—— 这条对 codex 一样有效；
// - 但 codex 无头下没有逐工具闸（claude 靠 PreToolUse 钩子、grok 靠 ACP 权限回调，
//   codex 只有粗粒度 sandbox），所以**拦不住它绕开桥**：直接调系统 ssh/scp + 用户自己的
//   ~/.ssh 密钥打同一台机器 —— 那条路（claude 靠 DANGER 清单弹卡）对 codex 一张卡都不弹。
// 所以这个组合能用，但「每条命令都确认」对它是打折的，UI 必须把这句话摆在用户眼前。
// ★ 配套硬性要求见 lib.rs::apply_brain_switch：**绝不能把 ssh 对话的 ask/auto 静默改写成 full** ——
//   桥那道卡是 codex 仅剩的闸，把它也拿掉就是彻底的无人确认。

/// 远程机器助手的系统提示（kind=ssh 的对话）。
///
/// 契约要和三处对齐，改这里必须同步看：`tools.rs::SSH_JS`（脚本参数）、
/// `bridge.rs`（执行/确认）、`permit.rs::is_bridge_cmd`（钩子放行的形状——**多一个尾巴就会多弹一张卡**）。
pub fn ssh_system_prompt(user: &str, host: &str, port: u16, ssh_js: &str) -> String {
    format!(
        "你是用户这台远程服务器（{user}@{host}:{port}）的运维助手：帮 TA 装软件、配服务、排障、做加固。\n\
【怎么在服务器上执行命令】用随附工具（**只有这一条路**）：\n\
`node \"{ssh_js}\" '<命令>'`，需要更长时限时 `node \"{ssh_js}\" --timeout <秒> '<命令>'`（默认 240 秒，最大 900）。\n\
- 命令**必须**用一对引号整个包起来，且引号后不许再挂任何东西（`; cmd`、`&& cmd`、`> file`、管道都不行）——\
多步请写成一条：`node \"{ssh_js}\" 'cd /etc/nginx && ls'`。\n\
- 输出是 JSON：`{{\"ok\":true,\"code\":<远端退出码>,\"stdout\":\"…\",\"stderr\":\"…\"}}`；\
`ok:false` 时 `error` 是原因（比如用户拒绝了这条命令）。**看 code 判断成败**，别只看有没有报错。\n\
- 凭据在 Pilot 手里，你拿不到也不需要——别去找密码/私钥，别读 ~/.ssh，别自己调系统的 ssh/scp/rsync：\
那些用的是用户自己的密钥、绕开了确认闸，会被拦下来。\n\
【这条通道的脾气】\n\
- **每条命令是独立的一次性 shell**：`cd` 不会留到下一条，环境变量也不会；要连续动作就用 `&&` 串在同一条里。\n\
- **没有 PTY**：交互式提示符没法应答。装包加 `DEBIAN_FRONTEND=noninteractive` 和 `-y`；\
`sudo` 必须免密（`sudo -n`），要输密码的一律会挂住直到超时。\n\
- 长任务（大编译、大下载）别占着通道：用 `nohup … > /tmp/x.log 2>&1 &` 或 systemd 放后台，再分次看日志。\n\
- 每条命令**用户都要点一次确认**（除非 TA 选了全自动）。所以：把相关步骤合并成一条，别拆成几十条来回；\
但也别把「装依赖」和「改配置删数据」混在一条里——用户要看得懂自己在批准什么。\n\
【干活的规矩】这是**用户的真机**，不是沙箱：\n\
- 先看后改：动配置前先 `cat` 出来看、先备份（`cp x x.bak.$(date +%s)`）；改完验证（`nginx -t`、`systemctl status`）。\n\
- 破坏性动作（删数据、`rm -rf`、改防火墙规则、重启机器、动 sshd 配置）**先说清楚后果、征得同意再做**；\
尤其小心把自己关在门外：改 sshd/防火墙前先确认新规则不会切断当前连接。\n\
- 装东西前先探明环境（`cat /etc/os-release`、`uname -m`、有没有 docker/systemd、磁盘和内存够不够），别猜。\n\
- 报告用中文，先说结论（成了/没成、影响是什么），再给关键输出；命令失败就把 stderr 原文给用户看，别自己编原因。"
    )
}

/// GitHub 仓库会话：在 Pilot 已克隆好的检出里改代码，走「分支 → 提交 → 推送 → PR」，
/// 默认不直接改默认分支。凭据由 Pilot 注入，模型不得读取、打印或写进任何文件。
pub fn github_system_prompt(repo: &str, login: &str) -> String {
    format!(
        "你在 GitHub 仓库 `{repo}` 的本地检出里工作（当前目录就是仓库根目录，Pilot 已克隆好并配置了凭据，\
提交作者为 `{login}`）。先读 README、目录结构和相关代码，弄清现状再动手；改动前说明打算怎么改。\n\
【工作流程】除非用户明确要求，否则不要直接提交到默认分支：先 `git switch -c pilot/<简短主题>` 新建分支，\
小步提交、写清楚的提交说明；改完先运行项目已有的测试 / 构建 / lint 并如实报告结果。需要发布时：有 \
`github_publish` 工具就用它（Pilot 代为推送并开 PR，合并用 `github_merge`；询问 / 自动档会先请用户确认，全自动档直接执行）；没有这个工具时 \
`git push -u origin <分支>`，再用 `gh pr create` 创建 PR（标题和描述写清改了什么、为什么、怎么验证），\
没有安装 gh 时，推送分支后把 `https://github.com/{repo}/compare/<分支>?expand=1` 给用户。\n\
【安全】GitHub 凭据由 Pilot 通过环境变量注入：不要读取、打印、回显 GITHUB_TOKEN / GH_TOKEN，\
不要把 Token 写进远程地址、配置文件、代码或提交里，也不要修改 credential 相关的 git 配置。\
强制推送、删除分支 / 仓库、合并 PR、改仓库设置、发 Release 都必须先征得用户明确同意。\
用户所用语言回复，简洁说明做了什么、结果如何、还需要用户确认什么。"
    )
}

/// 网页截图能力的系统提示补充（shot.js 由 Pilot 生成在数据目录，见 tools.rs）。
/// 只给 Cloudflare 建站会话：截本地预览自查 / 截参考站。
pub fn shot_prompt(shot_path: &str) -> String {
    let common = format!(
        "【网页截图】需要网页截图时先用随附工具自动后台渲染：\n\
`node \"{shot_path}\" --url <URL> --out shots/名字.png [--width 1280] [--full-page] [--wait 3000]`\n\
成功输出 JSON（含文件路径和最终 URL），失败返回结构化 `code`、`detail` 与是否可见重试。\
截外部网站时加 `--allow-window`：后台截图遇到真人验证会自动改用 Pilot 专用的正常 Chrome 窗口再截\
（之前要验证过的网站直接用窗口截），窗口里出现验证由用户完成，验证消失后自动截图并关窗；\
第一次带这个参数前先用一句话告诉用户可能会弹出 Pilot 浏览器窗口、出现验证请在窗口里完成。\
该浏览器使用 Pilot 独立配置，不读取用户日常 Chrome。不得要求用户自己保存、上传截图，也不得在该专用浏览器中\
代填密码、验证码或登录账号；若返回 `login_required`，停止并说明该页面必须登录。\n\
截完**必须用 Read 打开图片确认**内容正确\
——打不开的页面 Chrome 会把自己的错误页截下来，所以要看图排除：错误页 / 验证码 / 空白页 / Cookie 弹窗。\
不对就依据真实 `code/detail` 调整一次；禁止把所有失败笼统说成“反爬”，也不要反复启动可见浏览器。"
    );
    format!(
        "{common}\n\
用途：截 `http://127.0.0.1:<预览端口>` 检查你搭的页面实际效果（先确认本地预览在跑）；也可截参考网站找样式灵感。"
    )
}

/// 站点会话的截图说明：给文章配真实网页截图、截线上页面自查。
pub fn site_shot_prompt(shot_path: &str) -> String {
    format!(
        "【网页截图】文章需要截图时，用随附工具截真实网页，不要用占位图、示意图或 AI 生成的界面图：\n\
`node \"{shot_path}\" --url <URL> --out <站点图片目录>/<有意义的文件名>.png [--width 1280] [--height 800] [--full-page] [--wait 3000]`\n\
图片存进站点自己的静态图片目录（如 public/images/…），正文里用站点现有的写法引用，写清 alt 和图注。\
截完**必须打开图片确认**（Read；Pilot 原生 API 执行器里用 view_image）：不是错误页、验证码、空白页或 Cookie 弹窗；有个人信息、账号、余额等敏感内容先裁掉或打码再用。\
用户在对话里时，截外部网站加 `--allow-window`：后台截图遇到真人验证（Etherscan、BscScan 这类站常见）会自动改用 Pilot 专用的正常 Chrome 窗口再截，\
之前要验证过的网站直接用窗口截；窗口里出现验证由用户完成，验证一通过就自动截图并关窗。\
第一次带这个参数前先用一句话告诉用户「可能会弹出 Pilot 浏览器窗口，出现验证请在窗口里完成」。该浏览器用 Pilot 独立配置，不读用户日常 Chrome；\
不要替用户点验证、不要在里面填任何账号密码或验证码，同一个站点窗口截图失败后不要反复重试。\
无人值守任务（托管运营、后台任务）里不要用 `--allow-window` 或 `--visible`（没有人能处理验证），遇到 `verification_required` 当作截不到处理。\
需要登录才能看到的页面（`login_required`）不要截、不要代填任何账号密码。\
确实截不到时：正文先不放图，在 `.pilot/todo.json` 里写一条 kind 为 `ai` 的待办，title 写清缺哪个页面的图、给出网址，说明可以在对话里让你用可见浏览器再截一次；\
不要把截不到笼统说成「网站拦截」，要按返回的 `code` / `detail` 如实说明。\
也可以截自己站点的线上页面或本地预览，检查改动后的实际效果。"
    )
}

/// codex 专用兜底：它没有 `--plugin-dir`，吃不到随附的设计技能，只能在提示词里给绝对路径让它自己读。
/// claude/grok 走 plugin 注册（渐进披露、任意轮可重读），不需要这段。
pub fn design_prompt(skill_path: &str) -> String {
    format!(
        "【设计规范文件】完整建站设计规范在这个文件里：\n`{skill_path}`\n\
**每次要写 CSS、调版式、改配色、加区块之前，先用 Read 打开它读一遍**（它有设计变量契约、\
排版与间距刻度、组件与状态约定、成品自检清单，以及最容易翻车的那些点）。别凭记忆发挥。"
    )
}

// ---- 运行一轮 ----

/// 杀整棵进程树（进程组/子孙），别留下带着密钥继续写 CMS 的孙进程（node/bash 等）。
/// spawn 前需已设 process_group(0)（unix）；Windows 用 taskkill /T。
pub(crate) fn kill_tree(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .status();
    }
    #[cfg(windows)]
    if let Some(pid) = pid {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .status();
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_turn(
    registry: RunRegistry,
    conn: Connection,
    work_dir: String,
    brain: String,
    model: String,
    perm_mode: String,
    effort: String,
    // fast 模式：只有 Opus 5 / Opus 4.8 支持（CLI 2.1.205+ 才认 headless 的 fastMode 键）。
    fast: bool,
    data_dir: PathBuf,
    ssh: crate::ssh::SshSessions,
    session_ref: String,
    is_first: bool,
    system: Option<String>,
    message: String,
    turn_id: String,
    channel: Channel<TurnEvent>,
    api_execution_policy: crate::api_execution::ApiExecutionPolicy,
) -> TurnResult {
    // 用户消息仍按原文写入 conversations.json；这里只增强发给执行器的临时副本。
    // 每轮注入才能覆盖升级前已经建立的 Claude/Codex session。
    // 原生 API 执行器不包：它把用户消息原样持久化进自己的历史、每轮整段重发，包一层就是每轮多存一份；
    // 这两段约束它放在 system 里一份（见 api_agent_tools::instructions）。
    let message = if brain.starts_with("api:") {
        message
    } else {
        crate::editorial::for_turn(conversation_completion_policy(&message, &conn.kind), &conn.kind)
    };
    // ssh 连接没有 API key（密码/口令是给 Pilot 连机器用的，绝不进子进程），
    // 且无口令私钥根本没有钥匙串条目 —— 这里拿不到不算错。
    let api_key = if conn.kind == "ssh" || conn.kind == "workspace" {
        String::new()
    } else {
        match keychain::get_key(&conn.id) {
            Ok(k) => k,
            Err(e) => {
                let _ = channel.send(TurnEvent::Done {
                    ok: false,
                    error: e.clone(),
                });
                return TurnResult {
                    ok: false,
                    text: String::new(),
                    tools: vec![],
                    error: e,
                    session_ref,
                    usage: None,
                    limit_reset: None,
                };
            }
        }
    };

    let permit_base = data_dir.join("permit");
    let mode = PermMode::parse(&perm_mode);
    let perm = PermSpec {
        mode,
        conv_id: turn_id.clone(),
        gen_dir: permit_base.join("hooks").join(&turn_id),
        pending_dir: permit_base.join("pending"),
        ssh_js: crate::tools::ssh_js_path(&data_dir),
        fast,
    };

    let work_dir = if work_dir.trim().is_empty() {
        conn.skill_dir.clone()
    } else {
        work_dir
    };
    // GitHub：首轮（或本地检出被删后）由 Pilot 用这枚 Token 克隆仓库，模型不必碰凭据。
    if conn.kind == "github" {
        let _ = channel.send(TurnEvent::Activity {
            activity_id: "github-checkout".into(),
            label: "准备仓库".into(),
            detail: String::new(),
            error: String::new(),
            status: "running".into(),
            kind: "command".into(),
            phase: "正在准备仓库".into(),
            current: None,
            total: None,
        });
        let checkout =
            crate::github::ensure_checkout(&conn.skill_dir, &work_dir, &api_key, &conn.login, &conn.account_id).await;
        let (status, error) = match &checkout {
            Ok(_) => ("completed", String::new()),
            Err(e) => ("failed", e.clone()),
        };
        let _ = channel.send(TurnEvent::Activity {
            activity_id: "github-checkout".into(),
            label: "准备仓库".into(),
            detail: String::new(),
            error: error.clone(),
            status: status.into(),
            kind: "command".into(),
            phase: if checkout.as_ref().is_ok_and(|cloned| *cloned) { "仓库已克隆".into() } else { "仓库已就绪".into() },
            current: None,
            total: None,
        });
        if let Err(e) = checkout {
            return failed_turn_result(&session_ref, e, &channel);
        }
    }
    // 这是启动 Codex 前的最终兼容性闸门：覆盖首次安装、外部安装以及旧缓存残留。
    // 只做本地检查和可恢复的缓存改名，不在普通对话中偷偷升级 CLI。
    if brain == "codex" {
        if let Err(e) = prepare_codex_cache().await {
            let _ = channel.send(TurnEvent::Done {
                ok: false,
                error: e.clone(),
            });
            return TurnResult {
                ok: false,
                text: String::new(),
                tools: vec![],
                error: e,
                session_ref,
                usage: None,
                limit_reset: None,
            };
        }
    }
    // AI 桥租约：只有 ssh 连接才有。Drop（本函数任何出口）即撤销令牌 + 删目录 → 回合一结束，
    // 残留的 AI 进程也再没法在服务器上跑任何东西。
    let lease = crate::bridge::Lease::start(
        &conn,
        &work_dir,
        &turn_id,
        mode,
        perm.pending_dir.clone(),
        ssh.clone(),
    );
    // 随附的建站设计技能：只给 CF 会话（内容站/远程运维用不上它，白占上下文）。
    // claude 和 grok 吃同一个 plugin 目录格式；codex 没有 --plugin-dir，走 design_prompt 兜底。
    let plugin_dir =
        (conn.kind == "cloudflare").then(|| crate::tools::design_plugin_dir(&data_dir));
    // Native API services are independent of installed CLI clients. Never fall through
    // to Claude or silently change protocol/model after a failed API request.
    if let Some(profile_id) = brain.strip_prefix("api:") {
        return crate::api_agent::run_turn(
            registry, conn, work_dir, profile_id.to_string(), model, effort, mode,
            data_dir, lease.as_ref(), session_ref, is_first,
            system, message, turn_id, channel, api_key,
            api_execution_policy,
        ).await;
    }
    if !matches!(brain.as_str(), "claude" | "codex" | "grok") {
        return failed_turn_result(&session_ref, "不支持的模型服务，请重新选择".into(), &channel);
    }
    // grok 走 ACP（JSON-RPC over stdio）——协议不同族，整轮交给 acp 模块（事件/结果契约一致）。
    if brain == "grok" {
        return crate::acp::run_turn(
            registry,
            conn,
            work_dir,
            model,
            mode,
            effort,
            perm.pending_dir.clone(),
            perm.ssh_js.clone(),
            lease.as_ref(),
            session_ref,
            is_first,
            system,
            message,
            turn_id,
            channel,
            api_key,
            plugin_dir,
        )
        .await;
    }
    if brain == "codex" {
        match try_run_codex_app_server(
            registry.clone(),
            &conn,
            &work_dir,
            &model,
            &effort,
            mode,
            lease.as_ref(),
            &api_key,
            &session_ref,
            is_first,
            system.as_deref(),
            &message,
            &turn_id,
            channel.clone(),
        )
        .await
        {
            CodexAppServerAttempt::Completed(result) => {
                permit::sweep_conv(&perm.pending_dir, &turn_id);
                return result;
            }
            CodexAppServerAttempt::Unavailable(reason) => {
                // app-server 仍是实验接口：当前 CLI 不支持、初始化失败或协议变化时，
                // 静默回到已验证的 `codex exec`，避免用户失去对话能力。
                eprintln!("[codex-app-server] fallback to codex exec: {reason}");
            }
            CodexAppServerAttempt::ThreadBusy(reason) => {
                // 同一 thread 已有 writer 时绝不能再降级到 `codex exec resume`，否则
                // 只会制造第二次争抢，并把 JSON-RPC 原始错误暴露给用户。
                permit::sweep_conv(&perm.pending_dir, &turn_id);
                return failed_turn_result(&session_ref, reason, &channel);
            }
        }
    }
    let build: Result<(Command, Option<String>, Option<ScopedPromptFile>), String> =
        if brain == "codex" {
            build_codex(
                &conn,
                &model,
                &effort,
                &session_ref,
                is_first,
                system.as_deref(),
                &message,
                &api_key,
                &work_dir,
                mode,
                lease.as_ref(),
            )
            .map(|(cmd, prompt)| (cmd, Some(prompt), None))
        } else {
            build_claude(
                &conn,
                &model,
                &effort,
                &session_ref,
                is_first,
                system.as_deref(),
                &message,
                &api_key,
                &work_dir,
                &perm,
                lease.as_ref(),
                plugin_dir.as_deref(),
            )
            .map(|(cmd, prompt, prompt_file)| (cmd, Some(prompt), prompt_file))
        };
    let (mut cmd, stdin_payload, _prompt_file) = match build {
        Ok(c) => c,
        Err(e) => {
            let _ = channel.send(TurnEvent::Done {
                ok: false,
                error: e.clone(),
            });
            return TurnResult {
                ok: false,
                text: String::new(),
                tools: vec![],
                error: e,
                session_ref,
                usage: None,
                limit_reset: None,
            };
        }
    };

    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：跑 CLI 不弹控制台

    // 续跑会话只读取本轮追加内容，避免把历史活动重新投进当前卡片。
    // 首轮要等 thread.started 给出 session_ref，再从新建 rollout 的开头读取。
    let initial_rollout = (brain == "codex")
        .then(|| find_codex_rollout(&session_ref))
        .flatten();
    let initial_rollout_offset = initial_rollout
        .as_ref()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("启动 {brain} 失败（确认已安装并登录）: {e}");
            let _ = channel.send(TurnEvent::Done {
                ok: false,
                error: msg.clone(),
            });
            return TurnResult {
                ok: false,
                text: String::new(),
                tools: vec![],
                error: msg,
                session_ref,
                usage: None,
                limit_reset: None,
            };
        }
    };

    // Windows npm 全局安装的 Claude/Codex 入口是 .cmd。Rust 为防 cmd.exe 注入会拒绝
    // 给 .cmd/.bat 传递含 CR/LF 的普通参数（报 "batch file arguments are invalid"）。
    // 两种 CLI 都统一从 stdin 读取用户提示，既保留完整多行文本，也避开批处理参数限制。
    let stdin_task = if let Some(payload) = stdin_payload {
        let Some(mut stdin) = child.stdin.take() else {
            let pid = child.id();
            kill_tree(pid);
            let _ = child.kill().await;
            let _ = child.wait().await;
            let msg = format!("启动 {brain} 失败：stdin 不可用");
            let _ = channel.send(TurnEvent::Done {
                ok: false,
                error: msg.clone(),
            });
            return TurnResult {
                ok: false,
                text: String::new(),
                tools: vec![],
                error: msg,
                session_ref,
                usage: None,
                limit_reset: None,
            };
        };
        Some(tauri::async_runtime::spawn(async move {
            stdin.write_all(payload.as_bytes()).await
        }))
    } else {
        None
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let pid = child.id();
    let (_canceled, mut kill_rx) = registry.register(&turn_id);

    let collect = Arc::new(Mutex::new(Collect {
        text: String::new(),
        tools: Vec::new(),
        session_ref: session_ref.clone(),
        is_error: false,
        usage: None,
        raw_tail: String::new(),
        images: Vec::new(),
    }));
    let is_codex = brain == "codex";
    let out_task = stdout.map(|s| parse_stream(s, is_codex, channel.clone(), collect.clone()));
    let err_buf = Arc::new(Mutex::new(String::new()));
    let err_task = stderr.map(|s| collect_lines(s, err_buf.clone()));
    let native_stop = Arc::new(AtomicBool::new(false));
    let native_task = is_codex.then(|| {
        watch_codex_rollout(
            collect.clone(),
            channel.clone(),
            native_stop.clone(),
            initial_rollout,
            initial_rollout_offset,
        )
    });

    let status = tokio::select! {
        s = child.wait() => s.ok(),
        _ = &mut kill_rx => {
            kill_tree(pid);
            let _ = child.kill().await;
            child.wait().await.ok()
        }
    };
    let _ = pid;
    let stdin_error = match stdin_task {
        Some(task) => match task.await {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(format!("向 {brain} 发送提示词失败: {e}")),
            Err(e) => Some(format!("向 {brain} 发送提示词任务失败: {e}")),
        },
        None => None,
    };
    if let Some(t) = out_task {
        let _ = t.await;
    }
    if let Some(t) = err_task {
        let _ = t.await;
    }
    native_stop.store(true, Ordering::Relaxed);
    if let Some(t) = native_task {
        let _ = t.await;
    }
    // 必须在 unregister 之前读取取消标记——移除句柄后 is_canceled 恒为 false。
    let canceled = registry.is_canceled(&turn_id);
    registry.unregister(&turn_id);
    // 清掉本会话遗留的待批请求（取消时钩子被杀不会自删，否则下次冒幽灵批准卡）。
    permit::sweep_conv(&perm.pending_dir, &turn_id);

    let c = collect.lock().unwrap().clone();
    let err_text = err_buf.lock().unwrap().clone();
    let proc_ok = status.map(|s| s.success()).unwrap_or(false);
    let ok = proc_ok && !c.is_error && !canceled && stdin_error.is_none();

    let error = if canceled {
        "已停止".to_string()
    } else if !ok {
        // 诊断优先级：stderr 最后一行 → stdout 里最后一条非 JSON 行（CLI 崩溃常打在这）→ 兜底（带退出码 + 自救指引）。
        last_nonempty(&err_text)
            .or_else(|| {
                let t = c.raw_tail.trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            })
            .or(stdin_error)
            .or_else(|| {
                if c.text.trim().is_empty() {
                    let code = status
                        .and_then(|s| s.code())
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "未知/被信号终止".into());
                    Some(format!(
                        "模型没有产生输出（进程退出码：{code}）。若重试仍然如此，多半是这条会话的底层状态损坏——\
点「重建继续」换一个新会话原地续跑（历史和项目文件都会保留）。"
                    ))
                } else {
                    None
                }
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let error = clarify_claude_cli_error(&brain, error).to_string();

    let persisted_text = c.text.to_string();
    let clean_text = persisted_text.trim().to_string();
    // 生图产物：只补真实存在的文件（模型可能提了没写出来的路径）。
    let clean_text = if ok && !c.images.is_empty() {
        let existing: Vec<String> = c
            .images
            .iter()
            .filter(|p| std::path::Path::new(p.as_str()).is_file())
            .cloned()
            .collect();
        append_generated_images(&clean_text, &existing)
    } else {
        clean_text
    };

    // 限额识别只看失败回合的错误材料（stderr + 结构化错误 + stdout 尾行）。
    let limit_reset = if ok {
        None
    } else {
        detect_usage_limit(&format!("{error}\n{err_text}\n{}", c.raw_tail))
    };

    let _ = channel.send(TurnEvent::Done {
        ok,
        error: error.clone(),
    });
    TurnResult {
        ok,
        text: clean_text,
        tools: c
            .tools
            .into_iter()
            .map(|mut tool| {
                tool.detail = tool.detail.to_string();
                tool
            })
            .collect(),
        error,
        session_ref: c.session_ref,
        usage: c.usage,
        limit_reset,
    }
}

/// 外部安装的旧版 Claude Code 可能还不认识提示词文件参数。保留原始错误无法让普通用户
/// 判断是版本问题；只对明确命中该参数的“未知选项”错误换成可执行的升级提示。
fn clarify_claude_cli_error(brain: &str, error: String) -> String {
    if brain != "claude" {
        return error;
    }
    let lower = error.to_ascii_lowercase();
    let context_limit = [
        "prompt is too long",
        "context window",
        "context length",
        "maximum context",
        "max context",
        "too many tokens",
        "input is too long",
        "request too large",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || (error.contains("上下文")
            && (error.contains("超限") || error.contains("上限") || error.contains("过长")));
    if context_limit {
        return format!(
            "Claude 的当前底层会话已达到上下文上限。Pilot 中的聊天记录仍完整保留；请点击「重建继续」，Pilot 会用历史记录接入一个新的 Claude 会话。原始错误：{error}"
        );
    }
    let unknown_option = [
        "unknown option",
        "unknown argument",
        "unrecognized option",
        "unrecognized argument",
        "unexpected argument",
        "invalid option",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if lower.contains("append-system-prompt-file") && unknown_option {
        format!(
            "当前 Claude Code CLI 版本过旧，不支持 Pilot 所需的系统提示参数；请更新 Claude Code 后重新检测再试。原始错误：{error}"
        )
    } else {
        error
    }
}

#[derive(Clone)]
struct Collect {
    text: String,
    tools: Vec<ToolCall>,
    session_ref: String,
    is_error: bool,
    usage: Option<TurnUsage>,
    /// stdout 里最后一条**非 JSON** 行：CLI 崩溃/报错时常直接打纯文本到 stdout，
    /// 不留这个的话失败原因会被静默吞掉，只剩一句"模型没有产生输出"。
    raw_tail: String,
    /// codex 生图工具的产物路径（事件流里带 generated_images 目录的图片路径）：
    /// 工具结果不进对话正文，收集后在回合末尾补成「生成的图片」清单给前端渲染缩略图。
    images: Vec<String>,
}

/// 递归扫事件 JSON 的字符串值，抽生图产物路径：含 generated_images 目录 + 图片扩展名
/// 结尾的 token。只认生图目录，不把命令输出里的普通路径当图。
fn extract_generated_images(v: &serde_json::Value, out: &mut Vec<String>) {
    const IMG_EXTS: [&str; 8] = [
        ".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg", ".avif",
    ];
    match v {
        serde_json::Value::String(s) if s.contains("generated_images") => {
            for tok in s.split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '"' | '\'' | '(' | ')' | '[' | ']' | '`' | ',' | ';' | '<' | '>'
                    )
            }) {
                let t = tok.trim_end_matches(['.', '。', '，', '：', ':']);
                if t.contains("generated_images")
                    && IMG_EXTS.iter().any(|e| t.to_ascii_lowercase().ends_with(e))
                    && !out.iter().any(|x| x == t)
                {
                    out.push(t.to_string());
                }
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                extract_generated_images(x, out);
            }
        }
        serde_json::Value::Object(o) => {
            for x in o.values() {
                extract_generated_images(x, out);
            }
        }
        _ => {}
    }
}

/// 把正文没提到的生图产物补成「生成的图片」清单（前端按此标记渲染缩略图）。
/// images 需由调用方先过滤成真实存在的文件。
fn append_generated_images(text: &str, images: &[String]) -> String {
    let fresh: Vec<&String> = images
        .iter()
        .filter(|p| !text.contains(p.as_str()))
        .take(12)
        .collect();
    if fresh.is_empty() {
        return text.to_string();
    }
    let mut out = text.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("生成的图片：");
    for p in fresh {
        out.push_str("\n- ");
        out.push_str(p);
    }
    out
}

/// 从 usage 对象抽 token 数。兼容 Anthropic（input_tokens/…）与 OpenAI 风格（prompt/completion_tokens）。缺失按 0。
fn parse_usage(v: &serde_json::Value) -> Option<TurnUsage> {
    if !v.is_object() {
        return None;
    }
    let g = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_u64()))
            .unwrap_or(0)
    };
    let u = TurnUsage {
        input: g(&["input_tokens", "prompt_tokens", "inputTokens"]),
        output: g(&["output_tokens", "completion_tokens", "outputTokens"]),
        cache_read: g(&[
            "cache_read_input_tokens",
            "cached_input_tokens",
            "cachedInputTokens",
        ]),
        cache_create: g(&["cache_creation_input_tokens", "cacheCreationInputTokens"]),
    };
    if u.input + u.output + u.cache_read + u.cache_create == 0 {
        None
    } else {
        Some(u)
    }
}

/// 按连接类型注入凭据 env + 设置 cwd：
///   cloudflare → CLOUDFLARE_API_TOKEN/ACCOUNT_ID，cwd=项目目录；
///   github     → GITHUB_TOKEN/GH_TOKEN + 仅从环境读值的 git 凭据助手，cwd=仓库检出目录；
///   workspace  → **不注入任何连接凭据**，只设置用户明确选择/会话隔离的 cwd；
///   ssh        → **不注入任何凭据**（那是一台机器的 root shell，不是一个 API key），
///                只给本轮租约的目录+令牌，让 ssh.js 能把命令递给 Pilot 代跑。见 bridge.rs。
pub(crate) fn apply_env_cwd(
    cmd: &mut Command,
    conn: &Connection,
    work_dir: &str,
    api_key: &str,
    lease: Option<&crate::bridge::Lease>,
) {
    cmd.current_dir(work_dir);
    if conn.kind == "workspace" {
        return;
    }
    if conn.kind == "ssh" {
        if let Some(l) = lease {
            for (k, v) in l.env() {
                cmd.env(k, v);
            }
        }
        return;
    }
    if conn.kind == "cloudflare" {
        cmd.env("CLOUDFLARE_API_TOKEN", api_key);
        if !conn.account_id.is_empty() {
            cmd.env("CLOUDFLARE_ACCOUNT_ID", &conn.account_id);
        }
    }
    if conn.kind == "github" {
        for (key, value) in crate::github::git_env(api_key) {
            cmd.env(key, value);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_claude(
    conn: &Connection,
    model: &str,
    effort: &str,
    session_ref: &str,
    is_first: bool,
    system: Option<&str>,
    message: &str,
    api_key: &str,
    work_dir: &str,
    perm: &PermSpec,
    lease: Option<&crate::bridge::Lease>,
    plugin_dir: Option<&std::path::Path>,
) -> Result<(Command, String, Option<ScopedPromptFile>), String> {
    // 空 → 默认档位；别名（sonnet/opus/haiku）或完整模型 ID（如 claude-opus-4-8）都放行，
    // 只挡形似参数/含空白的非法值。claude --model 同时接受别名与完整 ID。
    let model = model.trim();
    let model = if model.is_empty() {
        // 显式 ID 而非别名 'sonnet'：别名由 CLI 解析且会落后（2.1.96 解析成 4.6），
        // 与 Pilot 界面「Sonnet 5」的承诺对不上。
        "claude-sonnet-5"
    } else if model.starts_with('-') || model.contains(char::is_whitespace) {
        return Err(format!("无效的模型标识: {model}"));
    } else {
        model
    };
    // 先完成其它可能失败的构建步骤，再落临时提示词文件，避免中途返回时留下残片。
    let perm_flags = permit::claude_flags(
        perm.mode,
        &perm.conv_id,
        &perm.gen_dir,
        &perm.pending_dir,
        &perm.ssh_js,
        perm.fast,
    )?;
    let prompt_file = if is_first {
        system
            .map(|sys| ScopedPromptFile::create(&perm.gen_dir, sys))
            .transpose()?
    } else {
        None
    };
    // 用检测同款的路径解析：Windows 上裸名找不到 .cmd/.exe 之外的安装形态。
    let mut cmd = Command::new(crate::brains::resolve_bin("claude"));
    // `-p` 不带位置参数时从 stdin 读取。消息不能进 argv：续轮粘贴多行文本同样会让
    // Windows 的 claude.cmd 在 CreateProcess 前失败。
    cmd.arg("-p").args(["--input-format", "text"]);
    if is_first {
        cmd.args(["--session-id", session_ref]);
        if let Some(file) = prompt_file.as_ref() {
            cmd.arg("--append-system-prompt-file").arg(file.path());
        }
    } else {
        cmd.args(["--resume", session_ref]);
    }
    // ★ 每轮都带（包括 --resume）：技能的价值就在「任意一轮能重新拉取」。只在首轮给等于白做——
    // 系统提示词首轮就有的东西，第 20 轮改版式时早漂没了，那正是要治的病。
    if let Some(p) = plugin_dir {
        cmd.args(["--plugin-dir", &p.to_string_lossy()]);
    }
    cmd.args(["--output-format", "stream-json"])
        .arg("--verbose")
        .arg("--include-partial-messages")
        .args(["--model", model]);
    // 权限档位 → claude 参数（plan/ask/auto/full）；ask/auto 会生成 PreToolUse 钩子 + settings。
    cmd.args(&perm_flags);
    // 直接使用 Claude Code 的原生 adaptive-reasoning 档位。旧实现把三档换算成固定的
    // MAX_THINKING_TOKENS，导致界面“拉满”其实仍只是 high，也无法表达 xhigh/max。
    // 空值不传参数，继续跟随所选模型默认档位。
    if matches!(effort, "low" | "medium" | "high" | "xhigh" | "max") {
        cmd.args(["--effort", effort]);
    }
    // Claude Code 原生 auto-compact 会在**同一个 session**内清理旧工具输出并摘要历史，
    // 不会像 Pilot 旧逻辑那样换 UUID。给它留 10% 余量，避免大文件/工具输出在默认约 95%
    // 的临界点直接顶爆；用户显式禁用或自定义阈值时尊重其环境设置。
    if std::env::var_os("DISABLE_AUTO_COMPACT").is_none()
        && std::env::var_os("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE").is_none()
    {
        cmd.env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "90");
    }
    apply_env_cwd(&mut cmd, conn, work_dir, api_key, lease);
    // 用户在 Pilot 中粘贴的 Claude 订阅 Token 只存在系统安全存储；每轮启动 Claude
    // 时临时注入，不写 shell profile、settings.json 或对话记录。
    if let Some(token) = keychain::claude_oauth_token()? {
        cmd.env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("ANTHROPIC_API_KEY")
            .env("CLAUDE_CODE_OAUTH_TOKEN", token);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok((cmd, message.to_string(), prompt_file))
}

#[allow(clippy::too_many_arguments)]
fn build_codex(
    conn: &Connection,
    model: &str,
    effort: &str,
    session_ref: &str,
    is_first: bool,
    system: Option<&str>,
    message: &str,
    api_key: &str,
    work_dir: &str,
    perm: PermMode,
    lease: Option<&crate::bridge::Lease>,
) -> Result<(Command, String), String> {
    // codex 无头没有逐工具回传 UI 的能力，权限档位只能落到 sandbox 粗粒度（精细批准以 claude 为主）。
    // plan＝只读；full＝完全放开；ask/auto＝可写工作区（无法逐命令确认）。
    let sandbox = match perm {
        PermMode::Plan => "read-only",
        PermMode::Full => "danger-full-access",
        PermMode::Ask | PermMode::Auto => "workspace-write",
    };
    // codex 没有独立系统提示位：首轮把角色框架并进消息（前端只展示用户原话）。
    let prompt = if is_first {
        match system {
            Some(sys) => format!("{sys}\n\n——\n用户：{message}"),
            None => message.to_string(),
        }
    } else {
        message.to_string()
    };
    // 同上：npm 装的 codex 在 Windows 上是 codex.cmd，必须用完整路径 spawn。
    let mut cmd = Command::new(crate::brains::resolve_bin("codex"));
    cmd.arg("exec");
    if is_first {
        cmd.arg("--json")
            .args(["--sandbox", sandbox])
            .args(["-c", "sandbox_workspace_write.network_access=true"])
            .arg("--skip-git-repo-check");
    } else {
        cmd.arg("resume")
            .arg(session_ref)
            .arg("--json")
            .arg("--skip-git-repo-check")
            .args(["-c", &format!("sandbox_mode={sandbox}")])
            .args(["-c", "sandbox_workspace_write.network_access=true"]);
    }
    // 自定义模型 ID（可选）：非空则用 -c model=<id> 覆盖 codex 本地默认；留空用其默认。
    // 与既有 -c sandbox_* 一致用裸字符串；含空白/形似参数的非法值直接拒。
    let model = model.trim();
    if !model.is_empty() {
        if model.starts_with('-') || model.contains(char::is_whitespace) {
            return Err(format!("无效的模型标识: {model}"));
        }
        cmd.args(["-c", &format!("model={model}")]);
    }
    // Normalize a retained higher tier when switching from Sol/Astra to Luna.
    let effort = codex_effort(model, effort);
    if matches!(
        effort,
        "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    ) {
        cmd.args(["-c", &format!("model_reasoning_effort={effort}")]);
    }
    // `-` 让 Codex 从 stdin 读完整提示词。不要把 prompt 直接作为参数：Windows 的
    // codex.cmd 无法接收含换行的参数，首轮系统提示必然会触发该限制。
    cmd.arg("-");
    apply_env_cwd(&mut cmd, conn, work_dir, api_key, lease);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok((cmd, prompt))
}

fn parse_stream(
    reader: impl AsyncRead + Unpin + Send + 'static,
    is_codex: bool,
    ch: Channel<TurnEvent>,
    collect: Arc<Mutex<Collect>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut r = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let cow = String::from_utf8_lossy(&buf);
                    let line = cow.trim_end_matches(['\n', '\r']);
                    let Ok(ev) = serde_json::from_str::<serde_json::Value>(line) else {
                        // CLI 崩溃/报错常直接打纯文本到 stdout；留住最后一条非 JSON 行当诊断线索。
                        let t = line.trim();
                        if !t.is_empty() {
                            let mut c = collect.lock().unwrap();
                            c.raw_tail = t.chars().take(300).collect();
                        }
                        continue;
                    };
                    if is_codex {
                        parse_codex(&ev, &ch, &collect);
                    } else {
                        parse_claude(&ev, &ch, &collect);
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn find_codex_rollout(session_ref: &str) -> Option<PathBuf> {
    if session_ref.trim().is_empty() {
        return None;
    }
    let root = codex_home()?.join("sessions");
    fn visit(dir: &Path, needle: &str, depth: usize) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        }
        for entry in fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = visit(&path, needle, depth + 1) {
                    return Some(found);
                }
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&format!("{needle}.jsonl")))
            {
                return Some(path);
            }
        }
        None
    }
    visit(&root, session_ref, 0)
}

fn native_activity(
    ev: &serde_json::Value,
) -> Option<(String, String, String, String, String, String)> {
    let outer = ev.get("type").and_then(|value| value.as_str())?;
    let payload = ev.get("payload")?;
    let event_type = payload.get("type").and_then(|value| value.as_str())?;
    match (outer, event_type) {
        ("response_item", "reasoning") => Some((
            "native:reasoning".into(),
            "reasoning".into(),
            "正在分析与规划".into(),
            String::new(),
            String::new(),
            "running".into(),
        )),
        ("response_item", "function_call" | "custom_tool_call") => {
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(|value| value.as_str())?;
            let name = payload
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("tool");
            let kind = activity_kind(name);
            let phase = match kind {
                "image_generation" => "正在生成图片",
                "image_review" => "正在检查图片",
                "skill" => "正在使用技能",
                "command" => native_command_phase(payload),
                _ => "正在调用工具",
            };
            Some((
                call_id.into(),
                kind.into(),
                name.into(),
                phase.into(),
                String::new(),
                "running".into(),
            ))
        }
        ("response_item", "function_call_output" | "custom_tool_call_output") => {
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(|value| value.as_str())?;
            let failed = payload.get("success").and_then(|value| value.as_bool()) == Some(false)
                || payload.get("status").and_then(|value| value.as_str()) == Some("failed");
            Some((
                call_id.into(),
                String::new(),
                String::new(),
                String::new(),
                if failed {
                    failure_detail(payload)
                } else {
                    String::new()
                },
                if failed { "failed" } else { "completed" }.into(),
            ))
        }
        ("event_msg", "image_generation_end") => {
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(|value| value.as_str())?;
            let failed = payload.get("status").and_then(|value| value.as_str()) == Some("failed");
            Some((
                call_id.into(),
                "image_generation".into(),
                "imagegen".into(),
                if failed {
                    "图片生成未成功"
                } else {
                    "图片已生成"
                }
                .into(),
                if failed {
                    failure_detail(payload)
                } else {
                    String::new()
                },
                if failed { "failed" } else { "completed" }.into(),
            ))
        }
        // agent_message 已经通过 stdout 的 Delta 进入助手正文。不能再把同一段自然语言
        // 伪装成持续 running 的活动，否则正文会在任务卡里重复一次，而且这个
        // native:phase 会一直压住后续真实的命令/技能/图片活动。
        ("event_msg", "agent_message") => None,
        ("event_msg", "task_complete") => Some((
            "native:phase".into(),
            "progress".into(),
            "任务进展".into(),
            "本轮任务已完成".into(),
            String::new(),
            "completed".into(),
        )),
        _ => None,
    }
}

/// 把 Codex 的命令调用翻译成用户能理解的业务阶段。这里只返回固定的安全文案，
/// 不把原始 shell、路径、Token 或参数送进活动卡。
fn native_command_phase(payload: &serde_json::Value) -> &'static str {
    let input = payload
        .get("arguments")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("command"))
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_default()
        .to_lowercase();
    if input.contains("webp")
        || input.contains("identify ")
        || input.contains("view_image")
        || input.contains("image")
    {
        return "正在检查图片文件";
    }
    "正在处理任务步骤"
}

fn send_native_activity(ev: &serde_json::Value, ch: &Channel<TurnEvent>) {
    let Some((activity_id, kind, label, phase, error, status)) = native_activity(ev) else {
        return;
    };
    if status == "running" && activity_id != "native:reasoning" {
        let _ = ch.send(TurnEvent::Activity {
            activity_id: "native:reasoning".into(),
            label: String::new(),
            detail: String::new(),
            error: String::new(),
            status: "completed".into(),
            kind: "reasoning".into(),
            phase: String::new(),
            current: None,
            total: None,
        });
    }
    let _ = ch.send(TurnEvent::Activity {
        activity_id,
        label,
        detail: String::new(),
        error,
        status,
        kind,
        phase,
        current: None,
        total: None,
    });
}

fn watch_codex_rollout(
    collect: Arc<Mutex<Collect>>,
    ch: Channel<TurnEvent>,
    stop: Arc<AtomicBool>,
    initial_path: Option<PathBuf>,
    initial_offset: u64,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut path = initial_path;
        let mut offset = initial_offset;
        let mut seen = HashSet::<(String, String, String)>::new();
        // 还没找到 rollout 文件时要递归扫 ~/.codex/sessions：放到阻塞线程做，并且越找不到扫得越稀
        // （300 ms 起、翻倍到 5 s），不然几百个会话目录每 300 ms 扫一遍把运行时卡住
        let mut search_wait = std::time::Duration::from_millis(300);
        loop {
            if path.is_none() {
                let session_ref = collect.lock().unwrap().session_ref.clone();
                path = tokio::task::spawn_blocking(move || find_codex_rollout(&session_ref)).await.ok().flatten();
                if path.is_some() {
                    offset = 0;
                }
            }
            if let Some(current_path) = path.as_ref() {
                if let Ok(mut file) = tokio::fs::File::open(current_path).await {
                    if file.seek(SeekFrom::Start(offset)).await.is_ok() {
                        let mut reader = BufReader::new(file);
                        loop {
                            let mut line = Vec::new();
                            let Ok(read) = reader.read_until(b'\n', &mut line).await else {
                                break;
                            };
                            if read == 0 || line.last() != Some(&b'\n') {
                                break;
                            }
                            offset += read as u64;
                            // 图片工具输出可能含巨型 data URL；这些行不需要进入 UI。
                            if line.len() > 2 * 1024 * 1024 {
                                continue;
                            }
                            let Ok(ev) = serde_json::from_slice::<serde_json::Value>(&line) else {
                                continue;
                            };
                            let Some((activity_id, _, _, phase, _, status)) = native_activity(&ev)
                            else {
                                continue;
                            };
                            if seen.insert((activity_id, status, phase)) {
                                send_native_activity(&ev, &ch);
                            }
                        }
                    }
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let wait = if path.is_some() {
                std::time::Duration::from_millis(300)
            } else {
                let w = search_wait;
                search_wait = (search_wait * 2).min(std::time::Duration::from_secs(5));
                w
            };
            tokio::time::sleep(wait).await;
        }
    })
}

fn parse_claude(ev: &serde_json::Value, ch: &Channel<TurnEvent>, collect: &Arc<Mutex<Collect>>) {
    send_claude_tool_result_activity(ev, ch);
    match ev.get("type").and_then(|t| t.as_str()) {
        Some("system") if claude_compact_pre_tokens(ev).is_some() => {
            let pre_tokens = claude_compact_pre_tokens(ev).unwrap_or(0);
            let _ = ch.send(TurnEvent::ContextCompacted { pre_tokens });
        }
        Some("stream_event") => {
            let e = &ev["event"];
            if e.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                if e["delta"].get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                    let _ = ch.send(TurnEvent::Activity {
                        activity_id: "native:reasoning".into(),
                        label: String::new(),
                        detail: String::new(),
                        error: String::new(),
                        status: "completed".into(),
                        kind: "reasoning".into(),
                        phase: String::new(),
                        current: None,
                        total: None,
                    });
                    if let Some(t) = e["delta"].get("text").and_then(|t| t.as_str()) {
                        collect.lock().unwrap().text.push_str(t);
                        let _ = ch.send(TurnEvent::Delta {
                            text: t.to_string(),
                        });
                    }
                } else if matches!(
                    e["delta"].get("type").and_then(|t| t.as_str()),
                    Some("thinking_delta" | "signature_delta")
                ) {
                    // 只暴露“正在思考”的语义脉冲，不把供应商思维链或签名内容送到 UI。
                    let _ = ch.send(TurnEvent::Activity {
                        activity_id: "native:reasoning".into(),
                        label: "reasoning".into(),
                        detail: String::new(),
                        error: String::new(),
                        status: "running".into(),
                        kind: "reasoning".into(),
                        phase: "正在分析与规划".into(),
                        current: None,
                        total: None,
                    });
                }
            }
        }
        Some("assistant") => {
            if let Some(u) = parse_usage(&ev["message"]["usage"]) {
                collect.lock().unwrap().usage = Some(u); // 每条 assistant 消息刷新，最后一条＝本轮最终上下文
            }
            if let Some(blocks) = ev["message"]["content"].as_array() {
                for b in blocks {
                    if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        let detail = tool_detail(name, &b["input"]);
                        if let Some(activity_id) = b.get("id").and_then(|id| id.as_str()) {
                            let _ = ch.send(TurnEvent::Activity {
                                activity_id: activity_id.to_string(),
                                label: name.to_string(),
                                detail: b
                                    .pointer("/input/description")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or(&detail)
                                    .chars()
                                    .take(200)
                                    .collect(),
                                error: String::new(),
                                status: "running".into(),
                                kind: activity_kind(name).into(),
                                phase: String::new(),
                                current: None,
                                total: None,
                            });
                        }
                        {
                            let mut g = collect.lock().unwrap();
                            let at = crate::convo::utf16_len(&g.text);
                            g.tools.push(ToolCall { label: name.into(), detail: detail.clone(), at });
                        }
                        let _ = ch.send(TurnEvent::Tool {
                            label: name.into(),
                            detail,
                        });
                    }
                }
            }
        }
        Some("result") => {
            let mut c = collect.lock().unwrap();
            // 优先用最后一条 assistant 消息的 usage（＝最终那次模型调用读入的整段上下文＝当前会话大小）；
            // result.usage 在多步回合里可能是各步汇总，会高估上下文，故仅在没抓到 assistant usage 时兜底。
            if c.usage.is_none() {
                if let Some(u) = parse_usage(&ev["usage"]) {
                    c.usage = Some(u);
                }
            }
            if ev
                .get("is_error")
                .and_then(|b| b.as_bool())
                .unwrap_or(false)
            {
                c.is_error = true;
            }
            if c.text.trim().is_empty() {
                if let Some(r) = ev.get("result").and_then(|r| r.as_str()) {
                    c.text = r.to_string();
                }
            }
        }
        _ => {}
    }
}

fn send_claude_tool_result_activity(ev: &serde_json::Value, ch: &Channel<TurnEvent>) {
    let send_result = |block: &serde_json::Value| {
        let Some((activity_id, status, error)) = claude_tool_result_activity(block) else {
            return;
        };
        let _ = ch.send(TurnEvent::Activity {
            activity_id,
            label: String::new(),
            detail: String::new(),
            error,
            status,
            kind: String::new(),
            phase: String::new(),
            current: None,
            total: None,
        });
    };
    match ev.get("type").and_then(|value| value.as_str()) {
        Some("user") => {
            if let Some(blocks) = ev
                .pointer("/message/content")
                .and_then(|value| value.as_array())
            {
                for block in blocks {
                    send_result(block);
                }
            }
        }
        Some("tool_result") => send_result(ev),
        _ => {}
    }
}

fn claude_tool_result_activity(block: &serde_json::Value) -> Option<(String, String, String)> {
    if block.get("type").and_then(|value| value.as_str()) != Some("tool_result") {
        return None;
    }
    let activity_id = block
        .get("tool_use_id")
        .or_else(|| block.get("id"))
        .and_then(|value| value.as_str())?;
    let failed = block
        .get("is_error")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Some((
        activity_id.to_string(),
        if failed { "failed" } else { "completed" }.into(),
        if failed {
            failure_detail(block)
        } else {
            String::new()
        },
    ))
}

fn claude_compact_pre_tokens(ev: &serde_json::Value) -> Option<u64> {
    (ev.get("type").and_then(|t| t.as_str()) == Some("system")
        && ev.get("subtype").and_then(|t| t.as_str()) == Some("compact_boundary"))
    .then(|| {
        ev.pointer("/compact_metadata/pre_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    })
}

fn parse_codex(ev: &serde_json::Value, ch: &Channel<TurnEvent>, collect: &Arc<Mutex<Collect>>) {
    // 生图工具的产物只出现在工具结果事件里、不进对话正文——从所有事件里顺手收集。
    {
        let mut imgs = Vec::new();
        extract_generated_images(ev, &mut imgs);
        if !imgs.is_empty() {
            let mut c = collect.lock().unwrap();
            for i in imgs {
                if !c.images.contains(&i) {
                    c.images.push(i);
                }
            }
        }
    }
    // codex 某些版本在事件里带 token 统计，尽力抽一下（拿不到就 None，前端会隐藏上下文条）。
    if let Some(u) = parse_usage(&ev["usage"])
        .or_else(|| parse_usage(&ev["item"]["usage"]))
        .or_else(|| parse_usage(&ev["info"]["usage"]))
    {
        collect.lock().unwrap().usage = Some(u);
    }
    match ev.get("type").and_then(|t| t.as_str()) {
        Some("thread.started") => {
            if let Some(id) = ev.get("thread_id").and_then(|i| i.as_str()) {
                collect.lock().unwrap().session_ref = id.to_string();
            }
        }
        Some("item.started") => {
            send_codex_activity(&ev["item"], "running", ch);
        }
        Some("item.completed") => {
            let it = &ev["item"];
            send_codex_activity(
                it,
                if it.get("status").and_then(|value| value.as_str()) == Some("failed") {
                    "failed"
                } else {
                    "completed"
                },
                ch,
            );
            match it.get("type").and_then(|t| t.as_str()) {
                Some("agent_message") => {
                    if let Some(t) = it.get("text").and_then(|t| t.as_str()) {
                        let mut c = collect.lock().unwrap();
                        if !c.text.is_empty() {
                            c.text.push_str("\n\n");
                        }
                        c.text.push_str(t);
                        let _ = ch.send(TurnEvent::Delta {
                            text: t.to_string(),
                        });
                    }
                }
                Some("command_execution") => {
                    let d = it
                        .get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(200)
                        .collect::<String>();
                    {
                        let mut g = collect.lock().unwrap();
                        let at = crate::convo::utf16_len(&g.text);
                        g.tools.push(ToolCall { label: "exec".into(), detail: d.clone(), at });
                    }
                    let _ = ch.send(TurnEvent::Tool {
                        label: "exec".into(),
                        detail: d,
                    });
                }
                Some("error") => {
                    collect.lock().unwrap().is_error = true;
                }
                _ => {}
            }
        }
        Some("error") | Some("turn.failed") => {
            collect.lock().unwrap().is_error = true;
        }
        _ => {}
    }
}

fn send_codex_activity(item: &serde_json::Value, status: &str, ch: &Channel<TurnEvent>) {
    let Some((activity_id, label, detail, error, status)) = codex_activity(item, status) else {
        return;
    };
    let _ = ch.send(TurnEvent::Activity {
        activity_id,
        kind: activity_kind(&label).into(),
        label,
        detail,
        error,
        status,
        phase: String::new(),
        current: None,
        total: None,
    });
}

/// 把不同供应商的工具名归一成稳定、低敏感度的活动类别。
/// 这里只按真实工具名分类，不从模型自然语言里猜测业务阶段或完成数量。
pub(crate) fn activity_kind(label: &str) -> &'static str {
    let key = label.to_ascii_lowercase();
    if key.contains("imagegen") || key.contains("image_gen") || key.contains("generate_image") {
        "image_generation"
    } else if key.contains("view_image")
        || key.contains("image_view")
        || key.contains("inspect_image")
    {
        "image_review"
    } else if key.contains("skill") {
        "skill"
    } else if key == "exec" || key == "bash" || key.contains("command") || key.contains("terminal")
    {
        "command"
    } else if key.contains("reason") || key.contains("thinking") {
        "reasoning"
    } else {
        "tool"
    }
}

fn codex_activity(
    item: &serde_json::Value,
    status: &str,
) -> Option<(String, String, String, String, String)> {
    let item_type = item
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !matches!(
        item_type,
        "command_execution"
            | "mcp_tool_call"
            | "tool_call"
            | "dynamic_tool_call"
            | "function_call"
            | "function_call_output"
    ) {
        return None;
    }
    let activity_id = ["id", "call_id", "tool_call_id"]
        .iter()
        .find_map(|key| item.get(*key).and_then(|value| value.as_str()))?;
    let label = item.get("name").and_then(|value| value.as_str()).unwrap_or(
        if item_type == "command_execution" {
            "exec"
        } else {
            "tool"
        },
    );
    let detail = ["description", "command", "title"]
        .iter()
        .find_map(|key| item.get(*key).and_then(|value| value.as_str()))
        .unwrap_or("");
    Some((
        activity_id.to_string(),
        label.to_string(),
        detail.chars().take(200).collect(),
        if status == "failed" {
            failure_detail(item)
        } else {
            String::new()
        },
        status.to_string(),
    ))
}

/// 从三家 CLI/ACP 的失败结果里提取一条适合界面展示的真实原因。
/// 只读明确的错误/输出字段，不回显整段输入参数，避免把密钥或大块 JSON 带进 UI。
pub(crate) fn failure_detail(value: &serde_json::Value) -> String {
    fn text(value: &serde_json::Value, depth: usize) -> Option<String> {
        if depth > 4 {
            return None;
        }
        if let Some(value) = value.as_str() {
            let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if !compact.is_empty() {
                return Some(compact);
            }
        }
        if let Some(items) = value.as_array() {
            return items.iter().find_map(|item| text(item, depth + 1));
        }
        let object = value.as_object()?;
        for key in [
            "message",
            "error",
            "stderr",
            "aggregated_output",
            "output",
            "content",
            "result",
            "text",
        ] {
            if let Some(found) = object.get(key).and_then(|item| text(item, depth + 1)) {
                return Some(found);
            }
        }
        None
    }

    let detail: String = text(value, 0)
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect();
    detail.to_string()
}

fn tool_detail(name: &str, input: &serde_json::Value) -> String {
    if name == "Bash" {
        if let Some(cmd) = input.get("command").and_then(|c| c.as_str()) {
            return cmd.chars().take(200).collect();
        }
    }
    // Skill＝加载随附技能（如建站设计规范）。不特判的话工具行会显示成一坨裸 JSON。
    if name == "Skill" {
        if let Some(s) = input
            .get("command")
            .or_else(|| input.get("skill"))
            .or_else(|| input.get("name"))
            .and_then(|c| c.as_str())
        {
            return s.chars().take(200).collect();
        }
    }
    input.to_string().chars().take(200).collect()
}

fn collect_lines(
    reader: impl AsyncRead + Unpin + Send + 'static,
    sink: Arc<Mutex<String>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut r = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let cow = String::from_utf8_lossy(&buf);
                    let mut s = sink.lock().unwrap();
                    s.push_str(cow.trim_end_matches(['\n', '\r']));
                    s.push('\n');
                }
                Err(_) => break,
            }
        }
    })
}

pub(crate) fn last_nonempty(s: &str) -> Option<String> {
    s.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ch() -> Channel<TurnEvent> {
        Channel::new(|_| Ok(()))
    }

    #[test]
    fn normal_conversations_cannot_leave_answer_work_in_a_private_background_job() {
        let prompt = conversation_completion_policy("检查 20 个页面", "cloudflare");
        assert!(prompt.starts_with("检查 20 个页面"));
        assert!(prompt.contains("不得为这类工作使用 run_in_background"));
        assert!(prompt.contains("并等待 completed、failed 或 canceled"));
        assert!(prompt.contains("不得让 pending/running 的任务跨出本回合"));
        assert!(prompt.contains("不得假装"));
    }

    #[test]
    fn ssh_conversations_keep_their_explicit_remote_background_policy() {
        let message = "后台编译并稍后查看日志";
        assert_eq!(conversation_completion_policy(message, "ssh"), message);
    }

    #[test]
    fn codex_version_parser_accepts_cli_output_and_prerelease_suffix() {
        assert_eq!(parse_codex_version("codex-cli 0.144.1"), Some((0, 144, 1)));
        assert_eq!(
            parse_codex_version("Codex 0.145.0-beta.1"),
            Some((0, 145, 0))
        );
        assert_eq!(parse_codex_version("not-a-version"), None);
    }

    #[test]
    fn codex_cache_compatibility_detects_new_effort_values_only_for_old_cli() {
        let cache = json!({
            "client_version": "0.145.0",
            "effort": "max"
        });
        assert!(contains_new_effort(&cache));
        assert!((0, 144, 1) < (0, 145, 0));
        assert!(!((0, 145, 0) < (0, 145, 0)));
        assert!(codex_models_missing_field(
            &json!({"models": [{"slug": "gpt-test"}]}),
            "supports_reasoning_summaries"
        ));
        assert!(!codex_models_missing_field(
            &json!({"models": [{"supports_reasoning_summaries": true}]}),
            "supports_reasoning_summaries"
        ));
    }

    #[test]
    fn codex_app_server_validates_model_and_maps_sandbox() {
        assert_eq!(validate_codex_model("").unwrap(), None);
        assert_eq!(
            validate_codex_model("gpt-5.6-sol").unwrap(),
            Some("gpt-5.6-sol".into())
        );
        assert_eq!(
            validate_codex_model("gpt-6-astra").unwrap(),
            Some("gpt-6-astra".into())
        );
        assert!(validate_codex_model("--bad").is_err());
        assert!(validate_codex_model("bad model").is_err());
        assert_eq!(codex_sandbox(PermMode::Plan), "read-only");
        assert_eq!(codex_sandbox(PermMode::Auto), "workspace-write");
        assert_eq!(codex_sandbox(PermMode::Full), "danger-full-access");
    }

    #[test]
    fn new_model_presets_are_passed_to_the_cli_without_aliasing() {
        let conn: Connection = serde_json::from_value(json!({
            "id": "test", "name": "test", "kind": "ssh", "skill_dir": ".",
            "key_prefix": "", "key_kind": "", "created_at": ""
        })).unwrap();
        for (model, effort) in [("gpt-6-sol", "ultra"), ("gpt-6-luna", "max"), ("gpt-6-luna", "ultra")] {
            assert_eq!(validate_codex_model(model).unwrap(), Some(model.into()));
            let (cmd, _) = build_codex(&conn, model, effort, "", true, None,
                "test", "", ".", PermMode::Plan, None).unwrap();
            let args: Vec<String> = cmd.as_std().get_args()
                .map(|arg| arg.to_string_lossy().into_owned()).collect();
            assert!(args.contains(&format!("model={model}")));
            assert!(args.contains(&format!("model_reasoning_effort={}", codex_effort(model, effort))));
        }
        let root = std::env::temp_dir().join(format!("pilot-model-test-{}", uuid::Uuid::new_v4()));
        let perm = PermSpec {
            mode: PermMode::Plan, conv_id: "model-test".into(), gen_dir: root.join("hooks"),
            pending_dir: root.join("pending"), ssh_js: root.join("ssh.js"), fast: false,
        };
        let (cmd, _, prompt_file) = build_claude(&conn, "claude-opus-5-5", "medium",
            "75739ec2-daa6-44c0-930c-30309ca88e45", true, None, "test", "", ".", &perm, None, None).unwrap();
        let args: Vec<String> = cmd.as_std().get_args()
            .map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|pair| pair == ["--model", "claude-opus-5-5"]));
        drop(prompt_file);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_app_server_negotiates_the_capability_required_by_resume() {
        let params = codex_initialize_params();
        assert_eq!(
            params["capabilities"]["experimentalApi"].as_bool(),
            Some(true),
            "excludeTurns 会被 Codex 拒绝，除非初始化时协商 experimentalApi"
        );
    }

    #[test]
    fn codex_active_writer_errors_are_not_sent_to_exec_fallback() {
        for error in [
            "thread/resume failed: thread abc already has an active writer (code -32600)",
            "Thread/Resume: already has an ACTIVE WRITER",
        ] {
            assert!(codex_thread_has_active_writer(error));
        }
        assert!(!codex_thread_has_active_writer(
            "thread/resume failed: thread not found"
        ));
        let message = codex_thread_busy_message();
        assert!(message.contains("上一轮仍在收尾"));
        assert!(!message.contains("active writer"));
        assert!(!message.contains("-32600"));
    }

    #[test]
    fn codex_app_server_streamed_message_is_not_duplicated_on_completion() {
        let collect = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: Vec::new(),
            session_ref: "thread-1".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: Vec::new(),
        }));
        let (done_tx, _done_rx) = oneshot::channel();
        let sink = CodexTurnSink {
            channel: ch(),
            collect: collect.clone(),
            completed: Arc::new(Mutex::new(Some(done_tx))),
            streamed_messages: Arc::new(Mutex::new(HashSet::new())),
        };
        handle_codex_app_notification(
            "item/agentMessage/delta",
            &json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "message-1",
                "delta": "已经完成"
            }),
            &sink,
        );
        handle_codex_app_notification(
            "item/completed",
            &json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "item": {
                    "type": "agentMessage",
                    "id": "message-1",
                    "text": "已经完成"
                }
            }),
            &sink,
        );
        assert_eq!(collect.lock().unwrap().text, "已经完成");
    }

    #[test]
    fn codex_app_server_completion_closes_the_waiter_with_real_status() {
        let collect = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: Vec::new(),
            session_ref: "thread-1".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: Vec::new(),
        }));
        let (done_tx, mut done_rx) = oneshot::channel();
        let sink = CodexTurnSink {
            channel: ch(),
            collect,
            completed: Arc::new(Mutex::new(Some(done_tx))),
            streamed_messages: Arc::new(Mutex::new(HashSet::new())),
        };
        handle_codex_app_notification(
            "turn/completed",
            &json!({
                "threadId": "thread-1",
                "turn": {"id": "turn-1", "status": "completed"}
            }),
            &sink,
        );
        assert_eq!(
            done_rx.try_recv().unwrap()["status"].as_str(),
            Some("completed")
        );
    }

    #[test]
    fn codex_app_server_reads_camel_case_token_usage() {
        let usage = parse_usage(&json!({
            "inputTokens": 120,
            "cachedInputTokens": 80,
            "outputTokens": 16
        }))
        .unwrap();
        assert_eq!(usage.input, 120);
        assert_eq!(usage.cache_read, 80);
        assert_eq!(usage.output, 16);
    }

    /// 目录非空时必须明确告诉模型「先读再改、别推倒重写」——这是内置模板能不能生效的**唯一**开关。
    /// 以前 use_template 把模板拷进去后没人吭声，模型视角永远是空目录，那份起点直接被重写掉。
    #[test]
    fn cf_prompt_tells_model_about_the_seed() {
        let seeded = cf_system_prompt("coffee", "acct", true);
        assert!(seeded.contains("已有起点"), "非空目录必须提示已有起点");
        assert!(
            seeded.contains("别推倒重写"),
            "得明说别重写，否则等于没提示"
        );
        assert!(seeded.contains("index.html"), "要点名先读 index.html");

        let blank = cf_system_prompt("coffee", "acct", false);
        assert!(!blank.contains("已有起点"), "空目录别瞎说有起点");

        // 两种情况都得带上设计底座与技能指针（这条是所有厂商共吃的地板）
        for p in [&seeded, &blank] {
            assert!(p.contains("设计底座"));
            assert!(
                p.contains("web-design"),
                "要指向随附技能，否则规范只活第一轮"
            );
        }
    }

    /// codex 吃不到 --plugin-dir，只能靠提示词里的绝对路径自己去读。
    #[test]
    fn design_prompt_points_at_the_skill_file() {
        let p = design_prompt("/data/plugins/pilot-design/skills/web-design/SKILL.md");
        assert!(p.contains("/data/plugins/pilot-design/skills/web-design/SKILL.md"));
        assert!(p.contains("Read"), "得让它用 Read 打开");
    }

    /// Windows 的 npm shim 是 codex.cmd，Rust 会拒绝给批处理文件传含换行的参数。
    /// 首轮和续轮都必须只把 `-` 放进 argv，并把原始多行提示词留给 stdin。
    #[test]
    fn codex_multiline_prompt_uses_stdin() {
        let conn: Connection = serde_json::from_value(json!({
            "id": "test", "name": "test", "kind": "ssh", "skill_dir": ".",
            "key_prefix": "", "key_kind": "", "created_at": ""
        }))
        .unwrap();

        let (first, first_stdin) = build_codex(
            &conn,
            "",
            "",
            "",
            true,
            Some("系统规则第一行\n系统规则第二行"),
            "用户第一行\n用户第二行",
            "",
            ".",
            PermMode::Auto,
            None,
        )
        .unwrap();
        let first_args: Vec<String> = first
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(first_args.last().map(String::as_str), Some("-"));
        assert!(first_args
            .iter()
            .all(|arg| !arg.contains('\r') && !arg.contains('\n')));
        assert!(first_stdin.contains("系统规则第二行\n\n——\n用户：用户第一行"));

        let (astra, _) = build_codex(
            &conn,
            "gpt-6-astra",
            "ultra",
            "",
            true,
            None,
            "处理复杂任务",
            "",
            ".",
            PermMode::Auto,
            None,
        )
        .unwrap();
        let astra_args: Vec<String> = astra
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(astra_args.iter().any(|arg| arg == "model=gpt-6-astra"));
        assert!(astra_args
            .iter()
            .any(|arg| arg == "model_reasoning_effort=ultra"));

        let (resume, resume_stdin) = build_codex(
            &conn,
            "",
            "",
            "thread-123",
            false,
            None,
            "续聊第一行\n续聊第二行",
            "",
            ".",
            PermMode::Auto,
            None,
        )
        .unwrap();
        let resume_args: Vec<String> = resume
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(resume_args
            .windows(2)
            .any(|pair| pair == ["resume", "thread-123"]));
        assert_eq!(resume_args.last().map(String::as_str), Some("-"));
        assert!(resume_args
            .iter()
            .all(|arg| !arg.contains('\r') && !arg.contains('\n')));
        assert_eq!(resume_stdin, "续聊第一行\n续聊第二行");
    }

    /// Windows npm 安装的入口是 claude.cmd：用户消息和系统提示都不能把换行带进 argv。
    #[test]
    fn claude_multiline_prompts_avoid_batch_arguments() {
        let conn: Connection = serde_json::from_value(json!({
            "id": "test", "name": "test", "kind": "ssh", "skill_dir": ".",
            "key_prefix": "", "key_kind": "", "created_at": ""
        }))
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("pilot-claude-argv-{}", uuid::Uuid::new_v4()));
        let perm = PermSpec {
            mode: PermMode::Plan,
            conv_id: "conv-test".into(),
            gen_dir: root.join("hooks"),
            pending_dir: root.join("pending"),
            ssh_js: root.join("ssh.js"),
            fast: false,
        };

        let system = "系统规则第一行\r\n系统规则第二行";
        let message = "用户第一行\n用户第二行";
        let (first, first_stdin, first_prompt_file) = build_claude(
            &conn,
            "",
            "",
            "75739ec2-daa6-44c0-930c-30309ca88e45",
            true,
            Some(system),
            message,
            "",
            ".",
            &perm,
            None,
            None,
        )
        .unwrap();
        let first_args: Vec<String> = first
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(first_args
            .iter()
            .all(|arg| !arg.contains('\r') && !arg.contains('\n')));
        assert!(!first_args.iter().any(|arg| arg == message));
        assert!(first_args
            .windows(2)
            .any(|pair| pair == ["--input-format", "text"]));
        let prompt_path = first_args
            .windows(2)
            .find(|pair| pair[0] == "--append-system-prompt-file")
            .map(|pair| PathBuf::from(&pair[1]))
            .expect("首轮必须通过文件传系统提示");
        assert_eq!(fs::read_to_string(&prompt_path).unwrap(), system);
        assert_eq!(first_stdin, message);
        assert!(first_prompt_file.is_some());
        drop(first_prompt_file);
        assert!(!prompt_path.exists(), "本轮结束后必须清理系统提示文件");

        let resume_message = "续聊第一行\r\n续聊第二行";
        let (resume, resume_stdin, resume_prompt_file) = build_claude(
            &conn,
            "",
            "",
            "75739ec2-daa6-44c0-930c-30309ca88e45",
            false,
            Some("续轮不应重复注入系统提示"),
            resume_message,
            "",
            ".",
            &perm,
            None,
            None,
        )
        .unwrap();
        let resume_args: Vec<String> = resume
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(resume_args
            .iter()
            .all(|arg| !arg.contains('\r') && !arg.contains('\n')));
        assert!(resume_args
            .windows(2)
            .any(|pair| pair == ["--resume", "75739ec2-daa6-44c0-930c-30309ca88e45"]));
        assert!(!resume_args
            .iter()
            .any(|arg| arg == "--append-system-prompt-file"));
        assert_eq!(resume_stdin, resume_message);
        assert!(resume_prompt_file.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn claude_effort_uses_native_cli_level_instead_of_fixed_token_budget() {
        let conn: Connection = serde_json::from_value(json!({
            "id": "test", "name": "test", "kind": "ssh", "skill_dir": ".",
            "key_prefix": "", "key_kind": "", "created_at": ""
        }))
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("pilot-claude-effort-{}", uuid::Uuid::new_v4()));
        let perm = PermSpec {
            mode: PermMode::Plan,
            conv_id: "conv-effort".into(),
            gen_dir: root.join("hooks"),
            pending_dir: root.join("pending"),
            ssh_js: root.join("ssh.js"),
            fast: false,
        };

        let (cmd, _, prompt_file) = build_claude(
            &conn,
            "claude-opus-5",
            "max",
            "75739ec2-daa6-44c0-930c-30309ca88e45",
            true,
            None,
            "测试",
            "",
            ".",
            &perm,
            None,
            None,
        )
        .unwrap();
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["--effort", "max"]));
        assert!(cmd
            .as_std()
            .get_envs()
            .all(|(key, _)| key != "MAX_THINKING_TOKENS"));

        drop(prompt_file);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn old_claude_prompt_file_flag_gets_an_upgrade_hint() {
        let clarified = clarify_claude_cli_error(
            "claude",
            "error: unknown option '--append-system-prompt-file'".into(),
        );
        assert!(clarified.contains("版本过旧"));
        assert!(clarified.contains("更新 Claude Code"));

        let unrelated = clarify_claude_cli_error("claude", "network timeout".into());
        assert_eq!(unrelated, "network timeout");
        let codex = clarify_claude_cli_error(
            "codex",
            "unknown option '--append-system-prompt-file'".into(),
        );
        assert_eq!(codex, "unknown option '--append-system-prompt-file'");
    }

    #[test]
    fn claude_context_limit_error_explains_that_pilot_history_is_preserved() {
        for raw in [
            "Prompt is too long",
            "maximum context length exceeded",
            "input is too long for the context window",
            "上下文已超过上限",
        ] {
            let clarified = clarify_claude_cli_error("claude", raw.into());
            assert!(clarified.contains("聊天记录仍完整保留"));
            assert!(clarified.contains("重建继续"));
            assert!(clarified.contains(raw));
        }
    }

    #[test]
    fn claude_compact_boundary_serializes_for_the_frontend() {
        let raw = json!({
            "type": "system",
            "subtype": "compact_boundary",
            "compact_metadata": {"pre_tokens": 912345, "trigger": "auto"}
        });
        assert_eq!(claude_compact_pre_tokens(&raw), Some(912_345));
        assert_eq!(
            claude_compact_pre_tokens(&json!({"type":"system","subtype":"init"})),
            None
        );

        let event = TurnEvent::ContextCompacted {
            pre_tokens: 912_345,
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({"type":"context_compacted","pre_tokens":912345})
        );
    }

    #[test]
    fn claude_text_delta_accumulates() {
        let c = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: vec![],
            session_ref: "s".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: vec![],
        }));
        let channel = ch();
        parse_claude(
            &json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"你好"}}}),
            &channel,
            &c,
        );
        parse_claude(
            &json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"世界"}}}),
            &channel,
            &c,
        );
        assert_eq!(c.lock().unwrap().text, "你好世界");
    }

    #[test]
    fn claude_tool_use_captured() {
        let c = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: vec![],
            session_ref: "s".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: vec![],
        }));
        parse_claude(
            &json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"npm run build"}}]}}),
            &ch(),
            &c,
        );
        let g = c.lock().unwrap();
        assert_eq!(g.tools.len(), 1);
        assert!(g.tools[0].detail.contains("npm run build"));
    }

    #[test]
    fn claude_and_codex_activity_helpers_preserve_real_terminal_states() {
        assert_eq!(
            claude_tool_result_activity(&json!({
                "type": "tool_result",
                "tool_use_id": "tool-1",
                "content": "done"
            })),
            Some(("tool-1".into(), "completed".into(), String::new()))
        );
        assert_eq!(
            claude_tool_result_activity(&json!({
                "type": "tool_result",
                "tool_use_id": "tool-2",
                "is_error": true,
                "content": [{"type":"text","text":"网页截图加载超时"}]
            })),
            Some(("tool-2".into(), "failed".into(), "网页截图加载超时".into()))
        );
        assert!(claude_tool_result_activity(&json!({
            "type": "assistant",
            "content": "not a tool result"
        }))
        .is_none());

        let started = codex_activity(
            &json!({
                "type": "command_execution",
                "id": "item-1",
                "command": "npm run check"
            }),
            "running",
        )
        .expect("Codex command should become a live activity");
        assert_eq!(
            started,
            (
                "item-1".into(),
                "exec".into(),
                "npm run check".into(),
                String::new(),
                "running".into()
            )
        );
        let completed = codex_activity(
            &json!({
                "type": "command_execution",
                "id": "item-1",
                "command": "npm run check"
            }),
            "completed",
        )
        .expect("same Codex item should reach a terminal state");
        assert_eq!(completed.0, started.0);
        assert_eq!(completed.4, "completed");
        let failed = codex_activity(
            &json!({
                "type": "command_execution",
                "id": "item-2",
                "command": "capture-page",
                "status": "failed",
                "error": {"message": "页面加载超时"}
            }),
            "failed",
        )
        .expect("failed Codex command should retain its reason");
        assert_eq!(failed.3, "页面加载超时");
        assert_eq!(failed.4, "failed");
        assert!(
            codex_activity(
                &json!({"type": "agent_message", "id": "message-1", "text": "done"}),
                "completed"
            )
            .is_none(),
            "assistant prose must not masquerade as execution progress"
        );
    }

    #[test]
    fn codex_native_events_expose_semantic_progress_without_private_payloads() {
        assert!(
            native_activity(&json!({
                "type": "event_msg",
                "payload": {
                    "type": "agent_message",
                    "message": "我会读取最新 ETag，然后发布这 12 条资源。"
                }
            }))
            .is_none(),
            "assistant prose belongs in the message body, not in the activity card"
        );

        let image = native_activity(&json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "image_gen__imagegen",
                "call_id": "call-image-1",
                "arguments": "{\"prompt\":\"private prompt\",\"token\":\"secret\"}"
            }
        }))
        .expect("imagegen should become semantic activity");
        assert_eq!(image.0, "call-image-1");
        assert_eq!(image.1, "image_generation");
        assert_eq!(image.3, "正在生成图片");
        assert!(!format!("{image:?}").contains("private prompt"));
        assert!(!format!("{image:?}").contains("secret"));

        let reasoning = native_activity(&json!({
            "type": "response_item",
            "payload": {
                "type": "reasoning",
                "encrypted_content": "encrypted-private-chain"
            }
        }))
        .expect("reasoning should expose a safe pulse");
        assert_eq!(reasoning.1, "reasoning");
        assert!(!format!("{reasoning:?}").contains("encrypted-private-chain"));

        let completed = native_activity(&json!({
            "type": "event_msg",
            "payload": {
                "type": "image_generation_end",
                "call_id": "call-image-1",
                "status": "completed",
                "saved_path": "/Users/private/generated_images/secret.webp",
                "result": "data:image/webp;base64,very-secret"
            }
        }))
        .expect("image end should complete the same activity");
        assert_eq!(completed.0, "call-image-1");
        assert_eq!(completed.5, "completed");
        assert!(!format!("{completed:?}").contains("/Users/private"));
        assert!(!format!("{completed:?}").contains("base64"));

    }

    #[test]
    fn codex_thread_and_message() {
        let c = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: vec![],
            session_ref: String::new(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: vec![],
        }));
        parse_codex(
            &json!({"type":"thread.started","thread_id":"tid-123"}),
            &ch(),
            &c,
        );
        parse_codex(
            &json!({"type":"item.completed","item":{"type":"agent_message","text":"明白"}}),
            &ch(),
            &c,
        );
        let g = c.lock().unwrap();
        assert_eq!(g.session_ref, "tid-123");
        assert_eq!(g.text, "明白");
    }

    #[test]
    fn result_error_flag() {
        let c = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: vec![],
            session_ref: "s".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: vec![],
        }));
        parse_claude(
            &json!({"type":"result","is_error":true,"result":"boom"}),
            &ch(),
            &c,
        );
        assert!(c.lock().unwrap().is_error);
    }

    /// 限额识别：claude 带重置时间戳、codex/429 无时间戳给 0、普通错误 None。
    #[test]
    fn detect_usage_limit_variants() {
        assert_eq!(
            detect_usage_limit("Claude AI usage limit reached|1736600400"),
            Some(1736600400)
        );
        assert_eq!(
            detect_usage_limit("blah limit reached|1736600400000 tail"),
            Some(1736600400)
        ); // 毫秒级归一
        assert_eq!(detect_usage_limit("HTTP 429 Too Many Requests"), Some(0));
        assert_eq!(detect_usage_limit("You've hit your usage limit."), Some(0));
        assert_eq!(
            detect_usage_limit("insufficient_quota: upgrade your plan"),
            Some(0)
        );
        assert_eq!(detect_usage_limit("没找到这个文件"), None);
        assert_eq!(detect_usage_limit("connection reset by peer"), None); // reset≠限额
    }

    /// 生图产物收集：工具结果里的 generated_images 路径进 images（去重）；普通路径不收。
    #[test]
    fn codex_generated_images_collected() {
        let c = Arc::new(Mutex::new(Collect {
            text: String::new(),
            tools: vec![],
            session_ref: "s".into(),
            is_error: false,
            usage: None,
            raw_tail: String::new(),
            images: vec![],
        }));
        let p = "/Users/x/.codex/generated_images/019f/exec-abc.png";
        parse_codex(
            &json!({"type":"item.completed","item":{"type":"tool_call","output":format!("saved to {p} done")}}),
            &ch(),
            &c,
        );
        parse_codex(
            &json!({"type":"item.completed","item":{"type":"tool_call","output":{"path": p}}}),
            &ch(),
            &c,
        ); // 重复不重收
        parse_codex(
            &json!({"type":"item.completed","item":{"type":"command_execution","command":"cp a.png b.png"}}),
            &ch(),
            &c,
        ); // 普通路径不收
        let g = c.lock().unwrap();
        assert_eq!(g.images, vec![p.to_string()]);
    }

    /// 「生成的图片」清单：正文没提到的才补；提到过/为空不动原文。
    #[test]
    fn append_generated_images_dedups_against_text() {
        let imgs = vec![
            "/a/generated_images/x.png".to_string(),
            "/a/generated_images/y.png".to_string(),
        ];
        let out = append_generated_images("已生成 /a/generated_images/x.png", &imgs);
        assert!(
            out.contains("生成的图片：\n- /a/generated_images/y.png"),
            "{out}"
        );
        assert_eq!(out.matches("x.png").count(), 1, "正文已有的不重复列");
        assert_eq!(append_generated_images("正文", &[]), "正文");
        let solo = append_generated_images("", &imgs[..1].to_vec());
        assert!(solo.starts_with("生成的图片："), "{solo}");
    }

}
