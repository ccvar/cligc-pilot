//! 连接存储：Cloudflare（API Token 建站）与 SSH（远程机器）两类连接。
//! 元数据存 connections.json（原子写），密钥/口令只进系统钥匙串（account = 连接 id）。

use serde::{Deserialize, Serialize};


use std::fs;

use std::path::{Path, PathBuf};


use crate::keychain;

#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    /// 用户自定义备注；列表优先显示它，密钥仍只显示脱敏前缀。
    #[serde(default)]
    pub remark: String,
    /// 连接类型：cloudflare（CF token 建站）| ssh（远程机器）。
    pub kind: String,
    /// 该连接的工作区目录（CF 项目放在其下的 projects/）。
    pub skill_dir: String,
    /// 仅用于展示的 key 前缀，完整 key 在 Keychain。
    pub key_prefix: String,
    /// cf_token（Cloudflare）/ ssh。
    pub key_kind: String,
    /// Cloudflare 账号 id（仅 kind=cloudflare）。
    #[serde(default)]
    pub account_id: String,
    /// 以下均仅 kind=ssh。密码/私钥口令进钥匙串（account = conn_id，与其它 kind 同约定，
    /// 因此 remove 无需特殊处理、不会留孤儿密钥）；私钥只存路径引用，不拷贝用户私钥。
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default)]
    pub ssh_port: u16,
    #[serde(default)]
    pub ssh_user: String,
    /// password | key
    #[serde(default)]
    pub ssh_auth: String,
    #[serde(default)]
    pub ssh_key_path: String,
    /// 用户已确认的主机指纹（TOFU）：此后每次连接必须匹配，不匹配即拒（防中间人）。
    #[serde(default)]
    pub ssh_fingerprint: String,
    /// 远端系统（/etc/os-release 的 PRETTY_NAME，如 "Ubuntu 24.04.1 LTS"）。首次连上时探一次并存下来，
    /// 之后直接显示、不再连（空 = 还没探过）。
    #[serde(default)]
    pub ssh_os: String,
    /// 发行版 id（os-release 的 ID，如 ubuntu/debian/alpine）——UI 据此选发行版图标。
    #[serde(default)]
    pub ssh_os_id: String,
    pub created_at: String,
}

#[derive(Clone)]
pub struct ConnStore {
    file: PathBuf,
    workspaces_dir: PathBuf,
}

impl ConnStore {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let workspaces_dir = data_dir.join("connections");
        fs::create_dir_all(&workspaces_dir).map_err(|e| format!("create connections dir: {e}"))?;
        Ok(Self {
            file: data_dir.join("connections.json"),
            workspaces_dir,
        })
    }

    pub fn list(&self) -> Vec<Connection> {
        let Ok(raw) = fs::read(&self.file) else {
            return Vec::new();
        };
        serde_json::from_slice(&raw).unwrap_or_default()
    }

    fn save(&self, conns: &[Connection]) -> Result<(), String> {
        let raw = serde_json::to_vec_pretty(conns).map_err(|e| e.to_string())?;
        // 同目录临时文件 + rename，避免写一半（崩溃/磁盘满）清空整个连接列表。
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, &raw).map_err(|e| format!("write connections tmp: {e}"))?;
        fs::rename(&tmp, &self.file).map_err(|e| format!("replace connections.json: {e}"))
    }

    pub fn get(&self, id: &str) -> Result<Connection, String> {
        self.list()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("未找到连接 {id}"))
    }


    /// 连接 Cloudflare：token 进钥匙串，建该连接的工作区目录，写 connections.json。
    /// 调用方应已先验证过 token（lib.rs 的 connect_cloudflare 会验证）。
    pub fn add_cloudflare(
        &self,
        name: &str,
        token: &str,
        account_id: &str,
    ) -> Result<Connection, String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("Token 不能为空".into());
        }
        let prefix = keychain::key_prefix(token);
        // 去重：同账号 + 同 token 前缀视为同一连接。
        if let Some(dup) = self.list().iter().find(|c| {
            c.kind == "cloudflare" && c.account_id == account_id && c.key_prefix == prefix
        }) {
            return Err(format!("已存在相同的 Cloudflare 连接「{}」。", dup.name));
        }
        let conn_id = uuid::Uuid::new_v4().to_string();
        // 每个 CF 连接一个工作区目录（站点项目放在其下的 projects/）。
        let dir = self.workspaces_dir.join(&conn_id);
        fs::create_dir_all(&dir).map_err(|e| format!("create cf workspace: {e}"))?;
        keychain::set_key(&conn_id, token)?;
        let conn = Connection {
            id: conn_id.clone(),
            name: if name.trim().is_empty() {
                "Cloudflare".into()
            } else {
                name.trim().into()
            },
            remark: String::new(),
            kind: "cloudflare".into(),
            skill_dir: dir.to_string_lossy().into_owned(),
            key_prefix: prefix,
            key_kind: "cf_token".into(),
            account_id: account_id.trim().to_string(),
            ssh_host: String::new(),
            ssh_port: 0,
            ssh_user: String::new(),
            ssh_auth: String::new(),
            ssh_key_path: String::new(),
            ssh_fingerprint: String::new(),
            ssh_os: String::new(),
            ssh_os_id: String::new(),
            created_at: chrono_now(),
        };
        let mut conns = self.list();
        conns.push(conn.clone());
        if let Err(e) = self.save(&conns) {
            let _ = keychain::delete_key(&conn_id);
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(conn)
    }

    /// 新建 SSH 远程连接：密码/私钥口令进钥匙串，建工作区目录，写 connections.json。
    /// 调用方必须已先 probe 过并由用户确认了主机指纹（TOFU）——没有指纹一律拒绝。
    #[allow(clippy::too_many_arguments)]
    pub fn add_ssh(
        &self,
        name: &str,
        host: &str,
        port: u16,
        user: &str,
        auth: &str,
        key_path: &str,
        secret: &str,
        fingerprint: &str,
    ) -> Result<Connection, String> {
        let (host, user, key_path, fingerprint) = (
            host.trim(),
            user.trim(),
            key_path.trim(),
            fingerprint.trim(),
        );
        if host.is_empty() {
            return Err("主机不能为空".into());
        }
        if user.is_empty() {
            return Err("用户名不能为空".into());
        }
        if fingerprint.is_empty() {
            return Err("缺少主机指纹：请先「测试连接」并确认指纹".into());
        }
        match auth {
            "password" if secret.is_empty() => return Err("密码不能为空".into()),
            "key" if key_path.is_empty() => return Err("请选择私钥文件".into()),
            "password" | "key" => {}
            other => return Err(format!("未知认证方式: {other}")),
        }
        let port = if port == 0 { 22 } else { port };
        // 去重：同一台机器的同一用户视为同一连接。
        if let Some(dup) = self.list().iter().find(|c| {
            c.kind == "ssh" && c.ssh_host == host && c.ssh_port == port && c.ssh_user == user
        }) {
            return Err(format!(
                "已存在相同的远程连接「{}」（{user}@{host}:{port}）。",
                dup.name
            ));
        }
        let conn_id = uuid::Uuid::new_v4().to_string();
        let dir = self.workspaces_dir.join(&conn_id);
        fs::create_dir_all(&dir).map_err(|e| format!("create ssh workspace: {e}"))?;
        // 无口令的私钥没有秘密可存 —— 不写钥匙串（读侧用 get_key(..).ok() 兜底）。
        if !secret.is_empty() {
            if let Err(e) = keychain::set_key(&conn_id, secret) {
                let _ = fs::remove_dir_all(&dir);
                return Err(e);
            }
        }
        let conn = Connection {
            id: conn_id.clone(),
            name: if name.trim().is_empty() {
                format!("{user}@{host}")
            } else {
                name.trim().into()
            },
            remark: String::new(),
            kind: "ssh".into(),
            skill_dir: dir.to_string_lossy().into_owned(),
            // 故意留空：key_prefix 会显示在连接列表里，SSH 这里的秘密是密码/口令，
            // 照抄前缀等于把密码前 13 位印在 UI 上。副标题改显 user@host:port。
            key_prefix: String::new(),
            key_kind: if auth == "key" {
                "ssh_key".into()
            } else {
                "ssh_password".into()
            },
            account_id: String::new(),
            ssh_host: host.into(),
            ssh_port: port,
            ssh_user: user.into(),
            ssh_auth: auth.into(),
            ssh_key_path: key_path.into(),
            ssh_fingerprint: fingerprint.into(),
            // 系统信息首次连上时再探（add 时不连，避免加连接就多一次登录记录）。
            ssh_os: String::new(),
            ssh_os_id: String::new(),
            created_at: chrono_now(),
        };
        let mut conns = self.list();
        conns.push(conn.clone());
        if let Err(e) = self.save(&conns) {
            let _ = keychain::delete_key(&conn_id);
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(conn)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut conns = self.list();
        let before = conns.len();
        conns.retain(|c| c.id != id);
        if conns.len() == before {
            return Err(format!("未找到连接 {id}"));
        }
        // 先删钥匙串：失败则中止（连接保留，可重试），绝不留下无 UI 句柄的孤儿密钥。
        keychain::delete_key(id)?;
        self.save(&conns)?;
        let _ = fs::remove_dir_all(self.workspaces_dir.join(id));
        Ok(())
    }

    /// 设置连接备注。空字符串表示清除；限制为单行短文本，避免把连接切换器撑坏。
    pub fn set_remark(&self, id: &str, remark: &str) -> Result<Connection, String> {
        let remark = remark.trim();
        if remark.chars().count() > 60 {
            return Err("备注最多 60 个字符".into());
        }
        if remark.chars().any(char::is_control) {
            return Err("备注只能填写单行文字".into());
        }
        let mut conns = self.list();
        let Some(slot) = conns.iter_mut().find(|c| c.id == id) else {
            return Err(format!("未找到连接 {id}"));
        };
        slot.remark = remark.to_string();
        let out = slot.clone();
        self.save(&conns)?;
        Ok(out)
    }

    /// 只替换指定 Cloudflare 连接的 Token；连接 id、账号、Zone 绑定和工作区全部保留。
    /// 调用方必须先验证新 Token 仍可访问原 account_id。
    pub fn replace_cloudflare_token(&self, id: &str, token: &str) -> Result<Connection, String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("Token 不能为空".into());
        }
        let mut conns = self.list();
        let Some(target) = conns.iter().position(|connection| connection.id == id) else {
            return Err(format!("未找到连接 {id}"));
        };
        if conns[target].kind != "cloudflare" {
            return Err("这不是 Cloudflare 连接".into());
        }
        let prefix = keychain::key_prefix(token);
        if let Some(duplicate) = conns.iter().enumerate().find(|(index, connection)| {
            *index != target
                && connection.kind == "cloudflare"
                && connection.account_id == conns[target].account_id
                && connection.key_prefix == prefix
        }) {
            return Err(format!(
                "这枚 Token 已用于 Cloudflare 连接「{}」",
                duplicate.1.name
            ));
        }
        let previous = keychain::get_key(id)?;
        keychain::set_key(id, token)?;
        conns[target].key_prefix = prefix;
        let out = conns[target].clone();
        if let Err(error) = self.save(&conns) {
            let restored = keychain::set_key(id, &previous);
            return Err(if let Err(restore_error) = restored {
                format!("{error}；恢复原 Token 失败：{restore_error}")
            } else {
                error
            });
        }
        Ok(out)
    }

    /// 记录远端系统信息（首次连上探到后写入，之后 UI 直接显示、不再连）。
    pub fn set_ssh_os(&self, id: &str, pretty: &str, os_id: &str) -> Result<Connection, String> {
        let mut conns = self.list();
        let Some(slot) = conns.iter_mut().find(|c| c.id == id) else {
            return Err(format!("未找到连接 {id}"));
        };
        slot.ssh_os = pretty.trim().to_string();
        slot.ssh_os_id = os_id.trim().to_lowercase();
        let out = slot.clone();
        self.save(&conns)?;
        Ok(out)
    }

    /// 修改已有的远程连接。
    ///
    /// 密码/口令：`secret` 为空 = 不动钥匙串里那条（UI 不回显密码，留空即保持原样）。
    /// **换认证方式时必须给新密钥**——旧钥匙串条目存的是另一种东西（密码 vs 私钥口令），留着必错。
    /// 指纹由调用方给（UI 侧刚试连确认过的那个）：换机器/换端口后旧指纹不再作数。
    #[allow(clippy::too_many_arguments)]
    pub fn update_ssh(
        &self,
        id: &str,
        name: &str,
        host: &str,
        port: u16,
        user: &str,
        auth: &str,
        key_path: &str,
        secret: &str,
        fingerprint: &str,
    ) -> Result<Connection, String> {
        let (host, user, key_path, fingerprint) = (
            host.trim(),
            user.trim(),
            key_path.trim(),
            fingerprint.trim(),
        );
        if host.is_empty() {
            return Err("主机不能为空".into());
        }
        if user.is_empty() {
            return Err("用户名不能为空".into());
        }
        if fingerprint.is_empty() {
            return Err("缺少主机指纹：请先「测试连接」并确认指纹".into());
        }
        let port = if port == 0 { 22 } else { port };
        let mut conns = self.list();
        let Some(old) = conns.iter().find(|c| c.id == id).cloned() else {
            return Err(format!("未找到连接 {id}"));
        };
        if old.kind != "ssh" {
            return Err("这不是远程连接".into());
        }
        // 先确认旧凭据是否真的还在。仅凭“以前也是密码认证”不能允许空保存：迁移失败
        // 正是连接记录仍在、钥匙串条目已丢的状态，继续空保存只会制造假成功。
        let old_secret = keychain::get_key(id).ok();
        match auth {
            "key" if key_path.is_empty() => return Err("请选择私钥文件".into()),
            "password"
                if secret.is_empty() && (old.ssh_auth != "password" || old_secret.is_none()) =>
            {
                return Err("钥匙串里没有这个连接的密码，请补录密码".into())
            }
            "key" | "password" => {}
            other => return Err(format!("未知认证方式: {other}")),
        }
        // 去重：改成另一台已存在的机器（同 user@host:port）＝和那条连接撞车。
        if let Some(dup) = conns.iter().find(|c| {
            c.id != id
                && c.kind == "ssh"
                && c.ssh_host == host
                && c.ssh_port == port
                && c.ssh_user == user
        }) {
            return Err(format!(
                "已存在相同的远程连接「{}」（{user}@{host}:{port}）。",
                dup.name
            ));
        }
        // 动钥匙串前已把旧值抄下来：存盘要是失败了，得原样放回去（无口令私钥本就没有条目 → None）。
        // 钥匙串先写：写失败就整个中止（连接还是原样，可重试），别让记录和密钥对不上。
        if !secret.is_empty() {
            keychain::set_key(id, secret)?;
        } else if auth == "key" && old.ssh_auth == "password" {
            // 密码登录 → 无口令私钥：旧密码留着就是个错的口令，必须清掉。
            let _ = keychain::delete_key(id);
        }
        let restore_secret = || match &old_secret {
            Some(s) => {
                let _ = keychain::set_key(id, s);
            }
            None => {
                let _ = keychain::delete_key(id);
            }
        };
        let slot = conns
            .iter_mut()
            .find(|c| c.id == id)
            .expect("checked above");
        slot.name = if name.trim().is_empty() {
            format!("{user}@{host}")
        } else {
            name.trim().into()
        };
        slot.ssh_host = host.into();
        slot.ssh_port = port;
        slot.ssh_user = user.into();
        slot.ssh_auth = auth.into();
        slot.ssh_key_path = if auth == "key" {
            key_path.into()
        } else {
            String::new()
        };
        slot.ssh_fingerprint = fingerprint.into();
        // 换了机器就别留旧系统信息（下次连上重探）。
        if old.ssh_host != host || old.ssh_port != port {
            slot.ssh_os = String::new();
            slot.ssh_os_id = String::new();
        }
        let out = slot.clone();
        if let Err(e) = self.save(&conns) {
            // 存盘失败：把钥匙串放回旧值，别留下「密钥是新的、记录是旧的」的错位状态。
            restore_secret();
            return Err(e);
        }
        Ok(out)
    }

}

fn chrono_now() -> String {
    // 避免引入 chrono：秒级 unix 时间足够（UI 端格式化）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_remark_rules() {
        let base = std::env::temp_dir().join(format!("pilot-remark-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let store = ConnStore::new(&base).unwrap();
        let conn = Connection {
            id: "c1".into(),
            name: "cf".into(),
            remark: String::new(),
            kind: "cloudflare".into(),
            skill_dir: String::new(),
            key_prefix: "cf_ab".into(),
            key_kind: "cf_token".into(),
            account_id: String::new(),
            ssh_host: String::new(),
            ssh_port: 0,
            ssh_user: String::new(),
            ssh_auth: String::new(),
            ssh_key_path: String::new(),
            ssh_fingerprint: String::new(),
            ssh_os: String::new(),
            ssh_os_id: String::new(),
            created_at: "t".into(),
        };
        store.save(&[conn]).unwrap();
        let saved = store.set_remark("c1", "  cms.ccvar.com  ").unwrap();
        assert_eq!(saved.remark, "cms.ccvar.com");
        assert_eq!(store.get("c1").unwrap().remark, "cms.ccvar.com");
        assert!(store.set_remark("c1", &"a".repeat(61)).is_err());
        assert!(store.set_remark("c1", "two\nlines").is_err());
        assert_eq!(store.set_remark("c1", "").unwrap().remark, "");
        fs::remove_dir_all(&base).ok();
    }

    /// 改远程连接：改地址/改名/换认证方式，以及不该发生的事（撞车、非 ssh、缺指纹）。
    /// 注意不碰钥匙串（CI 无钥匙串）：只走 secret 留空的路径。
    #[test]
    fn update_ssh_rules() {
        let base = std::env::temp_dir().join(format!("pilot-upd-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let store = ConnStore::new(&base).unwrap();
        let mk = |id: &str, host: &str, user: &str| Connection {
            id: id.into(),
            name: format!("{user}@{host}"),
            kind: "ssh".into(),
            remark: String::new(),
            skill_dir: String::new(),
            key_prefix: String::new(),
            key_kind: String::new(),
            account_id: String::new(),
            ssh_host: host.into(),
            ssh_port: 22,
            ssh_user: user.into(),
            ssh_auth: "key".into(),
            ssh_key_path: "/k/id_ed25519".into(),
            ssh_fingerprint: "SHA256:old".into(),
            ssh_os: "Ubuntu 24.04".into(),
            ssh_os_id: "ubuntu".into(),
            created_at: "t".into(),
        };
        let cf = Connection {
            kind: "cloudflare".into(),
            ..mk("g1", "", "")
        };
        // Connection 没有 Debug（也不该为了测试给它加），自己取错误文本
        fn err_of(r: Result<Connection, String>) -> String {
            match r {
                Err(e) => e,
                Ok(_) => panic!("这一步本该失败"),
            }
        }
        store
            .save(&[
                mk("c1", "1.1.1.1", "root"),
                mk("c2", "2.2.2.2", "root"),
                cf,
            ])
            .unwrap();

        // 改名 + 换指纹（同机器）：系统信息保留，不必重探
        let c = store
            .update_ssh(
                "c1",
                "生产机",
                "1.1.1.1",
                22,
                "root",
                "key",
                "/k/id_ed25519",
                "",
                "SHA256:new",
            )
            .unwrap();
        assert_eq!(c.name, "生产机");
        assert_eq!(c.ssh_fingerprint, "SHA256:new");
        assert_eq!(
            c.ssh_os, "Ubuntu 24.04",
            "同一台机器不该丢掉已探到的系统信息"
        );

        // 换了机器：系统信息必须清掉（否则显示的是上一台机器的系统）
        let c = store
            .update_ssh(
                "c1",
                "",
                "9.9.9.9",
                2222,
                "root",
                "key",
                "/k/id_ed25519",
                "",
                "SHA256:x",
            )
            .unwrap();
        assert_eq!(c.ssh_host, "9.9.9.9");
        assert_eq!(c.ssh_port, 2222);
        assert_eq!(c.ssh_os, "");
        assert_eq!(c.ssh_os_id, "");
        assert_eq!(c.name, "root@9.9.9.9", "名字留空＝回到默认 user@host");

        // 撞上另一条已有连接（同 user@host:port）→ 拒
        let err = err_of(store.update_ssh(
            "c1",
            "",
            "2.2.2.2",
            22,
            "root",
            "key",
            "/k/id_ed25519",
            "",
            "SHA256:x",
        ));
        assert!(err.contains("已存在相同的远程连接"), "{err}");
        // 自己和自己不算撞车
        assert!(store
            .update_ssh(
                "c1",
                "",
                "9.9.9.9",
                2222,
                "root",
                "key",
                "/k/id_ed25519",
                "",
                "SHA256:x"
            )
            .is_ok());

        // 缺指纹 → 拒（TOFU 的底线：没确认过就不给存）
        assert!(store
            .update_ssh(
                "c1",
                "",
                "9.9.9.9",
                2222,
                "root",
                "key",
                "/k/id_ed25519",
                "",
                "  "
            )
            .is_err());
        // key 认证但没给私钥路径 → 拒
        assert!(store
            .update_ssh("c1", "", "9.9.9.9", 2222, "root", "key", "", "", "SHA256:x")
            .is_err());
        // 从密钥换成密码却不给密码 → 拒（钥匙串里那条是私钥口令，不是密码，留着必错）
        let err = err_of(store.update_ssh(
            "c1", "", "9.9.9.9", 2222, "root", "password", "", "", "SHA256:x",
        ));
        assert!(err.contains("补录密码"), "{err}");
        // 非 ssh 连接 → 拒
        assert!(store
            .update_ssh("g1", "x", "h", 22, "u", "key", "/k", "", "SHA256:x")
            .is_err());
        // 不存在的连接 → 拒
        assert!(store
            .update_ssh("nope", "x", "h", 22, "u", "key", "/k", "", "SHA256:x")
            .is_err());

        // 没被碰过的那条要原样还在（别把别人写坏了）
        let c2 = store.get("c2").unwrap();
        assert_eq!(c2.ssh_host, "2.2.2.2");
        assert_eq!(c2.ssh_fingerprint, "SHA256:old");
        fs::remove_dir_all(&base).ok();
    }

}
