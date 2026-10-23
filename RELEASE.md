# Cligc Pilot —— GitHub 打包 + 在线自动升级

打 tag → CI 打包 → 发布到本仓库的 Releases，外加桌面端自动升级（Tauri updater，ed25519 验签）。
当前做了 **ad-hoc 签名**（`bundle.macOS.signingIdentity = "-"`，免 Apple 证书）但**未公证**——
首次从网上下载安装仍需手动放行一次（见下方「用户侧」）。

## 机制

- **打包**：推 `v*` tag → `.github/workflows/release.yml` 分别构建 macOS arm64 和 Windows x64；
  macOS 产出 `.dmg` 与签名更新包，Windows 产出签名 NSIS 安装器。两个平台完成后合并 `latest.json`，
  发布到 `ccvar/cligc-pilot` 的 Releases（工作流用内置 `GITHUB_TOKEN`，权限 `contents: write`）。
- **升级**：app 里「连接与模型 → 关于 → 检查更新」调用 Tauri updater，拉
  `https://github.com/ccvar/cligc-pilot/releases/latest/download/latest.json`，比版本 → ed25519 验签 →
  下载安装 → 重启。公钥已内置在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。

> 仓库是公开仓：源码与 Releases 都对外可见。

## 一次性配置（只做一次）

在仓库 Settings → Secrets and variables → Actions 加一个 Secret：

- `TAURI_SIGNING_PRIVATE_KEY` = 更新签名**私钥**内容。私钥已在维护者本机生成于
  `~/.tauri/cligc-pilot-updater.key`（无密码，所以 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空）。

> ⚠️ 私钥是唯一的签名凭据，丢了就没法再签更新包（老用户升不了级）。放进 GitHub Secret 后，
> 本机那份请另存到安全的地方（如密码管理器）备份。它与旧版 gcms Pilot 的签名密钥无关，不可混用。

## 每次发版

```bash
# 1. 改版本号（三处要一致）
#    src-tauri/tauri.conf.json  -> "version"
#    package.json               -> "version"
#    package-lock.json          -> 根版本及 packages[""].version
# 2. 提交后打 tag（v 前缀）
git tag v0.1.0
git push origin v0.1.0
```

CI 跑完后，Releases 里会出一个 `v0.1.0`，带 `.dmg` / `.app.tar.gz` / `-setup.exe` / `latest.json`。
老用户点「检查更新」即可升级。

## 用户侧（首次安装：绕过「已损坏」）

未公证的 app 从网上下载后会被打上 `com.apple.quarantine` 隔离属性，Apple Silicon 上 Gatekeeper 会误报
**「"Cligc Pilot" 已损坏，无法打开」**（并非真的损坏）。下载者首次需手动放行一次，三选一：

- **最稳（终端一行）**：把 app 拖进「应用程序」后执行，然后正常双击打开：
  ```bash
  xattr -cr "/Applications/Cligc Pilot.app"
  ```
- **系统设置放行**（macOS 15+）：双击被拦 → 系统设置 → 隐私与安全性 → 底部「仍要打开」。
- **右键打开**（macOS 14 及更早）：Finder 里右键 app →「打开」→ 再点「打开」。

之后升级由 app 内「检查更新」自动完成（updater 装的包不带隔离属性），无需再放行。

## 以后要「双击即开 + 静默」

补 Apple 代码签名 + 公证：Apple Developer 账号 → Developer ID 证书 → 把 `APPLE_CERTIFICATE` /
`APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`
加成 Secret，`tauri build` 会自动签名公证。这与上面的 ed25519 更新签名是两套，互不替代。
