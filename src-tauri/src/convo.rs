//! 对话式会话：一次会话 = 用户与某个模型围绕一个目标（Cloudflare 项目 / SSH 机器 /
//! 本地工作区）的多轮对话。
//! 持久化到 conversations.json（原子写 + 互斥串行化）。
//! 底层多轮机制：claude 用 --session-id/--resume，codex 用 exec/exec resume。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub label: String,
    pub detail: String,
    /// 这一步发生时正文已经写到哪（UTF-16 长度，前端按它把步骤插在正文中间，像 Claude 客户端那样）。
    #[serde(default)]
    pub at: u32,
}

/// 文本的 UTF-16 长度（和前端 JS 字符串下标一致）。
pub fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// begin_turn 的结果。
pub enum TurnStart {
    Started,
    /// session 尚未建立（当年首轮就崩 / 换厂商时被清空）：消息已入列、状态已置 running，
    /// 调用方须按「新会话首轮」跑（同重建口径：新 session + 系统提示 + 历史摘要）——
    /// 老对话不再因此死路一条。
    StartedFresh,
    Busy,
    NotFound,
    /// 用户消息和 running 状态没能写进磁盘（磁盘满 / 没权限）：这一轮不能跑，否则回复会对不上提问。
    Failed(String),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user" | "assistant"
    pub text: String,
    #[serde(default)]
    pub tools: Vec<ToolCall>,
    pub ts: u64,
    /// 内部框架消息（kickoff 等），不在界面展示。
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub error: bool,
    /// 本轮因订阅额度/限流失败：恢复时间戳秒（0=拿不到时间）。None=非限额错误。
    /// 前端据此渲染「额度已用完」卡片与到点自动续跑。
    #[serde(default)]
    pub limit_reset: Option<i64>,
    /// 运行中的这一轮写到一半的回复（每隔几秒存一次，见 turn_checkpoint）。应用中途退出时它就是
    /// 「中断前的进度」；这一轮正常结束会被正式回复替换掉。运行中不对外返回（界面有实时的那份）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// 这一轮没正常结束（被停止、出错）时的原因，如「已停止」；正常结束为空。
    /// error 只在「失败且没产出内容」时才标红，有半截回答的失败靠它认出来（无人值守任务不能把半截当做完）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unfinished: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    /// 会话在侧栏中的固定归属。workspace 会话只借这个字段记住“在哪里创建”，
    /// 实际运行始终使用隔离的 workspace 连接，不读取该连接的站点或凭据。
    pub conn_id: String,
    pub conn_name: String,
    pub site_slug: String,
    pub site_name: String,
    /// sitebuild（Cloudflare 项目）| remote（SSH）| workspace（自由对话）
    pub task_type: String,
    pub brain: String,
    pub model: String,
    /// 权限档位：plan | ask | auto | full。空串＝旧会话＝full（保持 0.1.10）。
    #[serde(default)]
    pub perm_mode: String,
    /// 思考等级（推理强度）：'' 默认 | low | medium | high。每轮下发，运行中可改。
    #[serde(default)]
    pub effort: String,
    /// fast 模式：只有 Opus 5 / Opus 4.8 支持。旧会话反序列化缺这个键 → default false。
    #[serde(default)]
    pub fast: bool,
    /// 自由对话显式选择的本地工作目录。空串表示使用 Pilot 为该会话创建的隔离目录。
    /// 只有 task_type=workspace 会读取；其它任务必须忽略，避免把本地路径误当站点上下文。
    #[serde(default)]
    pub workspace_dir: String,
    /// claude 的 session uuid 或 codex 的 thread_id；首轮后回填。
    pub session_ref: String,
    pub title: String,
    pub messages: Vec<Message>,
    /// idle | running
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 最近一轮的上下文 token（≈当前会话大小），用于「上下文 X/上限」。0＝没数据。
    #[serde(default)]
    pub ctx_tokens: u64,
    /// 本会话累计处理的 token（每轮 input+cache+output 相加）。
    #[serde(default)]
    pub total_tokens: u64,
    /// 按计费折算的累计 token：缓存读取按 1/10、缓存写入按 1.25 倍计。运营预算按它算；0＝旧会话没记，退回 total_tokens。
    #[serde(default)]
    pub billed_tokens: u64,
    /// 会话由谁发起：空＝用户；ops＝站点运营任务（无人值守开出来的会话，侧栏单独分组）。
    #[serde(default)]
    pub origin: String,
}

/// 活着的会话列表只在首次用到时从 conversations.json 读一次，之后常驻内存（写操作直写磁盘）。
/// 以前每次 list/get 都整文件读+解析、每次保存都整文件序列化——权限轮询、读图片都要过一遍，
/// 会话多了以后白白烤 CPU。
#[derive(Clone)]
pub struct ConvStore {
    file: PathBuf,
    /// 运行中那一轮的草稿各自一个小文件：conversations.partial/<id>.json。
    /// 每 3 秒一次的 checkpoint 只写这个小文件，不再整个重写 conversations.json。
    partial_dir: PathBuf,
    /// 超过 300 条上限挤出去的旧会话追加到这里，只存不读（界面不展示，但一条都不丢）。
    archive: PathBuf,
    /// None = 还没从磁盘装载。
    cache: Arc<Mutex<Option<Vec<Conversation>>>>,
}

/// 活着的会话上限；超出的最旧会话搬去归档文件。
const LIVE_CAP: usize = 300;

impl ConvStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            file: data_dir.join("conversations.json"),
            partial_dir: data_dir.join("conversations.partial"),
            archive: data_dir.join("conversations-archive.json"),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// 装载（只在第一次）并返回缓存。调用方必须已经持有锁。
    fn loaded<'a>(&self, cache: &'a mut Option<Vec<Conversation>>) -> &'a mut Vec<Conversation> {
        cache.get_or_insert_with(|| {
            let mut v: Vec<Conversation> = crate::store_file::read_or_quarantine(&self.file);
            v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            v
        })
    }

    /// 测试用：磁盘上的那份（绕开缓存）。
    #[cfg(test)]
    fn read(&self) -> Vec<Conversation> {
        let mut v: Vec<Conversation> = crate::store_file::read_or_quarantine(&self.file);
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    }

    fn partial_file(&self, id: &str) -> PathBuf {
        // id 是我们自己生成的 uuid；再过一遍白名单，别让奇怪的 id 变成路径
        let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        self.partial_dir.join(format!("{safe}.json"))
    }

    fn read_partial(&self, id: &str) -> Option<Message> {
        let raw = fs::read(self.partial_file(id)).ok()?;
        serde_json::from_slice::<Message>(&raw).ok().map(|m| Message { partial: true, ..m })
    }

    fn clear_partial(&self, id: &str) {
        let _ = fs::remove_file(self.partial_file(id));
    }

    /// 把会话连同磁盘上的草稿一起拼出来（草稿只在运行中才算）。
    fn with_partial(&self, mut c: Conversation) -> Conversation {
        if c.status == "running" {
            if let Some(p) = self.read_partial(&c.id) {
                if c.messages.last().map(|m| m.partial).unwrap_or(false) {
                    c.messages.pop();
                }
                c.messages.push(p);
            }
        }
        c
    }

    /// 最近更新在前。
    pub fn list(&self) -> Vec<Conversation> {
        let mut g = self.cache.lock().unwrap();
        self.loaded(&mut g).iter().cloned().map(without_partial).collect()
    }

    pub fn get(&self, id: &str) -> Option<Conversation> {
        let mut g = self.cache.lock().unwrap();
        self.loaded(&mut g).iter().find(|c| c.id == id).cloned().map(without_partial)
    }

    /// 连写到一半的那条一起取（界面丢了这一轮的实时通道时，用它接着显示进度）。
    pub fn get_live(&self, id: &str) -> Option<Conversation> {
        let mut g = self.cache.lock().unwrap();
        let c = self.loaded(&mut g).iter().find(|c| c.id == id).cloned()?;
        Some(self.with_partial(c))
    }

    /// 存下运行中这一轮写到一半的回复（替换上一次存的那份）。会话已经不在跑了就不存。
    /// 只写 conversations.partial/<id>.json 这个小文件，不碰主文件。
    pub fn checkpoint(&self, id: &str, partial: Message) -> Result<(), String> {
        let mut g = self.cache.lock().unwrap();
        let Some(c) = self.loaded(&mut g).iter().find(|c| c.id == id) else { return Ok(()) };
        if c.status != "running" {
            return Ok(());
        }
        let msg = Message { partial: true, ..partial };
        let raw = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.partial_dir).map_err(|e| format!("create partial dir: {e}"))?;
        let file = self.partial_file(id);
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, &raw).map_err(|e| format!("write partial tmp: {e}"))?;
        fs::rename(&tmp, &file).map_err(|e| format!("replace partial: {e}"))
    }

    /// 把缓存落盘（原子：同目录临时文件 + rename）。调用方须已持锁且已把缓存改好。
    /// 写失败就把缓存作废：下次访问重新从磁盘装载，内存里绝不留一份磁盘上没有的改动。
    fn commit(&self, cache: &mut Option<Vec<Conversation>>) -> Result<(), String> {
        let list = self.loaded(cache);
        let res = serde_json::to_vec_pretty(list).map_err(|e| e.to_string()).and_then(|raw| {
            let tmp = self.file.with_extension("json.tmp");
            fs::write(&tmp, &raw).map_err(|e| format!("write conversations tmp: {e}"))?;
            fs::rename(&tmp, &self.file).map_err(|e| format!("replace conversations.json: {e}"))
        });
        if res.is_err() {
            *cache = None;
        }
        res
    }

    /// 改完某条会话后的收尾：不在跑了就把草稿文件清掉（正式回复已经替代了它）。
    fn settle(&self, c: &Conversation) {
        if c.status != "running" {
            self.clear_partial(&c.id);
        }
    }

    /// 挤出上限的旧会话追加进归档文件（只追加，永不读回界面）。
    fn archive_overflow(&self, overflow: &[Conversation]) -> Result<(), String> {
        if overflow.is_empty() {
            return Ok(());
        }
        let mut old: Vec<Conversation> = crate::store_file::read_or_quarantine(&self.archive);
        old.extend(overflow.iter().cloned());
        let raw = serde_json::to_vec(&old).map_err(|e| e.to_string())?;
        let tmp = self.archive.with_extension("json.tmp");
        fs::write(&tmp, &raw).map_err(|e| format!("write archive tmp: {e}"))?;
        fs::rename(&tmp, &self.archive).map_err(|e| format!("replace archive: {e}"))
    }

    pub fn upsert(&self, conv: Conversation) -> Result<(), String> {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let id = conv.id.clone();
        if let Some(slot) = list.iter_mut().find(|c| c.id == conv.id) {
            *slot = conv;
        } else {
            list.push(conv);
        }
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        // 超出上限的最旧会话不再悄悄丢掉：先归档，归档写不进去就不删（宁可超一点）。
        let overflow: Vec<Conversation> = if list.len() > LIVE_CAP { list.split_off(LIVE_CAP) } else { Vec::new() };
        if let Err(e) = self.archive_overflow(&overflow) {
            list.extend(overflow);
            eprintln!("[pilot] 会话归档失败：{e}");
        } else {
            for c in &overflow {
                self.clear_partial(&c.id);
            }
        }
        let updated = list.iter().find(|c| c.id == id).cloned();
        self.commit(&mut g)?;
        if let Some(c) = updated {
            self.settle(&c);
        }
        Ok(())
    }

    /// 原子开始续对话：锁内检查—追加用户消息—置 running，杜绝同一会话并发起两轮（TOCTOU）。
    pub fn begin_turn(&self, id: &str, now: u64, user: Message) -> TurnStart {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let Some(slot) = list.iter_mut().find(|c| c.id == id) else {
            return TurnStart::NotFound;
        };
        if slot.status == "running" {
            return TurnStart::Busy;
        }
        let fresh = slot.session_ref.trim().is_empty();
        slot.messages.push(user);
        slot.status = "running".into();
        slot.updated_at = now;
        if let Err(e) = self.commit(&mut g) {
            return TurnStart::Failed(e);
        }
        if fresh {
            TurnStart::StartedFresh
        } else {
            TurnStart::Started
        }
    }

    /// 原子开始"重试上一轮"：锁内检查 running/session、去掉末尾那条失败/部分的助手消息、
    /// 取最后一条用户消息文本作为要重跑的提示词、置 running。不新增用户消息（重试不是新问）。
    /// 返回要重试的用户消息文本。
    pub fn begin_retry(&self, id: &str, now: u64) -> Result<String, String> {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let Some(slot) = list.iter_mut().find(|c| c.id == id) else {
            return Err("会话不存在".into());
        };
        if slot.status == "running" {
            return Err("上一轮还在进行中，请稍候".into());
        }
        if slot.session_ref.trim().is_empty() {
            return Err("这个会话已失效，请新建对话".into());
        }
        // 去掉末尾的助手消息（失败或只写了一半的那条），重试会用一条新的替代它。
        if slot
            .messages
            .last()
            .map(|m| m.role == "assistant")
            .unwrap_or(false)
        {
            slot.messages.pop();
        }
        let Some(text) = slot
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.text.clone())
        else {
            return Err("没有可重试的消息".into());
        };
        slot.status = "running".into();
        slot.updated_at = now;
        self.clear_partial(id);
        self.commit(&mut g)?;
        Ok(text)
    }

    /// 原子开始"重建会话"：同 begin_retry，但**不要求已有 session**——重建本来就是
    /// 为了抛弃损坏/缺失的底层会话另起新的（含首轮就崩、session_ref 为空的情况）。
    pub fn begin_rebuild(&self, id: &str, now: u64) -> Result<String, String> {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let Some(slot) = list.iter_mut().find(|c| c.id == id) else {
            return Err("会话不存在".into());
        };
        if slot.status == "running" {
            return Err("上一轮还在进行中，请稍候".into());
        }
        if slot
            .messages
            .last()
            .map(|m| m.role == "assistant")
            .unwrap_or(false)
        {
            slot.messages.pop();
        }
        let Some(text) = slot
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.text.clone())
        else {
            return Err("没有可重跑的消息".into());
        };
        slot.status = "running".into();
        slot.updated_at = now;
        self.clear_partial(id);
        self.commit(&mut g)?;
        Ok(text)
    }

    /// 在锁内对某会话做原子改动（读—改—写不被并发打断）。
    pub fn mutate<F>(&self, id: &str, now: u64, f: F) -> Result<Option<Conversation>, String>
    where
        F: FnOnce(&mut Conversation),
    {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let Some(slot) = list.iter_mut().find(|c| c.id == id) else {
            return Ok(None);
        };
        f(slot);
        slot.updated_at = now;
        let updated = slot.clone();
        self.commit(&mut g)?;
        self.settle(&updated);
        Ok(Some(updated))
    }

    /// Validate and change related fields under one lock. Failed validation must
    /// not persist any partial changes (for example model without its effort).
    pub fn mutate_checked<F>(
        &self,
        id: &str,
        now: u64,
        f: F,
    ) -> Result<Option<Conversation>, String>
    where
        F: FnOnce(&mut Conversation) -> Result<(), String>,
    {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let Some(slot) = list.iter_mut().find(|c| c.id == id) else {
            return Ok(None);
        };
        // 校验失败不能留下半截改动：先在副本上改，通过了再写回缓存。
        let mut draft = slot.clone();
        f(&mut draft)?;
        draft.updated_at = now;
        *slot = draft.clone();
        self.commit(&mut g)?;
        self.settle(&draft);
        Ok(Some(draft))
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        list.retain(|c| c.id != id);
        self.clear_partial(id);
        self.commit(&mut g)
    }

    /// 删除某连接下的所有对话（删连接时级联清理，避免留下够不到密钥的孤儿会话）。
    pub fn remove_by_conn_id(&self, conn_id: &str) -> Result<(), String> {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let gone: Vec<String> = list.iter().filter(|c| c.conn_id == conn_id).map(|c| c.id.clone()).collect();
        if gone.is_empty() {
            return Ok(());
        }
        list.retain(|c| c.conn_id != conn_id);
        for id in &gone {
            self.clear_partial(id);
        }
        self.commit(&mut g)
    }

    /// 启动时把残留 running 的会话置回 idle（进程随退出被杀，不可能还在跑）。
    pub fn mark_idle(&self, now: u64) {
        let mut g = self.cache.lock().unwrap();
        let list = self.loaded(&mut g);
        let mut changed = false;
        let mut settled: Vec<String> = Vec::new();
        for c in list.iter_mut() {
            if c.status == "running" {
                // 草稿在旁边的小文件里（旧版本是直接写在会话里的，两种都认）：先拼进来再转成「被中断」。
                if let Some(p) = self.read_partial(&c.id) {
                    if c.messages.last().map(|m| m.partial).unwrap_or(false) {
                        c.messages.pop();
                    }
                    c.messages.push(p);
                }
                c.status = "idle".into();
                c.updated_at = now;
                changed = true;
                interrupted(c, now);
                settled.push(c.id.clone());
            }
        }
        if changed {
            let _ = self.commit(&mut g);
            for id in &settled {
                self.clear_partial(id);
            }
        }
    }
}

/// 运行中的会话不对外露出写到一半的那条（界面有实时的一份，不然会显示两遍）。
fn without_partial(mut c: Conversation) -> Conversation {
    if c.status == "running" {
        c.messages.retain(|m| !m.partial);
    }
    c
}

/// 上一轮没跑完应用就退出了：把存下的进度转成一条「被中断」的回复；一点进度都没存下就补一条说明，
/// 别让对话停在用户那条消息上、像什么都没发生过。
fn interrupted(c: &mut Conversation, now: u64) {
    const NOTE: &str = "这一轮没有跑完：应用被关闭或重启了。";
    match c.messages.last_mut() {
        Some(m) if m.partial => {
            m.partial = false;
            m.error = true;
            m.text = if m.text.trim().is_empty() { format!("{NOTE}以下是中断前做过的步骤。") } else { format!("{NOTE}以下是中断前的进度：\n\n{}", m.text) };
        }
        Some(m) if m.role == "user" => c.messages.push(Message {
            role: "assistant".into(),
            text: format!("{NOTE}可以重试，或接着说要做什么。"),
            tools: vec![],
            ts: now,
            hidden: false,
            error: true,
            limit_reset: None,
            partial: false,
            unfinished: String::new(),
        }),
        _ => {}
    }
}

/// 把历史消息压成文本记录，供「重建会话」注入新会话衔接上下文。
/// 排除：隐藏消息、错误消息、以及**结尾最后一条用户消息**（那是将要重跑的请求本身）。
/// budget 为字符预算，超出时保留最近的并在开头标注省略；单条消息截前 8000 字符。
pub fn recap(messages: &[Message], budget: usize) -> String {
    let last_user = messages.iter().rposition(|m| m.role == "user");
    let mut lines: Vec<String> = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if Some(i) == last_user || m.hidden || m.error || m.text.trim().is_empty() {
            continue;
        }
        let who = if m.role == "user" { "用户" } else { "助手" };
        let body: String = m.text.trim().chars().take(8000).collect();
        lines.push(format!("{who}：{body}"));
    }
    let mut used = 0usize;
    let mut kept: Vec<&String> = Vec::new();
    for l in lines.iter().rev() {
        let n = l.chars().count() + 2;
        if used + n > budget && !kept.is_empty() {
            break;
        }
        used += n;
        kept.push(l);
        if used >= budget {
            break;
        }
    }
    let truncated = kept.len() < lines.len();
    let mut out = String::new();
    if truncated {
        out.push_str("（更早的历史已省略）\n");
    }
    for l in kept.iter().rev() {
        out.push_str(l);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.into(),
            text: text.into(),
            tools: vec![],
            ts: 0,
            hidden: false,
            error: role == "assistant" && text.is_empty(),
            limit_reset: None,
            partial: false,
            unfinished: String::new(),
        }
    }
    fn conv(id: &str, session: &str, msgs: Vec<Message>) -> Conversation {
        Conversation {
            id: id.into(),
            conn_id: "c".into(),
            conn_name: "".into(),
            site_slug: "s".into(),
            site_name: "".into(),
            task_type: "free".into(),
            brain: "claude".into(),
            model: "sonnet".into(),
            perm_mode: "full".into(),
            effort: String::new(),
            fast: false,
            workspace_dir: String::new(),
            session_ref: session.into(),
            title: "t".into(),
            messages: msgs,
            status: "idle".into(),
            created_at: 0,
            updated_at: 0,
            ctx_tokens: 0,
            total_tokens: 0,
            billed_tokens: 0,
            origin: String::new(),
        }
    }

    #[test]
    fn running_turn_checkpoints_are_hidden_while_running_and_kept_as_interrupted_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        let mut c = conv("r", "", vec![msg("user", "做第一版")]);
        c.status = "running".into();
        store.upsert(c).unwrap();
        store.checkpoint("r", Message { partial: true, ..msg("assistant", "写好了首页") }).unwrap();
        store.checkpoint("r", Message { partial: true, ..msg("assistant", "写好了首页和指南") }).unwrap();
        // 运行中：对外看不到草稿；草稿在旁边的小文件里只存一份（后一次替换前一次），主文件不动
        assert_eq!(store.get("r").unwrap().messages.len(), 1);
        assert_eq!(store.read()[0].messages.len(), 1, "checkpoint 不该重写 conversations.json");
        assert!(store.partial_file("r").exists());
        let live = store.get_live("r").unwrap();
        assert_eq!(live.messages.len(), 2);
        assert!(live.messages[1].partial && live.messages[1].text == "写好了首页和指南");
        // 应用重启：草稿变成「被中断」的回复
        store.mark_idle(9);
        let got = store.get("r").unwrap();
        let last = got.messages.last().unwrap();
        assert!(got.status == "idle" && last.error && !last.partial);
        assert!(last.text.contains("没有跑完") && last.text.contains("写好了首页和指南"));
        assert!(!store.partial_file("r").exists(), "转成正式回复后草稿文件要清掉");
        // 不在跑的会话不再接受草稿
        store.checkpoint("r", Message { partial: true, ..msg("assistant", "迟到的") }).unwrap();
        assert_eq!(store.get("r").unwrap().messages.len(), 2);
        assert!(!store.partial_file("r").exists());
    }

    /// 旧版本把草稿直接写在会话里：重启后同样要转成「被中断」的回复。
    #[test]
    fn legacy_inline_partial_is_still_recovered_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = conv("old", "", vec![msg("user", "做"), Message { partial: true, ..msg("assistant", "做了一半") }]);
        c.status = "running".into();
        std::fs::write(dir.path().join("conversations.json"), serde_json::to_vec(&vec![c]).unwrap()).unwrap();
        let store = ConvStore::new(dir.path());
        store.mark_idle(5);
        let got = store.get("old").unwrap();
        assert_eq!(got.status, "idle");
        assert!(got.messages.last().unwrap().text.contains("做了一半"));
        assert!(!got.messages.last().unwrap().partial);
    }

    /// 缓存：改动对下一次读立刻可见；也确实写进了磁盘（新建一个 ConvStore 能读回来）。
    #[test]
    fn cache_is_write_through_and_survives_a_fresh_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        store.upsert(conv("a", "s", vec![msg("user", "一")])).unwrap();
        assert_eq!(store.list().len(), 1);
        store.mutate("a", 7, |c| c.title = "改过".into()).unwrap();
        assert_eq!(store.get("a").unwrap().title, "改过");
        assert_eq!(store.get("a").unwrap().updated_at, 7);
        // 另一个 store（模拟重启）从磁盘读到同样的内容
        let again = ConvStore::new(dir.path());
        let c = again.get("a").unwrap();
        assert_eq!(c.title, "改过");
        assert_eq!(c.messages.len(), 1);
        again.remove("a").unwrap();
        assert!(again.get("a").is_none());
        assert!(ConvStore::new(dir.path()).get("a").is_none());
        // 校验失败的 mutate_checked 不污染缓存
        store.upsert(conv("b", "s", vec![])).unwrap();
        let _ = store.mutate_checked("b", 9, |c| { c.title = "坏".into(); Err("no".into()) });
        assert_eq!(store.get("b").unwrap().title, "t");
        assert_eq!(ConvStore::new(dir.path()).get("b").unwrap().title, "t");
    }

    /// 超过上限的旧会话搬去归档文件，一条都不丢；活着的列表仍然只留上限条。
    #[test]
    fn overflow_goes_to_archive_instead_of_vanishing() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        for i in 0..(LIVE_CAP + 3) {
            let mut c = conv(&format!("c{i}"), "s", vec![]);
            c.updated_at = i as u64;
            store.upsert(c).unwrap();
        }
        let live = store.list();
        assert_eq!(live.len(), LIVE_CAP);
        assert!(live.iter().all(|c| c.id != "c0" && c.id != "c1" && c.id != "c2"), "最旧的三条被挤出");
        let archived: Vec<Conversation> = crate::store_file::read_or_quarantine(&dir.path().join("conversations-archive.json"));
        let ids: Vec<&str> = archived.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["c0", "c1", "c2"]);
        // 再挤一条：归档是追加，不是覆盖
        let mut c = conv("new", "s", vec![]);
        c.updated_at = 999;
        store.upsert(c).unwrap();
        let archived: Vec<Conversation> = crate::store_file::read_or_quarantine(&dir.path().join("conversations-archive.json"));
        assert_eq!(archived.len(), 4);
        assert_eq!(archived[3].id, "c3");
        assert!(store.list().iter().all(|c| c.id != "c3"), "归档的不再出现在界面列表");
    }

    #[test]
    fn a_turn_killed_before_any_checkpoint_still_gets_an_interrupted_note() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        let mut c = conv("k", "", vec![msg("user", "做第一版")]);
        c.status = "running".into();
        store.upsert(c).unwrap();
        store.mark_idle(9);
        let got = store.get("k").unwrap();
        assert_eq!(got.messages.len(), 2);
        assert!(got.messages[1].error && got.messages[1].text.contains("没有跑完"));
    }

    #[test]
    fn checked_mutation_saves_model_and_effort_together_and_rolls_back_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        let mut c = conv("checked", "session", vec![]);
        c.brain = "api:profile".into();
        c.model = "claude-sonnet-5-5".into();
        c.effort = "api:xhigh".into();
        store.upsert(c).unwrap();
        let rejected = store.mutate_checked("checked", 5, |c| {
            c.model = "invalid-model".into();
            c.effort.clear();
            Err("invalid selection".into())
        });
        assert!(rejected.is_err());
        let unchanged = store.get("checked").unwrap();
        assert_eq!(unchanged.model, "claude-sonnet-5-5");
        assert_eq!(unchanged.effort, "api:xhigh");
        let updated = store
            .mutate_checked("checked", 10, |c| {
                c.model = "claude-opus-4-6".into();
                c.effort.clear();
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert_eq!(updated.model, "claude-opus-4-6");
        assert_eq!(updated.effort, "");
        assert_eq!(updated.updated_at, 10);
        assert_eq!(store.get("checked").unwrap().effort, "");
    }

    #[test]
    fn begin_retry_pops_assistant_reuses_last_user_and_marks_running() {
        let base = std::env::temp_dir().join(format!("convo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let store = ConvStore::new(&base);
        store
            .upsert(conv(
                "a",
                "sess-1",
                vec![
                    msg("user", "第一题"),
                    msg("assistant", "回答一"),
                    msg("user", "第二题"),
                    msg("assistant", ""),
                ],
            ))
            .unwrap();

        let text = store.begin_retry("a", 10).unwrap();
        assert_eq!(text, "第二题"); // 取最后一条用户消息
        let c = store.get("a").unwrap();
        assert_eq!(c.status, "running"); // 置 running
        assert_eq!(c.messages.len(), 3); // 末尾失败助手被弹掉
        assert_eq!(c.messages.last().unwrap().role, "user");

        // 正在跑时拒绝
        assert!(store.begin_retry("a", 11).is_err());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn begin_turn_without_session_starts_fresh_instead_of_dead_end() {
        // 首轮 401 就崩 / 换厂商清空 session 的老对话：发消息不再报「已失效」死路，
        // 而是消息照常入列、置 running，返回 StartedFresh 让调用方按新会话首轮跑。
        let base = std::env::temp_dir().join(format!("convo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let store = ConvStore::new(&base);
        store
            .upsert(conv(
                "a",
                "",
                vec![msg("user", "hi"), msg("assistant", "早年回复")],
            ))
            .unwrap();

        let started = store.begin_turn("a", 10, msg("user", "新消息"));
        assert!(matches!(started, TurnStart::StartedFresh));
        let c = store.get("a").unwrap();
        assert_eq!(c.status, "running");
        assert_eq!(c.messages.last().unwrap().text, "新消息");

        // 有 session 的照旧走 Started
        store.upsert(conv("b", "sess-1", vec![])).unwrap();
        assert!(matches!(
            store.begin_turn("b", 11, msg("user", "q")),
            TurnStart::Started
        ));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn recap_excludes_last_user_hidden_and_error() {
        let msgs = vec![
            msg("user", "第一问"),
            msg("assistant", "第一答"),
            Message {
                role: "assistant".into(),
                text: "内部".into(),
                tools: vec![],
                ts: 0,
                hidden: true,
                error: false,
                limit_reset: None,
                partial: false,
                unfinished: String::new(),
            },
            Message {
                role: "assistant".into(),
                text: "报错了".into(),
                tools: vec![],
                ts: 0,
                hidden: false,
                error: true,
                limit_reset: None,
                partial: false,
                unfinished: String::new(),
            },
            msg("user", "最新请求"),
        ];
        let r = recap(&msgs, 8000);
        assert!(r.contains("用户：第一问"));
        assert!(r.contains("助手：第一答"));
        assert!(!r.contains("最新请求")); // 将要重跑的请求本身不进 recap
        assert!(!r.contains("内部")); // hidden 排除
        assert!(!r.contains("报错了")); // error 排除
                                        // 预算收紧 → 保留最近 + 省略标注
        let tight = recap(&msgs, 12);
        assert!(tight.contains("（更早的历史已省略）") || tight.contains("第一答"));
    }

    #[test]
    fn begin_rebuild_works_without_session() {
        let base = std::env::temp_dir().join(format!("convo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let store = ConvStore::new(&base);
        // session_ref 为空（首轮就崩的会话）也能重建
        store
            .upsert(conv(
                "r1",
                "",
                vec![msg("user", "建个站"), msg("assistant", "")],
            ))
            .unwrap();
        let text = store.begin_rebuild("r1", 5).unwrap();
        assert_eq!(text, "建个站");
        let c = store.get("r1").unwrap();
        assert_eq!(c.status, "running");
        assert_eq!(c.messages.len(), 1); // 失败助手已弹掉
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn begin_retry_rejects_no_session_and_missing() {
        let base = std::env::temp_dir().join(format!("convo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let store = ConvStore::new(&base);
        store
            .upsert(conv("b", "", vec![msg("user", "问"), msg("assistant", "")]))
            .unwrap();
        assert!(store.begin_retry("b", 1).is_err()); // 无 session
        assert!(store.begin_retry("nope", 1).is_err()); // 会话不存在
        std::fs::remove_dir_all(&base).ok();
    }
}
