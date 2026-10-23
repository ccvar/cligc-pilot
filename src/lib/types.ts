export interface Connection {
  id: string;
  name: string;
  /** 用户自定义备注；缺省/空串时继续显示脱敏 Key。 */
  remark?: string;
  /** cloudflare（CF token 建站）| ssh（远程机器）| github（仓库）| workspace（前端合成的本地对话，不落盘）。 */
  kind: string;
  /** 连接工作区目录（CF 项目放在其下的 projects/）。 */
  skill_dir: string;
  /** SSH 连接此项恒为空：它的秘密是密码/口令，显示前缀等于把密码印在 UI 上。 */
  key_prefix: string;
  key_kind: string;
  /** Cloudflare 账号 id；GitHub 为数字用户 id。 */
  account_id: string;
  /** GitHub 登录名（仅 kind=github）。 */
  login?: string;
  /** 以下仅 kind=ssh。 */
  ssh_host?: string;
  ssh_port?: number;
  ssh_user?: string;
  /** password | key */
  ssh_auth?: string;
  ssh_key_path?: string;
  /** 已确认的主机指纹（TOFU）；连接时必须匹配。 */
  ssh_fingerprint?: string;
  /** 远端系统（os-release 的 PRETTY_NAME，如 "Ubuntu 24.04.1 LTS"）；空 = 还没探过。 */
  ssh_os?: string;
  /** 发行版 id（os-release 的 ID，如 ubuntu/debian）——UI 据此选发行版图标。 */
  ssh_os_id?: string;
  created_at: string;
}

export type LocalBrain = 'claude' | 'codex' | 'grok';
/** API profiles have stable identities independent of the model and site connection. */
export type Brain = LocalBrain | `api:${string}`;

export interface BrainStatus {
  found: boolean;
  path: string;
  version: string;
  logged_in: boolean | null;
  account: string;
  detail: string;
}

export interface BrainsInfo {
  claude: BrainStatus;
  codex: BrainStatus;
  /** xAI Grok CLI（ACP 接入）；登录态看 ~/.grok/auth.json。 */
  grok: BrainStatus;
  /** Cloudflare 部署工具（wrangler）；用 env token，logged_in 恒为 null。 */
  wrangler: BrainStatus;
  /** 无头截图用的浏览器（Chrome/Edge/Chromium，可选能力）。 */
  browser: BrainStatus;
  /** Node.js（npm 装 Codex/wrangler 的前置；Claude Code 原生安装不需要）。 */
  node: BrainStatus;
  path_env: string;
  /** 装着的 Claude CLI 够不够新到能用 headless fast mode（判定在 Rust，前端不重写 semver 比较）。 */
  claude_fast_ok: boolean;
}

/** Claude CLI 可升级情况（check_claude_update 的返回）。 */
export interface CliUpdateInfo {
  installed: string;
  latest: string;
  has_update: boolean;
  /** 会被真正升级的那一个（= PATH 里第一个）。 */
  target: string;
  /** PATH 上全部同名安装；>1 时只有第一个生效。 */
  all: string[];
  fast_after_upgrade: boolean;
}

// ---- 对话 ----

/** sitebuild = Cloudflare 项目；remote = SSH 机器；repo = GitHub 仓库；workspace = 本地对话（可接入一个文件夹）。 */
export type TaskType = 'sitebuild' | 'remote' | 'repo' | 'workspace';

/** fine-grained Token 能访问的 GitHub 仓库。 */
export interface GhRepo {
  full_name: string;
  private: boolean;
  default_branch: string;
  /** Token 对它是否有写权限（推分支、提 PR）。 */
  can_push: boolean;
}

export interface ToolCall {
  label: string;
  detail: string;
}

export interface Message {
  role: 'user' | 'assistant';
  text: string;
  tools: ToolCall[];
  ts: number;
  hidden: boolean;
  error: boolean;
  /** 本轮因订阅额度/限流失败：恢复时间戳秒（0=拿不到时间）。null/undefined=非限额错误。 */
  limit_reset?: number | null;
}

export interface Conversation {
  id: string;
  /** 所属连接；本地对话固定为 __workspace__。 */
  conn_id: string;
  conn_name: string;
  /** Cloudflare 项目 slug（其它会话为空）。 */
  site_slug: string;
  site_name: string;
  task_type: TaskType;
  brain: Brain;
  model: string;
  /** 权限档位：plan | ask | auto | full。 */
  perm_mode: string;
  /** 思考等级（推理强度）：'' 默认 | low | medium | high。 */
  effort?: string;
  /** fast 模式（仅 Opus 5 / Opus 4.8）。 */
  fast?: boolean;
  /** 本地对话显式选择的工作目录；空/缺省表示会话隔离目录。 */
  workspace_dir?: string;
  session_ref: string;
  title: string;
  messages: Message[];
  status: 'idle' | 'running';
  created_at: number;
  updated_at: number;
  /** 最近一轮上下文 token（≈当前会话大小），0＝无数据。 */
  ctx_tokens?: number;
  /** 本会话累计处理 token。 */
  total_tokens?: number;
}

export type TurnEvent =
  | { type: 'delta'; text: string }
  /** Replace only the current turn's streamed text with a safe response checkpoint. */
  | { type: 'text_reset'; text: string }
  | { type: 'tool'; label: string; detail: string }
  | {
      type: 'activity';
      activity_id: string;
      label: string;
      detail: string;
      error: string;
      status: 'running' | 'completed' | 'failed' | 'canceled';
      kind?: 'reasoning' | 'command' | 'tool' | 'skill' | 'image_generation' | 'image_review' | 'progress' | string;
      phase?: string;
      current?: number;
      total?: number;
    }
  | { type: 'context_compacted'; pre_tokens: number }
  | { type: 'done'; ok: boolean; error: string };
