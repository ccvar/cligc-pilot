//! 安卓手机：通过 adb 让 AI 在对话里操作手机 app、截图（给教程文章配真实截图）。
//!
//! - adb 来自 Google 官方的 platform-tools：用户在设置里点「安装」时下载到 `<数据目录>/tools/platform-tools/`，
//!   也认 PATH 和 Android Studio 的默认位置。
//! - AI 用随附的 `<数据目录>/tools/phone.js` 操作手机（见 `phone_prompt`），它自己找 adb。
//! - 密码框不代填（phone.js 里拦）；登录、短信验证码、支付、提交这些由提示词要求 AI 停下来交给用户。
//! - 邮箱验证码由 Pilot 自己去邮箱取、直接填进手机（见 `mail`），AI 只用 `mail-code` 命令触发。

use serde::Serialize;
use std::path::{Path, PathBuf};

fn exe() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

/// Pilot 自己装的 platform-tools 目录。
pub fn platform_tools_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tools").join("platform-tools")
}

/// 找 adb：Pilot 装的 > PATH > Android Studio 默认位置 > Homebrew。
pub fn adb_path(data_dir: &Path) -> Option<PathBuf> {
    let own = platform_tools_dir(data_dir).join(exe());
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    let mut cands = vec![own];
    cands.extend(crate::brains::which_all("adb").into_iter().map(PathBuf::from));
    cands.push(home.join("Library/Android/sdk/platform-tools").join(exe()));
    cands.push(home.join("Android/Sdk/platform-tools").join(exe()));
    cands.push(home.join("AppData/Local/Android/Sdk/platform-tools").join(exe()));
    cands.push(PathBuf::from("/opt/homebrew/bin/adb"));
    cands.push(PathBuf::from("/usr/local/bin/adb"));
    cands.into_iter().find(|p| p.is_file())
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub serial: String,
    /// device（可用）| unauthorized（手机上还没点允许）| offline
    pub state: String,
    pub model: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct PhoneStatus {
    /// adb 的路径；空＝没装
    pub adb: String,
    /// 是不是 Pilot 自己装的（可以在设置里卸载 / 更新）
    pub managed: bool,
    pub devices: Vec<Device>,
    pub error: String,
}

/// `adb devices -l` 的输出 → 设备列表。
pub fn parse_devices(raw: &str) -> Vec<Device> {
    raw.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let serial = it.next()?.to_string();
            let state = it.next()?.to_string();
            let model = it.find_map(|kv| kv.strip_prefix("model:")).unwrap_or("").replace('_', " ");
            Some(Device { serial, state, model })
        })
        .collect()
}

pub async fn status(data_dir: &Path) -> PhoneStatus {
    let Some(adb) = adb_path(data_dir) else { return PhoneStatus::default() };
    let managed = adb.starts_with(platform_tools_dir(data_dir));
    let adb_s = adb.to_string_lossy().into_owned();
    let out = tokio::process::Command::new(&adb).args(["devices", "-l"]).kill_on_drop(true).output();
    match tokio::time::timeout(std::time::Duration::from_secs(15), out).await {
        Ok(Ok(o)) => PhoneStatus {
            adb: adb_s,
            managed,
            devices: parse_devices(&String::from_utf8_lossy(&o.stdout)),
            error: if o.status.success() { String::new() } else { String::from_utf8_lossy(&o.stderr).trim().chars().take(300).collect() },
        },
        Ok(Err(e)) => PhoneStatus { adb: adb_s, managed, error: format!("运行 adb 失败：{e}"), ..Default::default() },
        Err(_) => PhoneStatus { adb: adb_s, managed, error: "adb 没有响应（15 秒）".into(), ..Default::default() },
    }
}

fn platform_tools_url() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("https://dl.google.com/android/repository/platform-tools-latest-darwin.zip")
    } else if cfg!(windows) {
        Some("https://dl.google.com/android/repository/platform-tools-latest-windows.zip")
    } else if cfg!(target_os = "linux") {
        Some("https://dl.google.com/android/repository/platform-tools-latest-linux.zip")
    } else {
        None
    }
}

/// 从 Google 下载官方 platform-tools，解压到 `<数据目录>/tools/platform-tools/`。
pub async fn install(data_dir: &Path) -> Result<String, String> {
    let url = platform_tools_url().ok_or("这个系统不支持自动安装 adb")?;
    let resp = reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(300)).send().await.map_err(|e| format!("下载失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：服务器返回 {}", resp.status()));
    }
    let data = resp.bytes().await.map_err(|e| format!("下载失败：{e}"))?;
    let tools = data_dir.join("tools");
    let target = platform_tools_dir(data_dir);
    let data_dir2 = tools.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(&data_dir2).map_err(|e| e.to_string())?;
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
        // 压缩包里的顶层目录就叫 platform-tools，直接解到 tools/ 下
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("读取压缩包：{e}"))?;
        ar.extract(&data_dir2).map_err(|e| format!("解压：{e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["adb", "fastboot"] {
                let p = target.join(name);
                if p.is_file() {
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    let adb = platform_tools_dir(data_dir).join(exe());
    if !adb.is_file() {
        return Err("解压后没找到 adb".into());
    }
    Ok(adb.to_string_lossy().into_owned())
}

/// 写给 AI 的说明：怎么用随附的 phone.js 操作安卓手机、截图，以及哪些事必须停下来交给用户。
pub fn phone_prompt(phone_js: &str) -> String {
    format!(
        "【安卓手机】用户的安卓手机可能用 USB 连着这台电脑，可以用随附工具操作手机上的 app、截取真实截图（给教程文章配图）：\n\
`node \"{phone_js}\" devices`（先看有没有连上）\n\
`node \"{phone_js}\" ui`（列出屏幕上的按钮、文字、输入框和坐标；先看再点）\n\
`node \"{phone_js}\" tap --text \"注册\"`（按文字点，最可靠；有多处时加 --index）· `tap --x 540 --y 1200`\n\
`node \"{phone_js}\" swipe up|down|left|right` · `key back|home|enter` · `type --text \"hello\"`（只能英文和数字）\n\
`node \"{phone_js}\" apps --search 关键词`（找包名）· `launch <包名>` · `open --url <链接>` · `wait 1500`\n\
`node \"{phone_js}\" shot --out <站点图片目录>/<有意义的文件名>.png`\n\
每条命令输出一行 JSON。做法：每做一步先 `wait` 一下让页面加载完，再 `ui` 或截图确认到了想要的页面；截完**必须打开图片看**\
（Read；Pilot 原生 API 执行器里用 view_image），确认不是加载中、弹窗、黑屏。返回 `likely_blank` 多半是 app 禁止截图\
（交易所、银行常这样），告诉用户这一步截不了，不要反复重试。\n\
必须停下来交给用户、不要自己点的：登录、输入密码 / 短信验证码 / 身份信息、实名认证上传证件、充值 / 提现 / 下单 / 转账 / 支付、\
确认提交、授权和权限弹窗、发消息或发帖、删除。走到这一步就在回复里说清楚要用户在手机上做什么，等用户说「好了」再接着做。\n\
【邮箱验证码】用户让你走注册流程、要填邮箱时：先 `node \"{phone_js}\" mail-info` 看 Pilot 有没有接邮箱（configured=false 就照旧停下来交给用户）。\
接了就填它给的邮箱；plus_alias=true 时用 alias_example 那种「名字+别名@」写法，每篇教程换一个别名，免得和用户的真账号冲突。\
点了「发送验证码」后，先 tap 验证码输入框，再 `node \"{phone_js}\" mail-code --from <发邮件的网站域名，如 okx.com>`：\
Pilot 会去邮箱里找这个网站刚发来的验证码、直接填进手机，只告诉你填好了（你拿不到邮件内容，也不需要）。\
邮件里是激活链接时用 `mail-link --from <域名>`，Pilot 会在手机上打开它。只用在用户要你做的注册（和用户在场时的登录）上；\
不要用来取谷歌、苹果、微软、银行的验证码，不要拿它重置密码。截图里出现的验证码也打码。\n\
截图里有手机号、邮箱、账号、余额、订单号、二维码、头像等个人信息时，先打码或裁掉再放进文章；状态栏的时间电量可以保留。\
没有连上手机时（`no_device`），告诉用户：用 USB 线连接、在手机「开发者选项」里打开「USB 调试」，并在 Pilot「设置 → 安卓手机」里确认已连上。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_adb_devices() {
        let raw = "* daemon started successfully\nList of devices attached\nR5CT1234       device usb:1-1 product:dm3q model:SM_S918B device:dm3q transport_id:1\nemulator-5554  unauthorized\n\n";
        let d = parse_devices(raw);
        assert_eq!(d, vec![
            Device { serial: "R5CT1234".into(), state: "device".into(), model: "SM S918B".into() },
            Device { serial: "emulator-5554".into(), state: "unauthorized".into(), model: String::new() },
        ]);
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }
}
