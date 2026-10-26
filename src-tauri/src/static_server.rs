//! 内建的本地静态预览服务器。
//!
//! **为什么不一律用 wrangler**：内置模板是我们自己钉死的「单文件、内联 CSS、零外部资源」的静态
//! HTML —— 为了看一个静态文件去装 wrangler（要 Node、要一大包、冷启十几秒），在 Windows 上
//! 直接把人挡在门外（用户原话：「一定要安装 wrangler 吗」）。**不需要。**
//!
//! 真正非 wrangler 不可的，只有用到 Workers 运行时/Pages 特性的项目 —— 见 `needs_wrangler()`。
//! 那种情况下**故意不降级**到这个服务器：静态服务器不认 `functions/`、`_headers`，
//! 硬用它预览会给出一个「能打开、但和线上不一样」的假象，比明说「要装 wrangler」更坏。
//! `_redirects` 里的固定路径规则（`/a /b 301`、`/feed/ /feed.xml 200`）照 Pages 的规矩实现了：
//! 迁移来的站点首页常常就是一条跳转，不认它连首页都打不开。带 `*` / `:占位` 的规则不认。

use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 这个目录**真的**需要 wrangler 吗？
///
/// 只认「静态服务器给不出正确结果」的那几样：
/// - `functions/` / `_worker.js`：要 Workers 运行时。
/// - `wrangler.*`：项目自己声明了配置（可能有 bindings/compat date）。
/// - `_headers` / `_redirects`：Pages 特性，本服务器不实现 —— 与其静默失真，不如交给 wrangler。
pub fn needs_wrangler(dir: &Path) -> bool {
    [
        "functions",
        "_worker.js",
        "_headers",
        "_redirects",
        "wrangler.toml",
        "wrangler.json",
        "wrangler.jsonc",
    ]
    .iter()
    .any(|f| dir.join(f).exists())
}

fn mime(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "xml" => "application/xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// `%20` 这类转义还原。文件名带空格/中文时必须有（用户存的模板什么名字都可能）。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            };
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 把请求路径解析成 root 下的真实文件。
///
/// ★ **路径穿越必须挡死**：这个服务器听在 127.0.0.1 上，但预览的是 AI 生成/用户存下来的站点，
/// 请求路径不可信。两道闸：先按段拒掉 `..`，再把最终路径 canonicalize 后核一遍还在不在 root 里
/// （只做第一道不够 —— 符号链接照样能把你带出去）。
fn resolve(root: &Path, req_path: &str) -> Option<PathBuf> {
    let path = req_path.split(['?', '#']).next()?;
    let decoded = percent_decode(path);
    let mut p = root.to_path_buf();
    for seg in decoded.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        // `..` 上跳、以及混进来的反斜杠（Windows 上会被当分隔符）一律拒
        if seg == ".." || seg.contains('\\') {
            return None;
        }
        p.push(seg);
    }
    if p.is_dir() {
        p.push("index.html");
    } else if !p.exists() {
        // `/a` → `a.html`（和 Cloudflare Pages 一样）：Astro 的 build.format 'file' / 'preserve' 产物就是这样
        let mut html = p.clone().into_os_string();
        html.push(".html");
        if Path::new(&html).is_file() {
            p = html.into();
        }
    }
    let real = p.canonicalize().ok()?;
    let base = root.canonicalize().ok()?;
    if !real.starts_with(&base) {
        return None; // 软链接指到 root 外面去了
    }
    if !real.is_file() {
        return None;
    }
    Some(real)
}

async fn write_resp(
    sock: &mut TcpStream,
    status: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    // Cache-Control: no-store —— 预览就是要立刻看到改动，缓存住等于骗人。
    // Connection: close —— 不做 keep-alive，本地预览不值当为它加复杂度。
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    sock.write_all(head.as_bytes()).await?;
    sock.write_all(body).await?;
    sock.flush().await
}

async fn handle(root: &Path, sock: &mut TcpStream) -> std::io::Result<()> {
    // 只读到请求头结束就够：静态服务器不需要 body。给个上限，别让畸形请求把内存撑爆。
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" && method != "HEAD" {
        return write_resp(
            sock,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"405",
        )
        .await;
    }
    let path = target.split(['?', '#']).next().unwrap_or("/");
    // Pages 的规矩：_redirects 先于静态文件
    let mut target = target.to_string();
    match redirect_rule(root, path) {
        Some((code, to)) if code != 200 => {
            let status = match code {
                302 => "302 Found",
                303 => "303 See Other",
                307 => "307 Temporary Redirect",
                308 => "308 Permanent Redirect",
                _ => "301 Moved Permanently",
            };
            let head = format!("HTTP/1.1 {status}\r\nLocation: {to}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
            sock.write_all(head.as_bytes()).await?;
            return sock.flush().await;
        }
        Some((_, to)) => target = to,
        None => {}
    }
    match resolve(root, &target) {
        Some(f) => {
            let body = tokio::fs::read(&f).await.unwrap_or_default();
            let ct = mime(&f);
            if method == "HEAD" {
                return write_resp(sock, "200 OK", ct, &[]).await;
            }
            write_resp(sock, "200 OK", ct, &body).await
        }
        None => {
            // 站点自带 404.html 就用它（和 Cloudflare Pages 的行为一致）
            let custom = root.join("404.html");
            if custom.is_file() {
                let body = tokio::fs::read(&custom).await.unwrap_or_default();
                return write_resp(sock, "404 Not Found", "text/html; charset=utf-8", &body).await;
            }
            write_resp(
                sock,
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"404 Not Found",
            )
            .await
        }
    }
}

/// `_redirects` 里匹配 `path` 的固定规则：(状态码, 目标)。没写状态码是 301；200 是改写（地址栏不变）。
/// 结尾斜杠宽松匹配（`/a` 和 `/a/` 算同一条），和 Pages 一样。每次请求现读：文件很小，改了立刻生效。
fn redirect_rule(root: &Path, path: &str) -> Option<(u16, String)> {
    let raw = std::fs::read_to_string(root.join("_redirects")).ok()?;
    let norm = |p: &str| p.trim_end_matches('/').to_string();
    let want = norm(&percent_decode(path));
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(from), Some(to)) = (it.next(), it.next()) else { continue };
        if from.contains('*') || from.contains(':') || to.contains(':') && !to.starts_with("http") {
            continue;
        }
        if norm(from) != want {
            continue;
        }
        let code = it.next().and_then(|c| c.trim_end_matches('!').parse().ok()).unwrap_or(301);
        return Some((code, to.to_string()));
    }
    None
}

/// 构建产物在不在：有 index.html，或者首页本身是 `_redirects` 里的一条跳转（迁移来的多语言站常见）。
/// 返回修改时间（秒），没有就是 None。
pub fn built_at(out: &Path) -> Option<u64> {
    let marker = if out.join("index.html").is_file() {
        out.join("index.html")
    } else if redirect_rule(out, "/").is_some() {
        out.join("_redirects")
    } else {
        return None;
    };
    let t = std::fs::metadata(marker).and_then(|m| m.modified()).ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs())
}

/// 在**已经绑好的** listener 上服务 `root`。
///
/// 为什么让调用方先 bind：绑定成功与否必须在开预览窗**之前**就知道 —— 端口被占要能立刻换一个，
/// 而不是等 25 秒超时再说（wrangler 那条路就是这么坑的）。
pub async fn serve(root: PathBuf, listener: TcpListener) {
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            continue;
        };
        let root = root.clone();
        // tokio::spawn 而不是 tauri::async_runtime::spawn：这里本来就在 tokio 任务里
        // （Tauri 的异步运行时就是 tokio），用它才能在 #[tokio::test] 里原样跑起来。
        tokio::spawn(async move {
            let _ = handle(&root, &mut sock).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_wrangler_only_for_real_workers_features() {
        let base = std::env::temp_dir().join(format!("nw-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("index.html"), "<h1>hi</h1>").unwrap();
        // 纯静态站：不该逼人装 wrangler
        assert!(
            !needs_wrangler(&base),
            "纯静态站不需要 wrangler —— 这正是用户问的那个点"
        );

        for f in ["_headers", "_redirects", "_worker.js", "wrangler.toml"] {
            let d = base.join(f.replace('.', "-"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("index.html"), "x").unwrap();
            std::fs::write(d.join(f), "x").unwrap();
            assert!(
                needs_wrangler(&d),
                "{f}: 静态服务器给不出正确结果，必须交给 wrangler"
            );
        }
        // functions/ 是目录
        let d = base.join("fn-dir");
        std::fs::create_dir_all(d.join("functions")).unwrap();
        assert!(needs_wrangler(&d), "functions/ 要 Workers 运行时");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn resolve_serves_files_and_blocks_traversal() {
        let base = std::env::temp_dir().join(format!("rs-{}", uuid::Uuid::new_v4()));
        let root = base.join("site");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("index.html"), "home").unwrap();
        std::fs::write(root.join("sub").join("index.html"), "sub-home").unwrap();
        std::fs::write(root.join("a b.css"), "css").unwrap();
        std::fs::write(base.join("secret.txt"), "绝不能被读到").unwrap();

        assert!(resolve(&root, "/").is_some(), "/ → index.html");
        assert!(resolve(&root, "/index.html").is_some());
        assert!(
            resolve(&root, "/sub/").is_some(),
            "目录 → 里面的 index.html"
        );
        assert!(
            resolve(&root, "/index.html?v=1#x").is_some(),
            "查询串和锚点要剥掉"
        );
        assert!(
            resolve(&root, "/a%20b.css").is_some(),
            "%20 要还原（用户存的模板文件名什么样都有）"
        );
        assert!(resolve(&root, "/nope.html").is_none());
        std::fs::write(root.join("about.html"), "about").unwrap();
        assert!(built_at(&root).is_some());
        assert!(resolve(&root, "/about").is_some_and(|p| p.ends_with("about.html")), "/about → about.html");

        // ★ 路径穿越
        assert!(resolve(&root, "/../secret.txt").is_none(), "..");
        assert!(
            resolve(&root, "/sub/../../secret.txt").is_none(),
            "绕一圈的 .."
        );
        assert!(
            resolve(&root, "/%2e%2e/secret.txt").is_none(),
            "编码过的 .. 也得挡（先解码再判段）"
        );
        assert!(resolve(&root, "/..%2fsecret.txt").is_none(), "编码的分隔符");

        std::fs::remove_dir_all(&base).ok();
    }

    /// ★ 端到端：真起服务器、真发 HTTP 请求、真读回来。
    /// 前面那些测的是「决策」和「路径解析」——都通过了也可能整个服务器是哑的。
    /// 这条同时是「**预览不需要 wrangler**」的实证：全程没有任何外部进程。
    #[tokio::test]
    async fn serves_a_real_template_over_http_without_wrangler() {
        let root = std::env::temp_dir().join(format!("e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("index.html"), "<h1>模板首页</h1>").unwrap();
        std::fs::write(root.join("sub").join("index.html"), "sub").unwrap();
        std::fs::write(root.join("a.css"), "body{color:red}").unwrap();
        std::fs::write(std::env::temp_dir().join("e2e-secret.txt"), "不能被读到").unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(serve(root.clone(), listener));

        async fn get(port: u16, path: &str) -> String {
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = Vec::new();
            s.read_to_end(&mut out).await.unwrap();
            String::from_utf8_lossy(&out).into_owned()
        }

        let r = get(port, "/").await;
        assert!(
            r.starts_with("HTTP/1.1 200 OK"),
            "根路径该给 index.html: {}",
            &r[..r.len().min(60)]
        );
        assert!(r.contains("<h1>模板首页</h1>"), "内容要对");
        assert!(
            r.contains("text/html; charset=utf-8"),
            "中文要靠这个 charset 才不乱码"
        );
        assert!(
            r.contains("Cache-Control: no-store"),
            "预览必须立刻反映改动，不许缓存"
        );

        std::fs::write(root.join("_redirects"), "/old /sub/ 301\n/feed/ /a.css 200\n").unwrap();
        let r = get(port, "/old?x=1").await;
        assert!(r.starts_with("HTTP/1.1 301") && r.contains("Location: /sub/"), "_redirects 的跳转: {r}");
        let r = get(port, "/feed/").await;
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("color:red"), "200 是改写: {r}");

        assert!(
            get(port, "/a.css").await.contains("text/css"),
            "CSS 的 content-type"
        );
        assert!(
            get(port, "/sub/").await.contains("sub"),
            "子目录 → 里面的 index.html"
        );
        assert!(
            get(port, "/nope").await.starts_with("HTTP/1.1 404"),
            "不存在 → 404"
        );

        // ★ 路径穿越：真发请求，不只是单测 resolve()
        let esc = get(port, "/../e2e-secret.txt").await;
        assert!(
            esc.starts_with("HTTP/1.1 404"),
            "穿越必须挡死，绝不能把 root 外的文件吐出去"
        );
        assert!(!esc.contains("不能被读到"), "内容一个字都不许漏");

        task.abort();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mime_covers_what_templates_actually_use() {
        assert_eq!(mime(Path::new("a/index.html")), "text/html; charset=utf-8");
        assert_eq!(
            mime(Path::new("a/x.SVG")),
            "image/svg+xml",
            "扩展名大小写不敏感"
        );
        assert_eq!(mime(Path::new("a/x.css")), "text/css; charset=utf-8");
        assert_eq!(mime(Path::new("a/x.woff2")), "font/woff2");
        assert_eq!(mime(Path::new("a/unknown")), "application/octet-stream");
    }

    #[test]
    fn redirects_file_rules_follow_pages() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        assert!(built_at(root).is_none(), "空目录不算构建过");
        std::fs::write(root.join("_redirects"), "# 注释\n/ /es-ar/ 301\n/old /new\n/feed/ /feed.xml 200\n/blog/* /posts/:splat 301\n/tmp /t 302!\n").unwrap();
        assert_eq!(redirect_rule(root, "/"), Some((301, "/es-ar/".into())));
        assert_eq!(redirect_rule(root, "/old/"), Some((301, "/new".into())), "没写状态码是 301，结尾斜杠宽松匹配");
        assert_eq!(redirect_rule(root, "/feed"), Some((200, "/feed.xml".into())));
        assert_eq!(redirect_rule(root, "/tmp"), Some((302, "/t".into())));
        assert_eq!(redirect_rule(root, "/blog/x"), None, "带通配的不认");
        assert!(built_at(root).is_some(), "首页是跳转也算构建过");
    }
}
