//! Pilot 托管的 VPS 发布：Pilot 自己把服务器配好，不需要项目自带发布脚本。基础是 **Caddy + Go + SQLite**：
//! Caddy 管静态文件、旧站跳转和自动 HTTPS；表单 / 评论由 Go 写的 pilot-site 小服务处理（接口和 Cloudflare 上的 Functions 一致），
//! 数据存 SQLite。站点 `deploy.mode = "vps"` 且 `deploy.command` 为空 = 托管发布（有发布命令的照旧走 site_vps 的「项目自带脚本」）。
//!
//! - 连接：用 Pilot 里已有的 SSH 连接（russh，凭据在钥匙串、主机指纹已确认）执行命令、用 SFTP 传文件，不依赖本机 known_hosts。
//! - 布局：`/var/www/pilot/<站点>/` 下 `releases/<时间>/`（每次发布一个目录）、`current`（指向当前版本，切换是原子的，
//!   保留最近 5 个版本，可以回退）、`redirects.caddy`（旧站跳转，从构建产物的 `_redirects` 转成 Caddy 规则）、
//!   `large/`（超过 25 MB 的大文件）、`data/site.db`（表单 / 评论）。
//! - Caddy：站点配置在 `/etc/caddy/pilot/<站点>.caddy`，主配置 `import` 这个目录（服务器上原有的 Caddy 站点不动）。
//!   切换前站点只开 HTTP（避免域名还没指过来时 Caddy 反复申请证书失败、撞上 Let's Encrypt 的频率限制），切换时打开自动 HTTPS。
//! - pilot-site：Pilot 打包了 linux amd64 / arm64 两个版本（build.rs 编译），传到 `/usr/local/bin/pilot-site`，每个站点一个 systemd 服务。
//! - 需要 root 或免密 sudo。80 / 443 端口被别的程序（nginx、Apache）占着时明确报错，不去停别人的服务。
//!
//! 状态存 `<站点数据>/vps.json`。

use crate::sites::Site;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Go 写的站点小服务（gzip），build.rs 编译；本机没 Go 时是空的。
pub const PILOT_SITE_AMD64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pilot-site-linux-amd64.gz"));
pub const PILOT_SITE_ARM64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pilot-site-linux-arm64.gz"));
const PILOT_SITE_BIN: &str = "/usr/local/bin/pilot-site";
const KEEP_RELEASES: usize = 5;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct VpsState {
    pub ssh_conn_id: String,
    /// 服务器上的站点目录名
    pub slug: String,
    pub root: String,
    /// 主域名；迁移中是旧站的主机名
    pub host: String,
    /// 另一种写法（www / 不带 www），301 到主域名
    #[serde(default)]
    pub alias: String,
    /// 服务器公网 IP（切换时 A 记录指向它）
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub trailing: String,
    /// 表单 / 评论服务的端口（0＝没开）
    #[serde(default)]
    pub api_port: u16,
    #[serde(default)]
    pub large: bool,
    /// 已经打开自动 HTTPS（切换后）
    #[serde(default)]
    pub https: bool,
    /// 另一种写法也指过来了，一起申请证书
    #[serde(default)]
    pub alias_https: bool,
    #[serde(default)]
    pub setup_at: u64,
    #[serde(default)]
    pub https_at: u64,
    /// 已发布的版本（新的在后）
    #[serde(default)]
    pub releases: Vec<String>,
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl VpsState {
    pub fn site_conf(&self) -> String {
        format!("/etc/caddy/pilot/{}.caddy", self.slug)
    }
    pub fn db_path(&self) -> String {
        format!("{}/data/site.db", self.root)
    }
    pub fn service(&self) -> String {
        format!("pilot-site-{}", self.slug)
    }
}

pub fn path(site_dir: &Path) -> PathBuf {
    site_dir.join("vps.json")
}
pub fn load(site_dir: &Path) -> Option<VpsState> {
    std::fs::read(path(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}
pub fn save(site_dir: &Path, v: &VpsState) -> Result<(), String> {
    std::fs::write(path(site_dir), serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// 托管发布：VPS 模式、没有自带发布命令。
pub fn managed(site: &Site) -> bool {
    site.deploy.mode == "vps" && site.deploy.command.trim().is_empty()
}

/// 这个构建里带了 pilot-site（编译时有 Go）。
pub fn has_pilot_site() -> bool {
    !PILOT_SITE_AMD64.is_empty() && !PILOT_SITE_ARM64.is_empty()
}

/// 服务器上的目录名：域名（或旧站域名 / 项目名 / 站点名）只留小写字母数字和 . -
pub fn slug_of(site: &Site) -> String {
    let base = [&site.domain, &site.migrate_from, &site.deploy.project, &site.name].into_iter().find(|s| !s.trim().is_empty()).cloned().unwrap_or_else(|| site.id.clone());
    let s: String = base.trim().trim_start_matches("www.").to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    let s = s.trim_matches(['-', '.']).chars().take(60).collect::<String>();
    if s.is_empty() { site.id.chars().take(12).collect() } else { s }
}

/// 表单 / 评论服务的端口：按站点名散列到 18000–18999，同一台机器上多个站点基本不撞。
pub fn api_port(slug: &str) -> u16 {
    let h = slug.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    18000 + (h % 1000) as u16
}

/// 另一种写法：apex ↔ www（多级子域名没有）。
pub fn alias_of(host: &str) -> String {
    if let Some(a) = host.strip_prefix("www.") {
        return a.to_string();
    }
    if host.matches('.').count() == 1 {
        return format!("www.{host}");
    }
    String::new()
}

fn cq(p: &str) -> String {
    // 占位符 {re.r1.1} 的花括号 Caddy 要原样看到，不用引号
    if p.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '#')) {
        format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        p.to_string()
    }
}

fn regex_escape(s: &str) -> String {
    s.chars().map(|c| if "\\.+*?()|[]{}^$".contains(c) { format!("\\{c}") } else { c.to_string() }).collect()
}

/// 把 Cloudflare Pages 的 `_redirects` 转成 Caddy 规则（被站点配置 import）。
/// 不包 `route`：Caddy 的指令有固定顺序，`redir` / `rewrite` 排在 `handle` 前面，包进 `route` 反而会被静态文件的 handle 抢先。
/// 支持：精确路径、结尾 `*`（目标里 `:splat`）、`:name` 占位、状态码 301 / 302 / 307 / 308 / 200（改写）、外站目标。
pub fn redirects_caddy(txt: &str) -> String {
    let mut body = String::new();
    let mut seen = BTreeSet::new();
    let mut n = 0;
    for line in txt.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        let mut it = l.split_whitespace();
        let (Some(from), Some(to)) = (it.next(), it.next()) else { continue };
        let code: u16 = it.next().map(|c| c.trim_end_matches('!')).and_then(|c| c.parse().ok()).unwrap_or(302);
        let to_ok = to.starts_with('/') || to.starts_with("http://") || to.starts_with("https://");
        if !from.starts_with('/') || !to_ok || !seen.insert(from.to_string()) {
            continue;
        }
        let code_s = if matches!(code, 301 | 302 | 307 | 308) { code.to_string() } else { "302".into() };
        let names: Vec<&str> = from.split('/').filter_map(|s| s.strip_prefix(':')).collect();
        let splat = from.ends_with('*');
        if !splat && names.is_empty() {
            if code == 200 {
                body.push_str(&format!("\trewrite {} {}\n", cq(from), cq(to)));
            } else {
                body.push_str(&format!("\tredir {} {} {code_s}\n", cq(from), cq(to)));
            }
            continue;
        }
        n += 1;
        let m = format!("r{n}");
        let mut re = String::from("^");
        let mut target = to.to_string();
        let mut g = 0;
        for (i, seg) in from.trim_end_matches('*').split('/').enumerate() {
            if i > 0 {
                re.push('/');
            }
            if let Some(name) = seg.strip_prefix(':') {
                g += 1;
                re.push_str("([^/]+)");
                target = target.replace(&format!(":{name}"), &format!("{{re.{m}.{g}}}"));
            } else {
                re.push_str(&regex_escape(seg));
            }
        }
        if splat {
            g += 1;
            re.push_str("(.*)");
            target = target.replace(":splat", &format!("{{re.{m}.{g}}}"));
        }
        re.push('$');
        body.push_str(&format!("\t@{m} path_regexp {m} {}\n", cq(&re)));
        if code == 200 {
            body.push_str(&format!("\trewrite @{m} {}\n", cq(&target)));
        } else {
            body.push_str(&format!("\tredir @{m} {} {code_s}\n", cq(&target)));
        }
    }
    format!("# Cligc Pilot 由 _redirects 生成，每次发布覆盖\n{body}")
}

/// 站点的 Caddy 配置。`v.https` 为 false 时只开 HTTP（切换前），true 时由 Caddy 自动申请证书。
pub fn caddy_site(v: &VpsState) -> String {
    let scheme = if v.https { "" } else { "http://" };
    let files = match v.trailing.as_str() {
        // 旧站都以 / 结尾：目录交给 file_server（/a 会跳到 /a/）
        "always" => "{path} {path}.html",
        _ => "{path} {path}.html {path}/index.html",
    };
    let mut s = format!(
        "# Cligc Pilot 托管：{host}（站点目录 {root}）\n{scheme}{host} {{\n\troot * {root}/current\n\tencode zstd gzip\n\timport {root}/redirects.caddy\n",
        host = v.host,
        root = v.root,
    );
    if v.api_port > 0 {
        s.push_str(&format!("\thandle /api/* {{\n\t\treverse_proxy 127.0.0.1:{}\n\t}}\n", v.api_port));
    }
    if v.large {
        s.push_str(&format!("\t@large file {{\n\t\troot {root}/large\n\t\ttry_files {{path}}\n\t}}\n\thandle @large {{\n\t\troot * {root}/large\n\t\tfile_server\n\t}}\n", root = v.root));
    }
    s.push_str(&format!(
        "\thandle {{\n\t\ttry_files {files}\n\t\tfile_server\n\t}}\n\theader /_astro/* Cache-Control \"public, max-age=31536000, immutable\"\n\thandle_errors {{\n\t\t@404 expression {{err.status_code}} == 404\n\t\thandle @404 {{\n\t\t\trewrite * /404.html\n\t\t\tfile_server\n\t\t}}\n\t}}\n}}\n"
    ));
    if !v.alias.is_empty() {
        // 另一种写法：切换后只有它也指过来了才申请证书，不然一直只走 HTTP 跳转
        let alias_scheme = if v.https && v.alias_https { "" } else { "http://" };
        let target = if v.https { "https" } else { "http" };
        s.push_str(&format!("{alias_scheme}{alias} {{\n\tredir {target}://{host}{{uri}} 301\n}}\n", alias = v.alias, host = v.host));
    }
    s
}

/// 脚本开头：出错即停；不是 root 就用免密 sudo，没有就明确报错。
const PRELUDE: &str = r#"set -eu
step() { echo "PILOT_STEP $1"; }
if [ "$(id -u)" = 0 ]; then SUDO=""; elif sudo -n true 2>/dev/null; then SUDO="sudo -n"; else echo "PILOT_ERR need_sudo"; exit 41; fi
reload_caddy() { $SUDO caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1 || { $SUDO caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile 2>&1 | tail -5; return 1; }; $SUDO systemctl reload caddy 2>/dev/null || $SUDO caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile; }
"#;

/// 检测服务器：系统、架构、能不能 sudo、80/443 端口被谁占着、有没有 Caddy。
pub const DETECT_SCRIPT: &str = r#"step detect
. /etc/os-release 2>/dev/null || true
echo "PILOT_OS ${PRETTY_NAME:-unknown}"
echo "PILOT_ARCH $(uname -m)"
command -v caddy >/dev/null 2>&1 && echo "PILOT_CADDY $(caddy version 2>/dev/null | head -1)"
if command -v ss >/dev/null 2>&1; then
  for p in 80 443; do
    who=$($SUDO ss -ltnpH "sport = :$p" 2>/dev/null | grep -o 'users:(("[^"]*"' | head -1 | sed 's/users:(("//')
    [ -n "$who" ] && echo "PILOT_PORT $p $who"
  done
fi
echo PILOT_OK
"#;

/// 一次性配置服务器：装 Caddy（没装的话）、建目录和站点配置、（有表单评论时）建 pilot-site 服务。
/// 之前要先把 `<root>/site.caddy.new`（和 pilot-site 的 gz）传上去。
pub fn setup_script(v: &VpsState, user: &str) -> String {
    let mut s = String::from(PRELUDE);
    s.push_str(DETECT_SCRIPT.trim_end_matches("echo PILOT_OK\n"));
    s.push_str(&format!(
        r#"for p in 80 443; do
  who=$($SUDO ss -ltnpH "sport = :$p" 2>/dev/null | grep -o 'users:(("[^"]*"' | head -1 | sed 's/users:(("//') || true
  if [ -n "$who" ] && [ "$who" != caddy ]; then echo "PILOT_ERR port_busy $p $who"; exit 47; fi
done
if ! command -v caddy >/dev/null 2>&1; then
  step install
  if command -v apt-get >/dev/null 2>&1; then
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get update -qq
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq debian-keyring debian-archive-keyring apt-transport-https curl gnupg >/dev/null
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | $SUDO gpg --batch --yes --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt | $SUDO tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get update -qq
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq caddy || true
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y -q 'dnf-command(copr)' >/dev/null 2>&1 || true
    $SUDO dnf copr enable -y @caddy/caddy >/dev/null 2>&1 && $SUDO dnf install -y -q caddy || true
  fi
  if ! command -v caddy >/dev/null 2>&1; then
    # 包管理器装不上：直接下载官方的单个可执行文件，自己建 systemd 服务
    case "$(uname -m)" in x86_64) A=amd64;; aarch64|arm64) A=arm64;; *) echo "PILOT_ERR caddy_arch"; exit 43;; esac
    $SUDO curl -fsSL --max-time 300 -o /usr/bin/caddy "https://caddyserver.com/api/download?os=linux&arch=$A" || {{ echo "PILOT_ERR caddy_download"; exit 44; }}
    $SUDO chmod 755 /usr/bin/caddy
    id caddy >/dev/null 2>&1 || $SUDO useradd --system --home /var/lib/caddy --create-home --shell /usr/sbin/nologin caddy
    $SUDO mkdir -p /etc/caddy
    $SUDO tee /etc/systemd/system/caddy.service >/dev/null <<'PILOT_UNIT'
[Unit]
Description=Caddy
After=network-online.target
Wants=network-online.target
[Service]
Type=notify
User=caddy
Group=caddy
ExecStart=/usr/bin/caddy run --environ --config /etc/caddy/Caddyfile
ExecReload=/usr/bin/caddy reload --config /etc/caddy/Caddyfile --force
TimeoutStopSec=5s
LimitNOFILE=1048576
AmbientCapabilities=CAP_NET_BIND_SERVICE
[Install]
WantedBy=multi-user.target
PILOT_UNIT
    $SUDO systemctl daemon-reload
  fi
fi
step dirs
$SUDO mkdir -p {root}/releases {root}/data {root}/large /etc/caddy/pilot
$SUDO chown -R {user} {root}
$SUDO chmod 755 /var/www /var/www/pilot {root} 2>/dev/null || true
[ -f {root}/redirects.caddy ] || echo '# Cligc Pilot：旧站跳转，发布时生成' > {root}/redirects.caddy
if [ ! -e {root}/current ]; then mkdir -p {root}/releases/empty && echo '<!doctype html><title>Cligc Pilot</title>' > {root}/releases/empty/index.html && ln -sfn releases/empty {root}/current; fi
step caddy
# 主配置：装好时自带的欢迎页配置换成只 import Pilot 的目录；用户自己写过的保留，只补一行 import
if [ ! -s /etc/caddy/Caddyfile ] || grep -q '/usr/share/caddy' /etc/caddy/Caddyfile; then
  printf '# Cligc Pilot：站点配置在 /etc/caddy/pilot/\nimport /etc/caddy/pilot/*.caddy\n' | $SUDO tee /etc/caddy/Caddyfile >/dev/null
elif ! grep -q '/etc/caddy/pilot/' /etc/caddy/Caddyfile; then
  printf '\nimport /etc/caddy/pilot/*.caddy\n' | $SUDO tee -a /etc/caddy/Caddyfile >/dev/null
fi
$SUDO cp {root}/site.caddy.new {conf}
$SUDO systemctl enable caddy >/dev/null 2>&1 || true
$SUDO systemctl is-active --quiet caddy || $SUDO systemctl start caddy
reload_caddy
if command -v ufw >/dev/null 2>&1 && $SUDO ufw status | grep -q active; then $SUDO ufw allow 80/tcp >/dev/null; $SUDO ufw allow 443/tcp >/dev/null; fi
if command -v firewall-cmd >/dev/null 2>&1 && $SUDO firewall-cmd --state >/dev/null 2>&1; then $SUDO firewall-cmd --permanent --add-service=http --add-service=https >/dev/null && $SUDO firewall-cmd --reload >/dev/null || true; fi
step ip
IP=$(curl -fsS4 --max-time 8 https://api.ipify.org 2>/dev/null || curl -fsS4 --max-time 8 https://ifconfig.me 2>/dev/null || hostname -I 2>/dev/null | awk '{{print $1}}')
echo "PILOT_IP $IP"
"#,
        root = v.root,
        conf = v.site_conf(),
        user = user,
    ));
    if v.api_port > 0 {
        s.push_str(&format!(
            r#"step api
gunzip -c {root}/pilot-site.gz > {root}/pilot-site.new && chmod 755 {root}/pilot-site.new && rm -f {root}/pilot-site.gz
$SUDO mv -f {root}/pilot-site.new {bin}
if [ "$(id -u)" = 0 ]; then id pilot >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin pilot; SVC_USER=pilot; chown -R pilot {root}/data; else SVC_USER={user}; fi
$SUDO tee /etc/systemd/system/{svc}.service >/dev/null <<PILOT_UNIT
[Unit]
Description=Cligc Pilot site service ({slug})
After=network.target
[Service]
User=$SVC_USER
ExecStart={bin} serve --port {port} --db {root}/data/site.db
EnvironmentFile=-{root}/data/env
Restart=always
RestartSec=3
NoNewPrivileges=true
[Install]
WantedBy=multi-user.target
PILOT_UNIT
$SUDO systemctl daemon-reload
$SUDO systemctl enable {svc} >/dev/null 2>&1
$SUDO systemctl restart {svc}
sleep 1
curl -fsS --max-time 5 http://127.0.0.1:{port}/api/health >/dev/null && echo "PILOT_API ok" || echo "PILOT_API down"
"#,
            root = v.root,
            bin = PILOT_SITE_BIN,
            svc = v.service(),
            slug = v.slug,
            user = user,
            port = v.api_port,
        ));
    }
    s.push_str("echo PILOT_OK\n");
    s
}

/// 发布：解开上传的包到新版本目录，原子切换 current，换上新的跳转规则（Caddy 配置检查不过就恢复旧的），只留最近几个版本。
pub fn deploy_script(v: &VpsState, release: &str) -> String {
    let mut s = String::from(PRELUDE);
    s.push_str(&format!(
        r#"R={root}
step unpack
mkdir -p "$R/releases/{rel}"
tar -xzf "$R/upload-{rel}.tar.gz" -C "$R/releases/{rel}"
rm -f "$R/upload-{rel}.tar.gz"
step switch
PREV=$(readlink "$R/current" 2>/dev/null || true)
cp "$R/redirects.caddy" "$R/redirects.caddy.prev" 2>/dev/null || true
mv -f "$R/redirects.caddy.new" "$R/redirects.caddy"
ln -sfn "releases/{rel}" "$R/current.tmp" && mv -Tf "$R/current.tmp" "$R/current"
if ! reload_caddy; then
  [ -n "$PREV" ] && ln -sfn "$PREV" "$R/current.tmp" && mv -Tf "$R/current.tmp" "$R/current"
  mv -f "$R/redirects.caddy.prev" "$R/redirects.caddy" 2>/dev/null || true
  reload_caddy || true
  echo "PILOT_ERR caddy_config"; exit 45
fi
step prune
cd "$R/releases" && ls -1 | grep -v '^empty$' | sort | head -n -{keep} | xargs -r rm -rf
echo "PILOT_RELEASES $(ls -1 "$R/releases" | grep -v '^empty$' | sort | tr '\n' ' ')"
echo PILOT_OK
"#,
        root = v.root,
        rel = release,
        keep = KEEP_RELEASES,
    ));
    s
}

/// 切到指定版本（回退）。
pub fn activate_script(v: &VpsState, release: &str) -> String {
    format!(
        "{PRELUDE}R={root}\n[ -d \"$R/releases/{rel}\" ] || {{ echo PILOT_ERR no_release; exit 46; }}\nln -sfn \"releases/{rel}\" \"$R/current.tmp\" && mv -Tf \"$R/current.tmp\" \"$R/current\"\necho PILOT_OK\n",
        root = v.root,
        rel = release
    )
}

/// 换站点配置（切换时打开 HTTPS / 回退时关掉）：先传 `<root>/site.caddy.new`。
pub fn reconf_script(v: &VpsState) -> String {
    format!("{PRELUDE}$SUDO cp {root}/site.caddy.new {conf}\nreload_caddy || {{ echo PILOT_ERR caddy_config; exit 45; }}\necho PILOT_OK\n", root = v.root, conf = v.site_conf())
}

/// 打开 HTTPS 后等 Caddy 拿到证书：在服务器上用 https 访问自己（--resolve 指向本机），最多等 2 分钟。
pub fn https_wait_script(host: &str) -> String {
    format!("for i in $(seq 1 24); do c=$(curl -s -o /dev/null --max-time 5 -w '%{{http_code}}' --resolve {host}:443:127.0.0.1 https://{host}/ || true); case \"$c\" in 2*|3*|404) echo \"PILOT_HTTPS $c\"; exit 0;; esac; sleep 5; done; echo PILOT_HTTPS none\n")
}

/// 域名还没指过来时在服务器上自测：带 Host 头请求本机 Caddy，逐个路径打印「状态码 跳转地址」。
pub fn live_check_script(host: &str, list_file: &str) -> String {
    format!(
        "while IFS= read -r p; do printf '%s\\t' \"$p\"; curl -s -o /dev/null --max-time 10 -w '%{{http_code}} %{{redirect_url}}\\n' -H 'Host: {host}' \"http://127.0.0.1$p\" || echo '000 '; done < {list_file}\n"
    )
}

/// 脚本输出里的 `PILOT_ERR xxx` 翻译成人话。
pub fn explain_err(out: &str) -> Option<String> {
    let line = out.lines().find_map(|l| l.strip_prefix("PILOT_ERR "))?.trim();
    let mut it = line.split_whitespace();
    let code = it.next().unwrap_or("");
    Some(match code {
        "need_sudo" => "这个 SSH 账号不是 root，也没有免密 sudo：换成 root 连接，或者在服务器上给它配置免密 sudo（NOPASSWD）".into(),
        "port_busy" => {
            let port = it.next().unwrap_or("");
            let who = it.next().unwrap_or("别的程序");
            format!("服务器的 {port} 端口被 {who} 占着（这台机器上已经有别的网站服务）。Pilot 不会去停它：换一台服务器，或者先把那边的网站迁走 / 停掉 {who} 再试")
        }
        "caddy_arch" => "服务器的 CPU 架构不支持自动安装 Caddy".into(),
        "caddy_download" => "下载 Caddy 失败（包管理器和官方下载都没成）：请在服务器上手动装好 Caddy 再试".into(),
        "caddy_config" => "新的 Caddy 配置检查没通过，已经恢复到上一个版本".into(),
        "no_release" => "服务器上没有这个版本".into(),
        x => format!("服务器上出错：{x}"),
    })
}

// ───────────────────────── 远程执行 ─────────────────────────

fn app_state() -> Result<tauri::State<'static, AppState>, String> {
    use tauri::Manager;
    Ok(crate::APP.get().ok_or("应用还没准备好")?.state::<AppState>())
}

async fn ensure_session(state: &AppState, conn: &crate::conns::Connection) -> Result<(), String> {
    let auth = crate::ssh::auth_for(conn)?;
    let expect = (!conn.ssh_fingerprint.is_empty()).then(|| conn.ssh_fingerprint.clone());
    state.ssh.ensure(&conn.id, &conn.ssh_host, conn.ssh_port, &auth, expect).await
}

/// 把脚本传到服务器的临时文件里执行（不经过 shell 转义），返回输出。退出码非 0 时报错（带上翻译过的原因）。
pub async fn run_script(conn: &crate::conns::Connection, script: &str, timeout: u64, job: Option<&crate::site_jobs::Job>) -> Result<String, String> {
    let state = app_state()?;
    ensure_session(&state, conn).await?;
    let tmp = format!("/tmp/pilot-{}.sh", uuid::Uuid::new_v4().simple());
    state.ssh.write_file(&conn.id, &tmp, script.as_bytes()).await?;
    let out = state.ssh.exec(&conn.id, &format!("bash {tmp}; rc=$?; rm -f {tmp}; exit $rc"), timeout, false).await?;
    if let Some(j) = job {
        for l in out.stdout.lines().chain(out.stderr.lines()).filter(|l| !l.trim().is_empty()).take(200) {
            j.line(l);
        }
    }
    let all = format!("{}\n{}", out.stdout, out.stderr);
    if out.code != 0 {
        return Err(explain_err(&all).unwrap_or_else(|| format!("服务器上执行失败（退出码 {}）：{}", out.code, out.stderr.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" / "))));
    }
    Ok(all)
}

pub async fn upload_bytes(conn: &crate::conns::Connection, remote: &str, data: &[u8]) -> Result<(), String> {
    let state = app_state()?;
    ensure_session(&state, conn).await?;
    state.ssh.write_file(&conn.id, remote, data).await
}

pub async fn upload_file(conn: &crate::conns::Connection, local: &Path, remote: &str) -> Result<u64, String> {
    let state = app_state()?;
    ensure_session(&state, conn).await?;
    state.ssh.upload(&conn.id, &local.to_string_lossy(), remote).await
}

fn tag(out: &str, key: &str) -> String {
    out.lines().find_map(|l| l.strip_prefix(&format!("{key} "))).unwrap_or("").trim().to_string()
}

/// 把构建产物打成 tar.gz（Pages 专用的 _redirects / _routes.json / _headers 不带；目录 755、文件 644）。
pub fn pack(out: &Path, dest: &Path) -> Result<usize, String> {
    let f = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mut n = 0usize;
    fn walk(base: &Path, dir: &Path, tar: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>, n: &mut usize) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let p = e.path();
            let rel = p.strip_prefix(base).map_err(|e| e.to_string())?;
            if rel.parent().is_none_or(|x| x.as_os_str().is_empty()) && matches!(rel.to_str(), Some("_redirects" | "_routes.json" | "_headers")) {
                continue;
            }
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(0o755);
                h.set_size(0);
                tar.append_data(&mut h, rel, std::io::empty()).map_err(|e| e.to_string())?;
                walk(base, &p, tar, n)?;
            } else if ft.is_file() {
                let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                let mut h = tar::Header::new_gnu();
                h.set_mode(0o644);
                h.set_size(data.len() as u64);
                h.set_mtime(0);
                tar.append_data(&mut h, rel, &data[..]).map_err(|e| e.to_string())?;
                *n += 1;
            }
        }
        Ok(())
    }
    walk(out, out, &mut tar, &mut n)?;
    tar.into_inner().map_err(|e| e.to_string())?.finish().map_err(|e| e.to_string())?;
    Ok(n)
}

// ───────────────────────── 配置服务器 ─────────────────────────

pub const SETUP_STEPS: &[(&str, &str)] = &[("connect", "连接服务器"), ("upload", "上传配置"), ("setup", "安装和配置 Caddy"), ("api", "表单和评论服务"), ("large", "上传大文件")];

/// 配置服务器（第一次，或者改了表单 / 评论之后）。`ssh_conn_id` 给了就顺便把站点的部署方式设成托管 VPS。
#[tauri::command]
pub async fn site_vps_setup(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String, ssh_conn_id: Option<String>) -> Result<String, String> {
    let mut site = state.sites.get(&id)?;
    if let Some(c) = ssh_conn_id.filter(|c| !c.is_empty()) {
        let conn = state.conns.get(&c).map_err(|_| "SSH 连接不存在".to_string())?;
        if conn.kind != "ssh" {
            return Err("要选一条远程（SSH）连接".into());
        }
        site = state.sites.update(&id, crate::now_secs(), |s| {
            s.deploy.mode = "vps".into();
            s.deploy.ssh_conn_id = c.clone();
            s.deploy.command.clear();
            Ok(())
        })?;
    }
    if !managed(&site) || site.deploy.ssh_conn_id.is_empty() {
        return Err("先选好要部署到的服务器（SSH 连接）".into());
    }
    let job = state.site_jobs.start(Some(app), &id, "vps:setup", "配置服务器", SETUP_STEPS)?;
    let job_id = job.id.clone();
    let dir = state.sites.data_dir(&id);
    let conns = state.conns.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_setup(site, conns, dir, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

/// 旧站路径结尾的习惯（迁移站点从盘点结果里读，其它站点按「混合」处理）。
fn trailing_of(site_dir: &Path) -> String {
    std::fs::read(site_dir.join("migrate/inventory.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["trailing_slash"].as_str().map(String::from))
        .unwrap_or_else(|| "mixed".into())
}

/// 迁移站点：旧站实际用的主机名（www 还是不带）。
fn origin_host(site_dir: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(site_dir.join("migrate/inventory.json")).ok()?).ok()?;
    reqwest::Url::parse(v["origin"].as_str()?).ok()?.host_str().map(String::from)
}

async fn run_setup(site: Site, conns: crate::conns::ConnStore, dir: PathBuf, job: &crate::site_jobs::Job) -> Result<(), String> {
    let conn = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "SSH 连接不存在".to_string())?;
    let mut v = load(&dir).unwrap_or_default();
    v.ssh_conn_id = conn.id.clone();
    v.slug = slug_of(&site);
    v.root = format!("/var/www/pilot/{}", v.slug);
    v.host = if !site.domain.is_empty() { site.domain.clone() } else { origin_host(&dir).unwrap_or_else(|| site.migrate_from.clone()) };
    if v.host.is_empty() {
        return Err("站点还没有域名".into());
    }
    v.alias = alias_of(&v.host);
    v.trailing = trailing_of(&dir);
    let dynamic = crate::site_dynamic::load(&dir).unwrap_or_default();
    v.api_port = if dynamic.needs_db() { api_port(&v.slug) } else { 0 };
    if v.api_port > 0 && !has_pilot_site() {
        return Err("这个 Pilot 构建里没有带表单 / 评论服务（编译时没有 Go）".into());
    }
    let large_dir = dir.join("migrate/large");
    v.large = dynamic.has("files") && large_dir.is_dir();
    v.notes.clear();

    job.step("connect", "running", &format!("{}@{}", conn.ssh_user, conn.ssh_host));
    let user = conn.ssh_user.clone();
    let out = run_script(&conn, &format!("{PRELUDE}{DETECT_SCRIPT}"), 60, None).await.inspect_err(|_| job.step("connect", "failed", ""))?;
    v.os = tag(&out, "PILOT_OS");
    v.arch = tag(&out, "PILOT_ARCH");
    job.step("connect", "ok", &format!("{} · {}", v.os, v.arch));

    // 站点配置和 pilot-site 先传上去（setup 脚本里再用 sudo 放到位）
    job.step("upload", "running", "");
    run_script(&conn, &format!("{PRELUDE}$SUDO mkdir -p {r} && $SUDO chown {user} {r} && echo PILOT_OK\n", r = v.root), 60, None).await?;
    upload_bytes(&conn, &format!("{}/site.caddy.new", v.root), caddy_site(&v).as_bytes()).await?;
    if v.api_port > 0 {
        let bin = match v.arch.as_str() {
            "x86_64" | "amd64" => PILOT_SITE_AMD64,
            "aarch64" | "arm64" => PILOT_SITE_ARM64,
            a => return Err(format!("服务器的 CPU 架构（{a}）没有对应的表单 / 评论服务")),
        };
        upload_bytes(&conn, &format!("{}/pilot-site.gz", v.root), bin).await?;
    }
    job.step("upload", "ok", "");

    job.step("setup", "running", "第一次要装 Caddy，可能要一两分钟");
    let out = run_script(&conn, &setup_script(&v, &user), 900, Some(job)).await.inspect_err(|_| job.step("setup", "failed", ""))?;
    let ip = tag(&out, "PILOT_IP");
    v.ip = if ip.is_empty() && conn.ssh_host.parse::<std::net::IpAddr>().is_ok() { conn.ssh_host.clone() } else { ip };
    job.step("setup", "ok", &format!("Caddy · IP {}{}", v.ip, if v.https { " · HTTPS" } else { "" }));

    if v.api_port > 0 {
        if out.contains("PILOT_API ok") {
            job.step("api", "ok", &format!("端口 {}，数据存在服务器上的 SQLite", v.api_port));
        } else {
            v.notes.push(format!("表单和评论服务没起来：在服务器上看 journalctl -u {}", v.service()));
            job.step("api", "failed", "服务没起来");
        }
        // 收件箱走 SSH 读服务器上的 SQLite：标记成「已配置」
        let mut d = dynamic.clone();
        d.setup_at = crate::now_secs();
        let _ = crate::site_dynamic::save(&dir, &d);
    } else {
        job.step("api", "skipped", "没有表单和评论");
    }

    if v.large {
        job.step("large", "running", "");
        let files = walk_files(&large_dir);
        let dirs: BTreeSet<String> = files
            .iter()
            .filter_map(|f| f.strip_prefix(&large_dir).ok().and_then(|r| r.parent()).map(|p| format!("{}/large/{}", v.root, p.to_string_lossy().replace('\\', "/"))))
            .collect();
        let mk: String = dirs.iter().map(|d| format!("mkdir -p '{}'\n", d.replace('\'', ""))).collect();
        run_script(&conn, &mk, 60, None).await?;
        for (i, f) in files.iter().enumerate() {
            let rel = f.strip_prefix(&large_dir).unwrap_or(f).to_string_lossy().replace('\\', "/");
            job.step("large", "running", &format!("{} / {}  {rel}", i + 1, files.len()));
            upload_file(&conn, f, &format!("{}/large/{rel}", v.root)).await?;
        }
        job.step("large", "ok", &format!("{} 个", files.len()));
    } else {
        job.step("large", "skipped", "");
    }
    v.setup_at = crate::now_secs();
    save(&dir, &v)?;
    Ok(())
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else if p.is_file() {
            out.push(p);
        }
    }
    out
}

// ───────────────────────── 发布 / 回退 / HTTPS ─────────────────────────

/// 托管发布（site_deploy::deploy_vps 在没有发布命令时调用）：构建产物打包上传、原子切换、换跳转规则。
pub async fn publish(site: &Site, conns: &crate::conns::ConnStore, site_dir: &Path, out_dir: &Path, job: &crate::site_jobs::Job) -> Result<String, String> {
    let conn = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "部署用的 SSH 连接不存在".to_string())?;
    let mut v = load(site_dir).filter(|v| v.setup_at > 0).ok_or("服务器还没配置：先点「配置服务器」")?;
    let release = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let pkg = tmp.path().join("site.tar.gz");
    let (out_owned, pkg2) = (out_dir.to_path_buf(), pkg.clone());
    let files = tokio::task::spawn_blocking(move || pack(&out_owned, &pkg2)).await.map_err(|e| e.to_string())??;
    let redirects = std::fs::read_to_string(out_dir.join("_redirects")).map(|t| redirects_caddy(&t)).unwrap_or_else(|_| redirects_caddy(""));
    job.line(&format!("打包 {files} 个文件，上传到 {}@{}:{}", conn.ssh_user, conn.ssh_host, v.root));
    upload_file(&conn, &pkg, &format!("{}/upload-{release}.tar.gz", v.root)).await?;
    upload_bytes(&conn, &format!("{}/redirects.caddy.new", v.root), redirects.as_bytes()).await?;
    let out = run_script(&conn, &deploy_script(&v, &release), 600, Some(job)).await?;
    v.releases = tag(&out, "PILOT_RELEASES").split_whitespace().map(String::from).collect();
    v.current = release;
    save(site_dir, &v)?;
    Ok(if site.domain.is_empty() { String::new() } else { format!("https://{}", site.domain) })
}

/// 回退到上一个版本（或指定版本）。
#[tauri::command]
pub async fn site_vps_activate(state: tauri::State<'_, AppState>, id: String, release: Option<String>) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let mut v = load(&dir).ok_or("服务器还没配置")?;
    let conn = state.conns.get(&site.deploy.ssh_conn_id)?;
    let target = match release {
        Some(r) => r,
        None => {
            let i = v.releases.iter().position(|r| r == &v.current).unwrap_or(v.releases.len());
            v.releases.get(i.checked_sub(1).ok_or("没有更早的版本")?).cloned().ok_or("没有更早的版本")?
        }
    };
    if !target.chars().all(|c| c.is_ascii_digit()) {
        return Err("版本名不对".into());
    }
    run_script(&conn, &activate_script(&v, &target), 60, None).await?;
    v.current = target.clone();
    save(&dir, &v)?;
    Ok(target)
}

/// 打开 / 关掉自动 HTTPS（切换时打开，回退时关掉）。打开后等 Caddy 拿到证书。
pub async fn set_https(conn: &crate::conns::Connection, site_dir: &Path, on: bool, alias_too: bool, job: Option<&crate::site_jobs::Job>) -> Result<bool, String> {
    let mut v = load(site_dir).ok_or("服务器还没配置")?;
    v.https = on;
    v.alias_https = on && alias_too;
    upload_bytes(conn, &format!("{}/site.caddy.new", v.root), caddy_site(&v).as_bytes()).await?;
    run_script(conn, &reconf_script(&v), 120, job).await?;
    let mut got = !on;
    if on {
        let out = run_script(conn, &https_wait_script(&v.host), 180, None).await?;
        got = !out.contains("PILOT_HTTPS none");
        if got {
            v.https_at = crate::now_secs();
        }
    }
    save(site_dir, &v)?;
    Ok(got)
}

/// 域名还没指过来时，在服务器上带 Host 头把每个旧路径请求一遍。返回 (路径, 状态码, 跳转地址)。
pub async fn remote_check(conn: &crate::conns::Connection, v: &VpsState, paths: &[String]) -> Result<Vec<(String, u16, String)>, String> {
    let list = format!("/tmp/pilot-paths-{}.txt", uuid::Uuid::new_v4().simple());
    let body: String = paths.iter().filter(|p| p.starts_with('/') && !p.contains(['\n', '\r'])).map(|p| format!("{p}\n")).collect();
    upload_bytes(conn, &list, body.as_bytes()).await?;
    let out = run_script(conn, &format!("{}rm -f {list}\n", live_check_script(&v.host, &list)), 900, None).await?;
    Ok(parse_check(&out))
}

pub fn parse_check(out: &str) -> Vec<(String, u16, String)> {
    out.lines()
        .filter_map(|l| {
            let (p, rest) = l.split_once('\t')?;
            let mut it = rest.splitn(2, ' ');
            let code = it.next()?.trim().parse().ok()?;
            Some((p.to_string(), code, it.next().unwrap_or("").trim().to_string()))
        })
        .collect()
}

/// 收件箱：在服务器上用 pilot-site admin 执行 SQL（只读写站点自己的 SQLite）。
pub async fn query(conn: &crate::conns::Connection, v: &VpsState, sql: &str, params: &[serde_json::Value]) -> Result<Vec<serde_json::Value>, String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(serde_json::json!({ "sql": sql, "params": params }).to_string());
    // 用数据库文件属主的身份执行：root 打开的话 SQLite 的 -wal / -shm 文件会变成 root 的，服务就写不进去了
    let db = v.db_path();
    let cmd = format!(
        "{PRELUDE}OWNER=$(stat -c %U {db} 2>/dev/null || id -un)\nif [ \"$(id -un)\" = \"$OWNER\" ]; then RUN=\"\"; elif [ \"$(id -u)\" = 0 ]; then RUN=\"runuser -u $OWNER --\"; else RUN=\"sudo -n -u $OWNER\"; fi\n$RUN {PILOT_SITE_BIN} admin --db {db} {b64}\n"
    );
    let out = run_script(conn, &cmd, 60, None).await?;
    let json = out.lines().find(|l| l.trim_start().starts_with('[')).unwrap_or("[]");
    serde_json::from_str(json).map_err(|e| format!("读服务器上的数据失败：{e}"))
}

#[tauri::command]
pub fn site_vps_state(state: tauri::State<'_, AppState>, id: String) -> Result<Option<VpsState>, String> {
    state.sites.get(&id)?;
    Ok(load(&state.sites.data_dir(&id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vs() -> VpsState {
        VpsState { slug: "example.com".into(), root: "/var/www/pilot/example.com".into(), host: "example.com".into(), alias: "www.example.com".into(), trailing: "always".into(), ..Default::default() }
    }

    #[test]
    fn redirects_to_caddy() {
        let c = redirects_caddy("/old /new 301\n/feed/ /feed.xml 200\n/blog/* /posts/:splat 301\n/:year/:slug/ /p/:slug/ 302\n/ext https://other.com/x 301\n/old /dup 301\n# comment\n/a b 301\n");
        assert!(c.starts_with("# Cligc Pilot"));
        assert!(!c.contains("route"));
        assert!(c.contains("\tredir /old /new 301\n"), "{c}");
        assert!(!c.contains("/dup"));
        assert!(c.contains("\trewrite /feed/ /feed.xml\n"));
        assert!(c.contains("\t@r1 path_regexp r1 ^/blog/(.*)$\n\tredir @r1 /posts/{re.r1.1} 301\n"), "{c}");
        assert!(c.contains("\t@r2 path_regexp r2 ^/([^/]+)/([^/]+)/$\n\tredir @r2 /p/{re.r2.2}/ 302\n"), "{c}");
        assert!(c.contains("\tredir /ext https://other.com/x 301\n"));
        assert!(!c.contains("redir /a "));
    }

    #[test]
    fn caddy_config() {
        let mut v = vs();
        let c = caddy_site(&v);
        assert!(c.contains("http://example.com {"), "{c}");
        assert!(c.contains("root * /var/www/pilot/example.com/current"));
        assert!(c.contains("import /var/www/pilot/example.com/redirects.caddy"));
        assert!(c.contains("try_files {path} {path}.html\n"));
        assert!(c.contains("http://www.example.com {\n\tredir http://example.com{uri} 301\n}"));
        assert!(!c.contains("/api/"));
        v.api_port = 18123;
        v.large = true;
        v.trailing = "never".into();
        v.https = true;
        let c = caddy_site(&v);
        assert!(c.contains("\nexample.com {"), "{c}");
        assert!(c.contains("reverse_proxy 127.0.0.1:18123"));
        assert!(c.contains("try_files {path} {path}.html {path}/index.html"));
        assert!(c.contains("root * /var/www/pilot/example.com/large"));
        assert!(c.contains("http://www.example.com {\n\tredir https://example.com{uri} 301"));
        v.alias_https = true;
        assert!(caddy_site(&v).contains("\nwww.example.com {"));
        assert_eq!(c.matches('{').count(), c.matches('}').count());
    }

    #[test]
    fn scripts() {
        let mut v = vs();
        let s = setup_script(&v, "deploy");
        assert!(s.starts_with("set -eu"));
        assert!(s.contains("PILOT_ERR need_sudo"));
        assert!(s.contains("PILOT_ERR port_busy"));
        assert!(s.contains("$SUDO cp /var/www/pilot/example.com/site.caddy.new /etc/caddy/pilot/example.com.caddy"));
        assert!(s.contains("import /etc/caddy/pilot/*.caddy"));
        assert!(!s.contains("step api"));
        v.api_port = 18123;
        let s = setup_script(&v, "deploy");
        assert!(s.contains("ExecStart=/usr/local/bin/pilot-site serve --port 18123 --db /var/www/pilot/example.com/data/site.db"));
        let d = deploy_script(&v, "20261025120000");
        assert!(d.contains("tar -xzf \"$R/upload-20261025120000.tar.gz\""));
        assert!(d.contains("mv -Tf \"$R/current.tmp\" \"$R/current\""));
        assert!(d.contains("head -n -5"));
        assert!(live_check_script("example.com", "/tmp/p").contains("-H 'Host: example.com' \"http://127.0.0.1$p\""));
        assert_eq!(parse_check("/a\t200 \n/b\t301 http://example.com/c\nbad\n"), vec![("/a".into(), 200, "".into()), ("/b".into(), 301, "http://example.com/c".into())]);
    }

    #[test]
    fn naming() {
        assert_eq!(alias_of("example.com"), "www.example.com");
        assert_eq!(alias_of("www.example.com"), "example.com");
        assert_eq!(alias_of("blog.example.com"), "");
        let p = api_port("example.com");
        assert!((18000..19000).contains(&p));
        assert!(explain_err("x\nPILOT_ERR port_busy 80 nginx\n").unwrap().contains("80 端口被 nginx 占着"));
        assert!(has_pilot_site(), "本机编译时应该带上 pilot-site");
    }

    #[test]
    fn schema_matches_go_service() {
        // Go 小服务里的 schema.sql 要和 Functions 用的一致（两边接口、数据表相同）
        let go = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/pilot-site/schema.sql")).unwrap();
        assert_eq!(go, crate::site_dynamic::SCHEMA_SQL);
    }

    #[test]
    fn packs_build_output() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("dist");
        std::fs::create_dir_all(out.join("a")).unwrap();
        std::fs::write(out.join("index.html"), "x").unwrap();
        std::fs::write(out.join("a/index.html"), "y").unwrap();
        std::fs::write(out.join("_redirects"), "/a /b 301").unwrap();
        std::fs::write(out.join("_routes.json"), "{}").unwrap();
        let pkg = d.path().join("p.tar.gz");
        assert_eq!(pack(&out, &pkg).unwrap(), 2);
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&pkg).unwrap()));
        let names: Vec<String> = ar.entries().unwrap().map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"index.html".to_string()));
        assert!(names.contains(&"a/index.html".to_string()));
        assert!(!names.iter().any(|n| n.contains("_redirects") || n.contains("_routes")));
    }

    /// 手动跑：把生成的配置和脚本导出到 VPS_DUMP 目录，放进 Docker 里的 Ubuntu 实际执行。
    #[test]
    #[ignore]
    fn dump_for_container() {
        let dir = PathBuf::from(std::env::var("VPS_DUMP").expect("VPS_DUMP"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut v = vs();
        v.api_port = 18123;
        v.large = true;
        std::fs::write(dir.join("site.caddy.new"), caddy_site(&v)).unwrap();
        std::fs::write(dir.join("setup.sh"), setup_script(&v, "root")).unwrap();
        std::fs::write(dir.join("deploy.sh"), deploy_script(&v, "20261025120000")).unwrap();
        std::fs::write(dir.join("deploy2.sh"), deploy_script(&v, "20261025130000")).unwrap();
        std::fs::write(dir.join("activate.sh"), activate_script(&v, "20261025120000")).unwrap();
        std::fs::write(dir.join("check.sh"), live_check_script(&v.host, "/work/paths.txt")).unwrap();
        v.https = true;
        std::fs::write(dir.join("site-https.caddy"), caddy_site(&v)).unwrap();
        std::fs::write(dir.join("pilot-site.gz"), PILOT_SITE_ARM64).unwrap();
        std::fs::write(dir.join("pilot-site-amd64.gz"), PILOT_SITE_AMD64).unwrap();
        let out = dir.join("dist");
        std::fs::create_dir_all(out.join("a")).unwrap();
        std::fs::write(out.join("index.html"), "<h1>home v1</h1>").unwrap();
        std::fs::write(out.join("a/index.html"), "<h1>page a</h1>").unwrap();
        std::fs::write(out.join("b.html"), "<h1>page b</h1>").unwrap();
        std::fs::write(out.join("404.html"), "<h1>not found page</h1>").unwrap();
        std::fs::write(out.join("feed.xml"), "<rss></rss>").unwrap();
        std::fs::write(out.join("_redirects"), "/old /a/ 301\n/feed/ /feed.xml 200\n/blog/* /a/ 301\n/:y/:m/:slug /p/:slug 301\n").unwrap();
        pack(&out, &dir.join("upload-20261025120000.tar.gz")).unwrap();
        std::fs::write(out.join("index.html"), "<h1>home v2</h1>").unwrap();
        pack(&out, &dir.join("upload-20261025130000.tar.gz")).unwrap();
        std::fs::write(dir.join("redirects.caddy.new"), redirects_caddy(&std::fs::read_to_string(out.join("_redirects")).unwrap())).unwrap();
        std::fs::write(dir.join("paths.txt"), "/\n/a\n/a/\n/b\n/old\n/feed/\n/blog/x\n/2020/01/hello\n/nope\n/big/video.mp4\n").unwrap();
    }
}
