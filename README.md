# Cligc Pilot

本地 AI 工作台 —— 桌面客户端（Tauri 2 + Svelte 5，macOS / Windows）。用本地 Claude Code / Codex / Grok
客户端，或自备 API Key 的模型服务，在三类工作区里干活：

- **本地对话**：不连接任何服务，可接入一个本地文件夹。首次打开即可用。
- **Cloudflare**：连接一个 Cloudflare 账号，边聊边建站，本地预览后部署到 Pages / Workers；
  可把做好的项目存成模板复用。
- **远程连接（SSH）**：终端、SFTP 文件管理与传输，以及让模型在这台机器上执行命令的 AI 桥。

API 服务由对应供应商独立计费，不使用本地客户端的订阅额度。

> 本项目从 gcms Pilot 0.2.64 拆分而来，去掉了全部 gcms 能力（技能包、站点、托管、定时任务、
> gcms 服务器安装与迁移等），保留界面设计与上面三类工作区。新旧客户端可以同时安装：
> 标识（`com.cligc.pilot`）、数据目录、钥匙串条目、升级地址与签名密钥都是独立的。

## 功能

- **模型服务 · 本地客户端**：自动检测 claude / codex / grok CLI 的安装与登录状态；未登录一键「去授权」
  （打开终端跑官方登录命令，自动带上系统代理）。缺 Node 时可一键自举便携版 Node。
- **权限档位**：每个对话选「计划 / 询问 / 自动 / 全自动」，映射到各 CLI 的权限参数，并经 PreToolUse
  钩子把需要确认的操作回传界面批准。本地对话默认全自动；Cloudflare / SSH 会话默认降为自动。
- **API 服务**：设置 → API 服务 → 添加。支持 OpenAI Responses、Anthropic Messages 和
  OpenAI 兼容 Chat Completions，可配置多个独立账号、读取或手填模型 ID。密钥只存系统凭据库。
- **API 用量与账单**：展开 API 连接下的「用量与账单」，单独配置查询凭据后手动刷新（官方 OpenAI / Claude）。
- **Cloudflare 建站**：API Token 只进钥匙串，运行时以 `CLOUDFLARE_API_TOKEN` 注入 wrangler；
  内置起始模板与建站设计规范，静态项目无需 wrangler 即可本地预览。
- **远程连接**：密码 / 私钥口令只在内存与钥匙串里流转，绝不进模型子进程；主机指纹 TOFU 校验。
  AI 桥让模型通过随附脚本把命令交给 Pilot 代为执行，每条命令按档位确认。
- **远程文件传输**：上传、下载、取消与最近 100 条传输记录；macOS 支持与 Finder 双向拖放。
- **分屏对话**：拖动侧栏对话即可分屏，多个对话并行运行互不抢焦点。
- **自动升级**：设置 → 关于 → 检查更新。更新包 ed25519 验签后安装并重启（见 [RELEASE.md](RELEASE.md)）。
- 关窗隐藏到托盘，后台对话继续跑；完成时系统通知。

## 开发

```bash
npm install
npm run tauri dev
```

依赖：Rust ≥ 1.88、Node ≥ 20；运行模型时需要至少一个已登录的本地客户端或已配置的 API 服务。

```bash
npm run check                                   # svelte-check
cd src-tauri && cargo test                      # Rust 单元测试
node --test src/lib/*.test.mjs                  # 前端单元测试（*Browser / editMenu 需另装 playwright）
```

### 前端结构

- `src/routes/+page.svelte`：应用外壳（标题栏、主区视图切换、根层弹窗）与全局样式；按原顺序调用各模块的 `init*()` 注册副作用。
- `src/lib/state/`：按领域拆分的 rune store 与逻辑（`app`、`connections`、`panes`、`turns`、`composer`、`launcher`、`sftp`、`workbench`……）。
  共享状态是类上的 `$state` / `$derived` 字段，组件直接读写 `app.conns`、`launcher.lPerm` 等，不层层传 props。
- `src/lib/components/`：侧栏、启动页、对话视图、远程工作台、设置、更新中心、各类弹窗与图标。
- 样式是各组件的作用域样式：祖先或运行时类不在本组件里的选择器要把那一段包进 `:global(...)`
  （如 `:global(.app.rail-collapsed) .thread-head`、`.thread-head:global(.elevated)`）。
  只导出 snippet、从不渲染默认导出的组件，其样式会被 Vite 摇掉，所以图标做成了独立组件。
- 部分测试在 vm 里执行 `src/lib/state` 中的真实函数，读取方式见 `src/lib/testing/appSource.mjs`。

### API 服务边界

- 「测试连接」只验证认证及模型列表，不会付费生成内容，也不代表已验证工具能力。模型需支持工具调用。
- 原生 API 执行支持流式文本、本地文件与 SSH 工具。图片 / PDF 识别、Cloudflare 授权操作暂用本地客户端。
- API Key 不传给工具子进程。计划模式禁止写入；询问 / 自动模式下通用本机命令逐次确认；
  全自动模式允许用户已授权的本机命令，**不是操作系统级文件沙箱**。
- 工具执行前持久化中断状态，全部工具结果原子保存后形成安全检查点；不自动重放工具，
  不自动重试断网、鉴权失败或其他错误。单轮最多 32 次模型请求。
- Claude 明确返回 `max_tokens` 时丢弃当次不完整工具调用，最多按原输出上限重新生成一次。
- 账单查询凭据与模型调用 Key 独立保存在系统凭据库，只用于官方报表接口，不用于模型生成。

## 本地打包

```bash
scripts/package-macos.sh        # macOS：.app + .dmg（不出更新包）
scripts/package-windows.cmd     # Windows：NSIS 安装器
```

正式发版走 GitHub Actions，见 [RELEASE.md](RELEASE.md)。

## 安全要点

- 密钥仅存系统凭据库（macOS 钥匙串 / Windows 凭据管理器），service = `com.cligc.pilot`。
- 取消对话时对整个进程组 SIGKILL，避免孙进程带着凭据继续运行。
- SSH 凭据永远不进模型子进程的环境变量；AI 桥只下发本轮租约目录与令牌（`PILOT_SSH_DIR` / `PILOT_SSH_TOKEN`）。
