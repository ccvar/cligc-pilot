# Cligc Pilot

**本地 AI 工作台**：一个桌面客户端（macOS / Windows），让 AI 帮你做网站、上线、看数据、持续运营，也能管理你的服务器。

AI 用你电脑上已经登录的 **Claude Code / Codex / Grok**，或者你自己填的 **API Key**（Anthropic、OpenAI 及兼容接口）。所有账号密钥只存在系统钥匙串里，不经过任何第三方服务器。

[下载最新版](https://github.com/ccvar/cligc-pilot/releases/latest) · [发版说明](RELEASE.md) · [许可协议](LICENSE.md)

---

## 能做什么

### 网站：从零做到持续运营

- **制作站点**：一句话说清是个什么站，AI 自己定类型、语言、栏目和技术栈，直接在对话里搭出第一版；想自己把关也可以用「详细设置」逐项选。内置写作和视觉标准，可以加自己的补充要求。
- **导入已有站点**：选本地文件夹或 GitHub 仓库（可一次选多个），自动识别框架、域名和部署方式。
- **改版已有站点**：给一个线上域名，盘点 → 抓取内容 → AI 用新设计重做 → 逐页核对 → 切换域名。保留原来的网址和内容，切换前记下 DNS，可一键回退。
- **上线**：部署到 Cloudflare Pages / Workers，或者你自己的服务器（Caddy + Go + SQLite，Pilot 帮你配好）。上线向导一步步接好 GA4、Search Console、Bing 和 IndexNow，能自动处理的会列出来，你确认后才做。
- **看数据**：把 GA4、Search Console、Bing 的数据汇总到一个看板，按查询词和页面找出运营机会。
- **托管运营**：给站点定一个方案（做哪些事、多大自主度、每周额度），AI 定期出计划、写新页面、改标题描述、修技术问题。每条改动单独提交，可以回滚。
- **表单和评论**：给静态站加联系表单、评论，数据存在你自己的 Cloudflare 账号里，在 Pilot 里收件和审核。
- **分发**：把文章同步发到 Blogger、WordPress.com。

### 本地对话

不连接任何服务也能用：选一个本地文件夹，让 AI 在里面读写文件、跑命令。首次打开即可用。

### 远程服务器（SSH）

终端、文件管理（上传下载、在 Finder 里拖进拖出），以及让 AI 在这台机器上执行命令。每条命令按你选的权限档位确认，服务器密码永远不交给 AI。

### 手机和邮箱验证码

接上一台安卓手机，AI 可以帮你在 App 里完成注册：填邮箱、点发送、自动去你的邮箱取验证码填回去。居住地、密码、同意协议、最终确认这些步骤留给你自己点。

### 其他

- **权限档位**：每个对话可选「计划 / 询问 / 自动 / 全自动」。危险命令（删除、推送、上线、改 DNS 等）和会被执行的文件改动，在「自动」档也会弹卡片让你确认。
- **分屏**：把侧栏里的对话拖出来分屏，多个对话同时跑。
- **模板与提示词库**：做好的项目存成模板，常用的话存成提示词。
- **后台运行**：关掉窗口会缩到托盘，对话继续跑，完成时发系统通知。
- **自动升级**：设置 → 关于 → 检查更新，更新包验签后安装。

---

## 安装

到 [Releases](https://github.com/ccvar/cligc-pilot/releases/latest) 下载：

| 系统 | 文件 | 要求 |
| --- | --- | --- |
| macOS | `Cligc-Pilot_x.y.z_aarch64.dmg` | macOS 11 及以上，Apple 芯片（M 系列） |
| Windows | `Cligc-Pilot_x.y.z_x64-setup.exe` | Windows 10 / 11，64 位 |

**macOS 第一次打开**：应用还没有经过 Apple 公证，第一次打开会提示「已损坏，无法打开」（并不是真的损坏）。把应用拖进「应用程序」后，在终端执行一次：

```bash
xattr -cr "/Applications/Cligc Pilot.app"
```

然后正常双击打开。以后通过「检查更新」升级不用再做这一步。

**用之前**：至少准备一种 AI，二选一即可。

- 在电脑上装好并登录 Claude Code、Codex 或 Grok 的命令行客户端。Pilot 会自动检测，没登录可以一键去授权；缺 Node.js 时也能一键装好。
- 或者在「设置 → API 服务」里填自己的 API Key。API 服务由对应厂商单独计费。

---

## 安全与隐私

- 所有密钥和令牌（Cloudflare、GitHub、Google、SSH 密码、API Key）只存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器）里，不写进文件，不交给 AI。
- SSH 首次连接先让你核对主机指纹，核对后才发送密码；指纹变了会直接拒绝连接。
- 删除、下线、改 DNS、推送到默认分支等不可撤销的操作都要你确认。无人值守的托管任务不会自动做需要确认的事。
- Pilot 没有自己的服务器，你的数据只在你的电脑和你自己的账号里。

---

## 从源码构建

依赖：Rust ≥ 1.88、Node.js ≥ 20、Go ≥ 1.23（编译随附的站点小服务）。

```bash
npm install
npm run tauri dev          # 开发版
```

```bash
npm run check                      # 前端类型检查
cd src-tauri && cargo test         # Rust 单元测试
node --test src/lib/*.test.mjs     # 前端单元测试（*Browser 那几个需要另装 playwright）
```

本地打包：`scripts/package-macos.sh`（.app + .dmg）、`scripts/package-windows.cmd`（NSIS 安装器）。正式发版走 GitHub Actions，见 [RELEASE.md](RELEASE.md)。

代码结构：

- `src-tauri/src/`：Rust 后端，包括连接与钥匙串、AI 执行器（本地客户端与 API）、站点（制作、导入、改版、部署、数据、托管运营）、SSH / SFTP。
- `src/lib/state/`：前端按领域拆分的状态与逻辑（Svelte 5 runes）。
- `src/lib/components/`：界面组件。

---

## 许可协议

本项目源码公开，采用 **[PolyForm Noncommercial 1.0.0](LICENSE.md)** 协议：

- ✅ 个人学习、研究、自用、爱好项目，以及学校、公益组织、政府机构等非营利用途，都可以免费使用、修改和分发。
- ❌ **不允许商业用途**，包括把它（或改过的版本）作为商业产品或服务提供、在公司里用于营利业务、二次打包售卖等。

分发时需要保留 LICENSE.md 里的版权声明。需要商业授权请到 [Issues](https://github.com/ccvar/cligc-pilot/issues) 联系。

注意：这是「源码公开」协议，不属于 OSI 定义的开源协议。
