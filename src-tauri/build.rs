use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    build_pilot_site();
    tauri_build::build()
}

/// 站点小服务 pilot-site（Go，SQLite 纯 Go 驱动）：编译成 Linux amd64 / arm64，gzip 后由 site_vps_host 打包进应用，
/// VPS 托管发布时上传到服务器。本机没有 Go 时放空文件并给出警告（这台机器编出来的 Pilot 不能给 VPS 站点开表单和评论）。
fn build_pilot_site() {
    let src = Path::new("pilot-site");
    for f in ["main.go", "go.mod", "go.sum", "schema.sql"] {
        println!("cargo:rerun-if-changed=pilot-site/{f}");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let go = std::env::var("GO").unwrap_or_else(|_| "go".into());
    let have_go = Command::new(&go).arg("version").output().map(|o| o.status.success()).unwrap_or(false);
    for arch in ["amd64", "arm64"] {
        let bin = out.join(format!("pilot-site-linux-{arch}"));
        let gz = out.join(format!("pilot-site-linux-{arch}.gz"));
        let ok = have_go
            && Command::new(&go)
                .current_dir(src)
                .env("CGO_ENABLED", "0")
                .env("GOOS", "linux")
                .env("GOARCH", arch)
                .args(["build", "-trimpath", "-ldflags", "-s -w", "-o"])
                .arg(&bin)
                .arg(".")
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        if ok {
            let data = std::fs::read(&bin).unwrap();
            let mut e = flate2::write::GzEncoder::new(std::fs::File::create(&gz).unwrap(), flate2::Compression::best());
            std::io::Write::write_all(&mut e, &data).unwrap();
            e.finish().unwrap();
        } else {
            println!("cargo:warning=没有编译出 pilot-site（linux/{arch}）：本机需要 Go 1.23+。VPS 站点的表单和评论在这个构建里不可用");
            std::fs::write(&gz, b"").unwrap();
        }
    }
}
