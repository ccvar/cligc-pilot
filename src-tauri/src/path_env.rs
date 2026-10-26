//! GUI 进程拿到的是登录时的 PATH 快照，找不到装完 CLI 后新增的目录（也可能缺代理变量）。
//! macOS：起一次交互登录 shell（-l -i，读 .zprofile + .zshrc），把 PATH + 代理变量注入本进程。
//! Windows：从注册表（User + Machine）读当前 PATH，再补 npm 全局 bin，合进本进程 PATH。
//! 之后 spawn 的所有 CLI 子进程都继承。

pub fn fix() {
    #[cfg(target_os = "macos")]
    fix_macos();
    #[cfg(target_os = "windows")]
    fix_windows();
}

// ---------------- macOS ----------------

#[cfg(target_os = "macos")]
const MARK: &str = "__CLIGC_PILOT_ENV__";

/// 白名单导入：PATH + 代理相关。不整包导入，避免把 shell 私货带进 GUI 进程。
#[cfg(target_os = "macos")]
const IMPORT_KEYS: &[&str] = &[
    "PATH",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

#[cfg(target_os = "macos")]
fn fix_macos() {
    if let Some(pairs) = login_shell_env() {
        for (k, v) in pairs {
            if IMPORT_KEYS.contains(&k.as_str()) && !v.trim().is_empty() {
                std::env::set_var(&k, v.trim());
            }
        }
    }
}

/// 登录 shell 最多等这么久：.zshrc 里有东西卡住（等输入、连网、nvm 慢）时，不能让应用连窗口都不出。
/// 超时就杀掉它，PATH 用进程继承的那份凑合（最多是找不到新装的 CLI，再探测时还有托管 Node 兜底）。
#[cfg(target_os = "macos")]
const SHELL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(target_os = "macos")]
fn login_shell_env() -> Option<Vec<(String, String)>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let cmd = format!("printf '%s\\n' \"{MARK}\"; env; printf '%s\\n' \"{MARK}\"");
    let mut child = std::process::Command::new(&shell)
        .args(["-l", "-i", "-c", &cmd])
        .stdin(std::process::Stdio::null()) // 交互 shell 别等人敲键
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // 读 stdout 放到另一个线程、用带超时的 channel 收结果：主线程最多等 SHELL_TIMEOUT，
    // 到点就杀 shell；读线程在管道关掉后自己退出（不 join，免得被 shell 遗留的后台进程拖住）。
    let mut pipe = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let raw = match rx.recv_timeout(SHELL_TIMEOUT) {
        Ok(raw) => raw,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("[pilot] 登录 shell {SHELL_TIMEOUT:?} 内没退出，已放弃导入其 PATH/代理，沿用进程自带的环境");
            return None;
        }
    };
    // 输出已经收完，shell 通常也已退出；万一还赖着（关了 stdout 却没退），稍等一下就杀，不无限等。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill();
    }
    let _ = child.wait();
    let stdout = String::from_utf8_lossy(&raw);
    // shell rc 文件可能自带输出，只取两个标记行之间的内容。
    let start = stdout.find(MARK)? + MARK.len();
    let end = start + stdout[start..].rfind(MARK)?;
    let mut pairs = Vec::new();
    for line in stdout[start..end].lines() {
        if let Some((k, v)) = line.split_once('=') {
            if !k.is_empty() {
                pairs.push((k.to_string(), v.to_string()));
            }
        }
    }
    Some(pairs)
}

// ---------------- Windows ----------------

#[cfg(target_os = "windows")]
fn fix_windows() {
    let mut dirs: Vec<String> = Vec::new();

    // 1) 注册表里的当前 PATH（User + Machine）——反映刚装的 CLI，GUI 进程的快照里没有。
    use std::os::windows::process::CommandExt;
    if let Ok(out) = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine')",
        ])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW：不弹控制台窗口
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for p in s.split(';') {
                let p = p.trim();
                if !p.is_empty() {
                    dirs.push(p.to_string());
                }
            }
        }
    }

    // 2) npm 全局 bin（claude.cmd / codex.cmd 常在这），注册表里未必收录。
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(format!("{appdata}\\npm"));
    }
    // 3) Claude 官方 Windows 安装器的默认目录。必须在应用启动阶段就补，而不是依赖
    // 前端先跑一次 detect；这样定时任务/恢复会话也能直接解析到原生 claude.exe。
    if let Ok(profile) = std::env::var("USERPROFILE") {
        dirs.push(format!("{profile}\\.local\\bin"));
    }

    if dirs.is_empty() {
        return;
    }

    // 合进现有 PATH：新目录在前、原有在后，按小写去重（Windows 路径大小写不敏感）。
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged: Vec<String> = Vec::new();
    for d in dirs
        .into_iter()
        .chain(existing.split(';').map(|s| s.to_string()))
    {
        let d = d.trim().to_string();
        if d.is_empty() {
            continue;
        }
        if seen.insert(d.to_lowercase()) {
            merged.push(d);
        }
    }
    std::env::set_var("PATH", merged.join(";"));
}
