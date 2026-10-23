//! VPS 部署：项目自带发布脚本（scp / ssh / rsync 到自己的服务器），由 Pilot 在构建检查通过后执行。
//!
//! 发布脚本通常要人在终端里输 SSH 密码，后台跑不了。做法是：
//! - 用 Pilot 里已有的 SSH 连接（凭据在钥匙串）在服务器上一次性加一把 Pilot 专用的部署公钥；
//!   私钥只进钥匙串，部署时才临时写进一个 0600 的临时目录，跑完即删。
//! - 执行发布脚本时，在 PATH 最前面放 `ssh` / `scp` 包装：强制只用这把密钥、只认已知主机、禁止交互。
//!   ssh 的同名选项「先出现的生效」，所以脚本自己写的 `-o PubkeyAuthentication=no`、`BatchMode=no` 都会被覆盖。
//!   发布脚本本身（备份、健康检查、失败恢复）原样保留。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::conns::Connection;

#[derive(Serialize, Deserialize, Clone)]
struct StoredKey {
    private: String,
    public: String,
}

fn account(ssh_conn_id: &str) -> String {
    format!("deploy-key:{ssh_conn_id}")
}

fn comment(ssh_conn_id: &str) -> String {
    format!("cligc-pilot-deploy-{}", &ssh_conn_id[..ssh_conn_id.len().min(8)])
}

fn load(ssh_conn_id: &str) -> Option<StoredKey> {
    let raw = crate::keychain::get_named_secret(&account(ssh_conn_id)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 公钥只允许 `ssh-ed25519 <base64> <注释>` 这种形状：它会被拼进远端的 shell 命令。
pub fn safe_pubkey(p: &str) -> bool {
    let parts: Vec<&str> = p.split_whitespace().collect();
    parts.len() == 3
        && parts[0] == "ssh-ed25519"
        && parts[1].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        && parts[2].chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 生成一把 ed25519 部署密钥（本机 ssh-keygen），存进钥匙串。已有就直接返回。
pub fn ensure_key(ssh_conn_id: &str) -> Result<String, String> {
    if let Some(k) = load(ssh_conn_id) {
        return Ok(k.public);
    }
    let t = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = t.path().join("k");
    let out = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", &comment(ssh_conn_id), "-f"])
        .arg(&path)
        .output()
        .map_err(|e| format!("生成部署密钥失败（需要系统自带的 ssh-keygen）：{e}"))?;
    if !out.status.success() {
        return Err(format!("生成部署密钥失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let private = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let public = std::fs::read_to_string(path.with_extension("pub")).map_err(|e| e.to_string())?.trim().to_string();
    if !safe_pubkey(&public) {
        return Err("生成的公钥格式不对".into());
    }
    let raw = serde_json::to_string(&StoredKey { private, public: public.clone() }).map_err(|e| e.to_string())?;
    crate::keychain::set_named_secret(&account(ssh_conn_id), &raw)?;
    Ok(public)
}

pub fn has_key(ssh_conn_id: &str) -> bool {
    load(ssh_conn_id).is_some()
}

pub fn public_key(ssh_conn_id: &str) -> Option<String> {
    load(ssh_conn_id).map(|k| k.public)
}

fn add_cmd(pubkey: &str) -> String {
    format!(
        "umask 077; mkdir -p ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && \
(grep -qxF '{pubkey}' ~/.ssh/authorized_keys || printf '%s\\n' '{pubkey}' >> ~/.ssh/authorized_keys) && echo PILOT_KEY_OK"
    )
}

fn remove_cmd(comment: &str) -> String {
    format!(
        "umask 077; [ -f ~/.ssh/authorized_keys ] || exit 0; grep -v ' {comment}$' ~/.ssh/authorized_keys > ~/.ssh/authorized_keys.pilot-tmp; \
mv ~/.ssh/authorized_keys.pilot-tmp ~/.ssh/authorized_keys && echo PILOT_KEY_REMOVED"
    )
}

async fn session(sessions: &crate::ssh::SshSessions, conn: &Connection) -> Result<(), String> {
    let auth = crate::ssh::auth_for(conn)?;
    let expect = (!conn.ssh_fingerprint.is_empty()).then(|| conn.ssh_fingerprint.clone());
    sessions.ensure(&conn.id, &conn.ssh_host, conn.ssh_port, &auth, expect).await
}

/// 在服务器上加部署公钥（走 Pilot 里已认证的 SSH 连接），然后用这把密钥实际登录一次确认。
pub async fn install(sessions: &crate::ssh::SshSessions, conn: &Connection) -> Result<String, String> {
    let pubkey = ensure_key(&conn.id)?;
    if !safe_pubkey(&pubkey) {
        return Err("部署公钥格式不对".into());
    }
    session(sessions, conn).await?;
    let out = sessions.exec(&conn.id, &add_cmd(&pubkey), 30, false).await?;
    if !out.stdout.contains("PILOT_KEY_OK") {
        return Err(format!("在服务器上添加部署密钥失败：{}", out.stderr.trim()));
    }
    verify(conn).await?;
    Ok(pubkey)
}

/// 撤销：从服务器的 authorized_keys 删掉这把公钥，再删掉钥匙串里的私钥。
pub async fn revoke(sessions: &crate::ssh::SshSessions, conn: &Connection) -> Result<(), String> {
    session(sessions, conn).await?;
    let out = sessions.exec(&conn.id, &remove_cmd(&comment(&conn.id)), 30, false).await?;
    if !out.stdout.contains("PILOT_KEY_REMOVED") {
        return Err(format!("从服务器删除部署密钥失败：{}", out.stderr.trim()));
    }
    let _ = crate::keychain::delete_named_secret(&account(&conn.id));
    Ok(())
}

/// 部署时用的临时目录：私钥（0600）+ `ssh` / `scp` 包装。Drop 即删。
pub struct DeployShim {
    dir: tempfile::TempDir,
}

impl DeployShim {
    pub fn bin_dir(&self) -> PathBuf {
        self.dir.path().join("bin")
    }
}

fn wrapper(real: &str, key: &Path) -> String {
    format!(
        "#!/bin/sh\n# Cligc Pilot：只用部署密钥、只认已知主机、不交互。\nexec {real} -i '{}' -o IdentitiesOnly=yes -o PubkeyAuthentication=yes -o PreferredAuthentications=publickey -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no -o BatchMode=yes -o StrictHostKeyChecking=yes \"$@\"\n",
        key.display()
    )
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}

pub fn shim(ssh_conn_id: &str) -> Result<DeployShim, String> {
    let k = load(ssh_conn_id).ok_or("还没有为部署授权：请在站点设置里点「为部署授权」")?;
    let dir = tempfile::Builder::new().prefix("pilot-deploy-").tempdir().map_err(|e| e.to_string())?;
    let key = dir.path().join("id_ed25519");
    std::fs::write(&key, &k.private).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    chmod(&key, 0o600);
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for (name, real) in [("ssh", crate::brains::resolve_bin("ssh")), ("scp", crate::brains::resolve_bin("scp"))] {
        let real = if Path::new(&real).is_file() { real } else { format!("/usr/bin/{name}") };
        let p = bin.join(name);
        std::fs::write(&p, wrapper(&real, &key)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        chmod(&p, 0o700);
    }
    Ok(DeployShim { dir })
}

/// 用部署密钥实际登录一次（不经过包装，直接调系统 ssh）。
pub async fn verify(conn: &Connection) -> Result<(), String> {
    let s = shim(&conn.id)?;
    let target = format!("{}@{}", conn.ssh_user, conn.ssh_host);
    let out = tokio::process::Command::new(s.bin_dir().join("ssh"))
        .args(["-p", &conn.ssh_port.to_string(), &target, "echo PILOT_DEPLOY_OK"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("启动 ssh 失败：{e}"))?;
    if String::from_utf8_lossy(&out.stdout).contains("PILOT_DEPLOY_OK") {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if err.contains("Host key verification failed") || err.contains("No ED25519 host key") || err.contains("known") {
        format!("本机的 known_hosts 里没有 {} 的主机密钥：先在终端里用 ssh 连一次这台服务器并确认指纹", conn.ssh_host)
    } else {
        format!("用部署密钥登录失败：{err}")
    })
}

/// 为发布脚本准备的环境：PATH 最前面是包装目录。
pub fn deploy_env(s: &DeployShim) -> Vec<(String, String)> {
    let path = std::env::var("PATH").unwrap_or_default();
    vec![("PATH".into(), format!("{}:{path}", s.bin_dir().display()))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_ed25519_keys_reach_the_remote_shell() {
        assert!(safe_pubkey("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEx+/= cligc-pilot-deploy-0102b4ab"));
        assert!(!safe_pubkey("ssh-ed25519 AAAA cligc' ; rm -rf ~ #"));
        assert!(!safe_pubkey("ssh-rsa AAAA x"));
        assert!(!safe_pubkey("ssh-ed25519 AAAA"));
    }

    #[test]
    fn remote_commands_only_touch_the_pilot_key() {
        let c = add_cmd("ssh-ed25519 AAAA cligc-pilot-deploy-1");
        assert!(c.contains("grep -qxF 'ssh-ed25519 AAAA cligc-pilot-deploy-1'"));
        let r = remove_cmd("cligc-pilot-deploy-1");
        assert!(r.contains("grep -v ' cligc-pilot-deploy-1$'"));
    }

    #[test]
    fn wrapper_forces_key_auth_before_script_options() {
        let w = wrapper("/usr/bin/ssh", Path::new("/tmp/k"));
        let i_key = w.find("-i '/tmp/k'").unwrap();
        let args = w.find("\"$@\"").unwrap();
        assert!(i_key < args);
        assert!(w.contains("PubkeyAuthentication=yes"));
        assert!(w.contains("BatchMode=yes"));
        assert!(w.contains("StrictHostKeyChecking=yes"));
    }
}
