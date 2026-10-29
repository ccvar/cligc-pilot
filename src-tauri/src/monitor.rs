//! 网站在线监测：每小时打开一次每个站点的首页（打不开连续两次就通知），每天看一眼 HTTPS 证书还有多久过期，
//! 每周用 RDAP 查一次域名到期日。证书 14 天内、域名 60 天内到期会提醒。只发网络请求，不用 AI。
//! 结果存在站点数据目录的 cache/uptime.json。同一轮里顺带查有没有被标记为危险网站（safety，每天一次）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const HOUR: u64 = 3600;
const DAY: u64 = 86_400;
/// 证书 / 域名还剩多少天开始提醒
pub const CERT_WARN_DAYS: i64 = 14;
pub const DOMAIN_WARN_DAYS: i64 = 60;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Ping {
    pub ts: u64,
    pub ok: bool,
    pub status: u16,
    pub ms: u32,
    #[serde(default)]
    pub err: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Monitor {
    /// 最近 7 天的首页检查（旧的在前）
    pub pings: Vec<Ping>,
    pub cert_expires: u64,
    pub cert_err: String,
    pub cert_checked: u64,
    pub domain_expires: u64,
    pub domain_err: String,
    pub domain_checked: u64,
    /// 已经发过「打不开」通知、还没恢复
    #[serde(default)]
    pub down_alerted: bool,
    #[serde(default)]
    pub cert_alert_at: u64,
    #[serde(default)]
    pub domain_alert_at: u64,
}

impl Monitor {
    /// 最近两次都打不开才算挂了（一次超时可能只是网络抖动）
    pub fn down(&self) -> bool {
        self.pings.len() >= 2 && self.pings.iter().rev().take(2).all(|p| !p.ok)
    }
    pub fn down_since(&self) -> u64 {
        if !self.down() {
            return 0;
        }
        self.pings.iter().rev().take_while(|p| !p.ok).last().map(|p| p.ts).unwrap_or(0)
    }
    pub fn cert_days(&self, now: u64) -> Option<i64> {
        (self.cert_expires > 0).then(|| (self.cert_expires as i64 - now as i64).div_euclid(DAY as i64))
    }
    pub fn domain_days(&self, now: u64) -> Option<i64> {
        (self.domain_expires > 0).then(|| (self.domain_expires as i64 - now as i64).div_euclid(DAY as i64))
    }
}

fn path(site_dir: &Path) -> PathBuf {
    crate::site_data::cache_dir(site_dir).join("uptime.json")
}

pub fn load(site_dir: &Path) -> Monitor {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(site_dir: &Path, m: &Monitor) {
    let p = path(site_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec(m) {
        let _ = std::fs::write(p, raw);
    }
}

// ───────────────────────── 三项检查 ─────────────────────────

async fn ping(domain: &str) -> Ping {
    let c = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Mozilla/5.0 (compatible; CligcPilot-Uptime/1.0)")
        .build()
        .unwrap_or_default();
    let t0 = std::time::Instant::now();
    let ts = crate::now_secs();
    match c.get(format!("https://{domain}/")).send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let ok = status < 400;
            Ping { ts, ok, status, ms: t0.elapsed().as_millis() as u32, err: if ok { String::new() } else { format!("首页返回 {status}") } }
        }
        Err(e) => Ping { ts, ok: false, status: 0, ms: t0.elapsed().as_millis() as u32, err: if e.is_timeout() { "15 秒没打开（超时）".into() } else if e.is_connect() { "连不上服务器".into() } else { "打不开".into() } },
    }
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_platform_verifier()
        .map_err(|e| e.to_string())?
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// 连一次 443，读服务器证书的到期时间。证书已经过期或不被信任时握手会失败，返回失败原因。
async fn cert_expiry(domain: &str) -> Result<u64, String> {
    let t = Duration::from_secs(15);
    let tcp = tokio::time::timeout(t, tokio::net::TcpStream::connect((domain, 443))).await.map_err(|_| "连接超时".to_string())?.map_err(|e| format!("连不上：{e}"))?;
    let name = rustls::pki_types::ServerName::try_from(domain.to_string()).map_err(|e| e.to_string())?;
    let tls = tokio::time::timeout(t, tokio_rustls::TlsConnector::from(tls_config()?).connect(name, tcp))
        .await
        .map_err(|_| "建立 HTTPS 连接超时".to_string())?
        .map_err(|e| {
            let s = e.to_string();
            if s.to_lowercase().contains("expired") { "证书已经过期".to_string() } else { format!("证书有问题：{s}") }
        })?;
    let der = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).ok_or("服务器没有给出证书")?;
    not_after(der.as_ref()).ok_or_else(|| "读不出证书的到期时间".to_string())
}

/// 读 DER 里的一个 TLV：（标签, 内容, 剩下的）
fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = b.split_first()?;
    let (&l0, rest) = rest.split_first()?;
    let (len, rest) = if l0 & 0x80 == 0 {
        (l0 as usize, rest)
    } else {
        let n = (l0 & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        (rest[..n].iter().fold(0usize, |a, &x| (a << 8) | x as usize), &rest[n..])
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

/// 证书的 notAfter：Certificate → TBSCertificate → [version] serial signature issuer validity{notBefore, notAfter}
fn not_after(der: &[u8]) -> Option<u64> {
    let (_, cert, _) = tlv(der)?;
    let (_, tbs, _) = tlv(cert)?;
    let mut rest = tbs;
    let (tag, _, r) = tlv(rest)?;
    if tag == 0xA0 {
        rest = r; // 有 version 字段
    }
    for _ in 0..3 {
        rest = tlv(rest)?.2; // serial、signature、issuer
    }
    let (_, validity, _) = tlv(rest)?;
    let (_, _, after) = tlv(validity)?;
    let (tag, t, _) = tlv(after)?;
    let s = std::str::from_utf8(t).ok()?.trim_end_matches('Z');
    let full = match tag {
        0x17 => format!("{}{s}", if s[..2].parse::<u32>().ok()? < 50 { "20" } else { "19" }), // UTCTime：YYMMDDHHMMSS
        0x18 => s.to_string(),                                                                 // GeneralizedTime：YYYYMMDDHHMMSS
        _ => return None,
    };
    chrono::NaiveDateTime::parse_from_str(&full, "%Y%m%d%H%M%S").ok().map(|d| d.and_utc().timestamp().max(0) as u64)
}

/// 用 RDAP（公开的域名注册信息查询）查域名到期日。查的是注册的主域名（www.x.com → x.com）。
async fn domain_expiry(domain: &str) -> Result<u64, String> {
    let root = registrable(domain);
    // rdap.org 不带 User-Agent 的请求一律回 403
    let c = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("CligcPilot/1.0 (domain expiry check)").build().unwrap_or_default();
    let r = c.get(format!("https://rdap.org/domain/{root}")).header("accept", "application/rdap+json").send().await.map_err(|e| format!("查不到：{e}"))?;
    if !r.status().is_success() {
        return Err(match r.status().as_u16() {
            404 => "这个域名后缀查不到公开的注册信息".to_string(),
            429 => "查询太频繁，稍后再查".to_string(),
            s => format!("注册信息查询失败（{s}），稍后再查"),
        });
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    let date = v["events"].as_array().and_then(|a| a.iter().find(|e| e["eventAction"] == "expiration")).and_then(|e| e["eventDate"].as_str()).ok_or("注册信息里没有到期日")?;
    chrono::DateTime::parse_from_rfc3339(date).map(|d| d.timestamp().max(0) as u64).map_err(|e| e.to_string())
}

/// 粗略取注册域名：最后两段（co.uk 这类两段后缀取三段）
fn registrable(domain: &str) -> String {
    let parts: Vec<&str> = domain.trim_end_matches('.').split('.').collect();
    let two_level = parts.len() >= 3 && matches!(parts[parts.len() - 2], "co" | "com" | "net" | "org" | "gov" | "edu" | "ac") && parts[parts.len() - 1].len() == 2;
    let n = if two_level { 3 } else { 2 };
    parts[parts.len().saturating_sub(n)..].join(".")
}

// ───────────────────────── 检查一个站点 ─────────────────────────

/// 检查一个站点。force＝三项都现在查（「立即检查」）；否则按周期：首页每小时、证书每天、域名每周。
/// 返回需要发的通知（标题, 内容）。
pub async fn check_site(site: &crate::sites::Site, site_dir: &Path, force: bool) -> (Monitor, Vec<(String, String)>) {
    let domain = site.domain.trim().trim_end_matches('/').to_ascii_lowercase();
    let mut m = load(site_dir);
    let mut alerts = Vec::new();
    if domain.is_empty() {
        return (m, alerts);
    }
    let now = crate::now_secs();
    if force || m.pings.last().is_none_or(|p| now.saturating_sub(p.ts) >= HOUR - 300) {
        let p = ping(&domain).await;
        m.pings.push(p);
        m.pings.retain(|p| now.saturating_sub(p.ts) <= 7 * DAY);
        if m.down() && !m.down_alerted {
            m.down_alerted = true;
            alerts.push((format!("{domain} 打不开"), format!("连续两次没打开：{}", m.pings.last().map(|p| p.err.clone()).unwrap_or_default())));
        } else if !m.down() && m.down_alerted && m.pings.last().is_some_and(|p| p.ok) {
            m.down_alerted = false;
            alerts.push((format!("{domain} 恢复了"), "首页又能打开了".into()));
        }
    }
    if force || now.saturating_sub(m.cert_checked) >= DAY {
        m.cert_checked = now;
        match cert_expiry(&domain).await {
            Ok(t) => { m.cert_expires = t; m.cert_err.clear(); }
            Err(e) => m.cert_err = e,
        }
        let soon = m.cert_days(now).is_some_and(|d| d <= CERT_WARN_DAYS);
        if (soon || !m.cert_err.is_empty()) && now.saturating_sub(m.cert_alert_at) >= 3 * DAY {
            m.cert_alert_at = now;
            alerts.push((format!("{domain} 的 HTTPS 证书要处理"), if m.cert_err.is_empty() { format!("{} 天后过期", m.cert_days(now).unwrap_or(0)) } else { m.cert_err.clone() }));
        }
    }
    // 查到了每周查一次；没查到（网络、限流）6 小时后再试
    let domain_every = if m.domain_err.is_empty() && m.domain_expires > 0 { 7 * DAY } else { 6 * HOUR };
    if force || now.saturating_sub(m.domain_checked) >= domain_every {
        m.domain_checked = now;
        if !force {
            // 后台挨个站点查时放慢一点，别一下子把公开的 RDAP 服务打到限流
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        match domain_expiry(&domain).await {
            Ok(t) => { m.domain_expires = t; m.domain_err.clear(); }
            Err(e) => m.domain_err = e,
        }
        if m.domain_days(now).is_some_and(|d| d <= DOMAIN_WARN_DAYS) && now.saturating_sub(m.domain_alert_at) >= 7 * DAY {
            m.domain_alert_at = now;
            alerts.push((format!("{} 快到期了", registrable(&domain)), format!("域名 {} 天后到期，记得续费", m.domain_days(now).unwrap_or(0))));
        }
    }
    save(site_dir, &m);
    (m, alerts)
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 调度循环每轮（10 分钟）调一次：有到期的检查就在后台挨个站点查（同一时间只跑一轮），该提醒的发系统通知。
pub fn schedule(app: &tauri::AppHandle) {
    use tauri::Manager;
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<crate::AppState>();
        let mut alerts = Vec::new();
        for site in st.sites.list().into_iter().filter(|s| !s.domain.trim().is_empty()) {
            let dir = st.sites.data_dir(&site.id);
            let (_, a) = check_site(&site, &dir, false).await;
            alerts.extend(a);
            // 有没有被标记为危险网站：每天一次（见 safety）
            let (_, a) = crate::safety::check_site(&site, &dir, false).await;
            alerts.extend(a);
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        for (title, body) in alerts.into_iter().take(5) {
            crate::push::notify(&app, &title, &body);
        }
        use tauri::Emitter;
        let _ = app.emit("monitor-checked", ());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_certificate_expiry_from_der() {
        use base64::Engine;
        let der = base64::engine::general_purpose::STANDARD.decode("MIIDdDCCAxqgAwIBAgIRAJy2PyyGpDeGDufTC5+dtFAwCgYIKoZIzj0EAwIwOzELMAkGA1UEBhMCVVMxHjAcBgNVBAoTFUdvb2dsZSBUcnVzdCBTZXJ2aWNlczEMMAoGA1UEAxMDV0UxMB4XDTI2MTAyMjEwMzQzNVoXDTI3MDEyMDExMzQzMlowFjEUMBIGA1UEAxMLZ2F0ZXNvci5jb20wWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAARNf1GZQzfZOqk+OdOZ4a8UVWssQo8iH3KWBIW+P5MXuO9pD1qeQrQpsoNhv2OjHTpt5rjCRp6bPTzImuWV2r0co4ICIjCCAh4wDgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAwGA1UdEwEB/wQCMAAwHQYDVR0OBBYEFF4PMf/CEHzLldezrgAkUGZQRrVVMB8GA1UdIwQYMBaAFJB3kjVnxP+ozKnme9mAeXvMk/k4MDUGCCsGAQUFBwEBBCkwJzAlBggrBgEFBQcwAoYZaHR0cDovL2kucGtpLmdvb2cvd2UxLmNydDAWBgNVHREEDzANggtnYXRlc29yLmNvbTATBgNVHSAEDDAKMAgGBmeBDAECATA2BgNVHR8ELzAtMCugKaAnhiVodHRwOi8vYy5wa2kuZ29vZy93ZTEvc0FsTTd6MUhXWVkuY3JsMIIBCwYKKwYBBAHWeQIEAgSB/ASB+QD3AHYARqI5Z8YNtkaHxm89+ZmUdpOmphEghFfVVefj0KHZtkYAAAGhY2Q8wgAABAMARzBFAiAzdTTS4n0lHvSTbRwCqZfufGoEKnOXg8iNwJbUq7qyvwIhAJU301hsM2zltlon3cHwRbMej7+hM83Wkk4mYSllPu4PAH0AGouda43XkdHNBUnttgNV1ga2T60w23H+eI8Px8j7xLEAAAGhY2RDrAAIAAAFAAwGJt4EAwBGMEQCIFoqvRTRkJgkY40ZUOgVvNC63F2tEJeMJ5JOerlMTrpbAiBZwx7+Xg3cRxuMoYtUQJK9k9fMxhYlfebWtuvs3Fs++DAKBggqhkjOPQQDAgNIADBFAiEA9DkDtuhZBwaOkLpYPSIdnABQSDkP4tkye16pfp3BMMICIFu5U51DXlRgb+hwLBfvQdyjzoleD6ULNYx6EbvYc7b6").unwrap();
        // gatesor.com 的证书：notAfter=Jan 20 11:34:32 2027 GMT
        let t = not_after(&der).unwrap();
        assert_eq!(chrono::DateTime::from_timestamp(t as i64, 0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string(), "2027-01-20 11:34:32");
        assert!(not_after(&[0x30, 0x03, 0x02, 0x01, 0x00]).is_none());
    }

    #[test]
    fn two_failed_pings_in_a_row_mean_down() {
        let p = |ok| Ping { ok, ..Default::default() };
        let m = Monitor { pings: vec![p(true), p(false)], ..Default::default() };
        assert!(!m.down());
        let m = Monitor { pings: vec![p(true), p(false), p(false)], ..Default::default() };
        assert!(m.down());
        assert_eq!(registrable("www.gatesor.com"), "gatesor.com");
        assert_eq!(registrable("shop.example.co.uk"), "example.co.uk");
    }
}
