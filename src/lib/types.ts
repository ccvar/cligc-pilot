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

/** sitebuild = Cloudflare 项目；remote = SSH 机器；repo = GitHub 仓库；workspace = 本地对话（可接入一个文件夹）；site = 站点会话。 */
export type TaskType = 'sitebuild' | 'remote' | 'repo' | 'workspace' | 'site';

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
  /** 这一步发生时正文已经写到哪（字符位置）；用来把步骤插在正文中间。旧消息没有。 */
  at?: number;
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
  /** 会话由谁发起：空＝用户；ops＝站点运营任务。 */
  origin?: string;
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

// ---- 站点 ----

export interface SiteBuild {
  /** 站点在仓库里的子目录（空＝仓库根目录） */
  root?: string;
  install: string;
  build: string;
  out_dir: string;
  checks: string[];
  /** 构建变量名（值只在钥匙串）。 */
  env_keys: string[];
}
export interface SiteDeploy {
  /** direct = Pilot 构建后上传；pages_git = Pages 绑定 GitHub；none = 暂不部署 */
  mode: 'direct' | 'workers' | 'vps' | 'pages_git' | 'none' | '';
  cf_conn_id: string;
  project: string;
  branch: string;
  /** VPS：授权用的 SSH 连接与项目自带的发布命令 */
  ssh_conn_id: string;
  command: string;
  /** VPS：服务器上的配置变量文件；空＝/opt/<站点>/config/<站点>.env */
  env_file?: string;
}
export interface SiteData {
  google_conn_id: string;
  ga4_property: string;
  ga4_property_name: string;
  gsc_site: string;
  /** Search Console 用的 Google 连接；空＝和 GA4 同一条。 */
  gsc_conn_id: string;
  bing_conn_id: string;
  bing_site: string;
  /** Pilot 自动加到 Bing 时拿到的验证码 */
  bing_auth?: string;
  indexnow_key: string;
}
export interface SiteOps {
  level: number;
  whitelist: string[];
  scan: string;
  health: string;
  report: string;
  review: string;
  budget: { changes_per_week: number; items_per_run: number; tokens_per_week: number };
  brain: string;
  model: string;
  downgraded: string[];
  min_impressions: number;
  /** 托管用的站点阶段：空＝自动；new / growing / established＝手动指定 */
  stage: string;
}
export interface Site {
  id: string;
  name: string;
  domain: string;
  source: 'github' | 'local';
  gh_conn_id: string;
  repo: string;
  local_dir: string;
  default_branch: string;
  framework: string;
  build: SiteBuild;
  deploy: SiteDeploy;
  data: SiteData;
  ops: SiteOps;
  pinned: boolean;
  tags: string[];
  /** created：在 Pilot 里从零制作的；migrating / migrated：从线上站点迁移来的；空＝导入的 */
  origin?: string;
  /** 迁移中：旧站域名（迁移期间 domain 留空，切换时才填上） */
  migrate_from?: string;
  created_at: number;
  updated_at: number;
}
/** 站点制作：第一版进度的一步（看文件判断，不看对话） */
export interface MakeStep { key: string; label: string; done: boolean; detail: string }
/** 上线前检查的一项（扫构建产物）。fail 的问题不修不该上线 */
export interface MakeCheck { key: string; label: string; level: 'ok' | 'warn' | 'fail'; detail: string; items: string[] }
/** AI 写在 .pilot/todo.json 里、需要我处理的一件事 */
export interface TodoInput { key: string; label: string; placeholder: string; value: string }
export interface SiteTodo { id: string; title: string; why: string; kind: 'input' | 'ai' | 'confirm' | 'suggest' | string; affects_goal: boolean; where: string; inputs: TodoInput[] }
/** 站点配置变量（值存在 Pilot，不返回给界面） */
export interface ConfigVar { key: string; desc: string; secret: boolean; declared: boolean; set: boolean; /** 没填值时 Pilot 自动给的值（如 SITE_URL＝https://<站点域名>） */ auto: string; /** Pilot 里没值，但读到线上已经设了（Cloudflare 的密钥读不到值） */ remote: boolean; /** 公开变量存的值（密钥不带） */ value: string; /** 上次从线上读取时两边不一样 */ differ: boolean; /** 有值但还没同步到部署目标 */ pending: boolean }
export interface ConfigState { vars: ConfigVar[]; missing: string[]; target: string; target_label: string; needs_sync: boolean; synced_at: number; env_file: string; /** 以前同步过、Pilot 里已删：整体同步时会从那边删掉 */ will_remove: string[] }
/** todos：null＝AI 还没写 .pilot/todo.json */
export interface MakeState { conventions: boolean; todos: SiteTodo[] | null; config: ConfigVar[]; steps: MakeStep[]; checks: MakeCheck[]; built_at: number; profile?: boolean }
export interface SiteDetected {
  framework: string;
  build: SiteBuild;
  env: [string, string][];
  cf_project: string;
  deploy_kind: string;
  deploy_command: string;
  deploy_target: string;
  branch: string;
  domain: string;
  indexnow_key: string;
  git: boolean;
  git_branch: string;
  git_default_branch: string;
  github_repo: string;
  has_dotenv: boolean;
  dotenv_example_keys: string[];
  notes: string[];
}
export interface SiteImportPreview { detected: SiteDetected; name: string; dir: string }
export interface PagesProject {
  name: string;
  subdomain: string;
  domains: string[];
  production_branch: string;
  source_type: string;
  source_repo: string;
}
export interface PagesDeployment {
  id: string;
  url: string;
  environment: string;
  branch: string;
  commit: string;
  message: string;
  status: string;
  stage: string;
  created_on: string;
}
export interface JobStep { key: string; label: string; status: 'pending' | 'running' | 'ok' | 'failed' | 'skipped'; detail: string }
export interface SiteJob {
  id: string;
  site_id: string;
  kind: string;
  title: string;
  status: 'running' | 'ok' | 'failed' | 'canceled';
  started_at: number;
  finished_at: number;
  steps: JobStep[];
  log: string[];
  url: string;
  error: string;
}
export interface DeployRecord {
  id: string;
  ts: number;
  ok: boolean;
  trigger: string;
  commit: string;
  message: string;
  url: string;
  changed: number;
  indexnow: string;
  error: string;
  duration: number;
  dirty: boolean;
}
export interface SiteStatus {
  code_dir: string;
  exists: boolean;
  git: boolean;
  branch: string;
  dirty: boolean;
  head: string;
  head_subject: string;
  job: SiteJob | null;
  last_deploy: DeployRecord | null;
}

// ---- 站点数据 ----
export interface SourceMeta { fetched_at: number; start: string; end: string; error: string }
export interface CacheMeta { ga4: SourceMeta; gsc: SourceMeta; bing: SourceMeta }
export interface DataRow { key: string; key2?: string; clicks: number; impressions: number; ctr: number; position: number }
export interface ComparedRow extends DataRow { prev_clicks: number; prev_impressions: number; prev_position: number }
export interface GaRow { key: string; key2?: string; sessions: number; users: number; views: number; engaged: number }
export interface EventRow { event: string; dim: string; value: string; count: number; users: number }
export interface Opportunity {
  /** data | gap | topic | positioning | health | manual（空＝老数据，按 kind 推断） */
  source?: string;
  /** 目标查询词 */
  target?: string;
  /** AI 计划时写下的预期效果（复盘按它对比） */
  expect_metric?: string;
  expect_query?: string;
  expect_note?: string;
  key: string;
  kind: string;
  title: string;
  page: string;
  queries: DataRow[];
  clicks: number;
  impressions: number;
  ctr: number;
  position: number;
  prev_clicks: number;
  score: number;
  change: string;
  evidence: string;
  /** 判断模型（Jev）给的「搜索词和站点定位的相关度」0–3；没打就没有这个字段 */
  relevance?: number;
}
export interface Dashboard {
  meta: CacheMeta;
  gsc_daily: DataRow[];
  gsc_totals: { clicks: number; impressions: number; ctr: number; position: number };
  gsc_prev_totals: { clicks: number; impressions: number; ctr: number; position: number };
  gsc_queries: ComparedRow[];
  gsc_pages: ComparedRow[];
  gsc_sitemaps: unknown;
  ga4_daily: GaRow[];
  ga4_totals: { sessions: number; users: number; views: number; engaged: number };
  ga4_prev_views: number;
  ga4_pages: GaRow[];
  ga4_sources: GaRow[];
  ga4_events: EventRow[];
  bing_daily: DataRow[];
  bing_totals: { clicks: number; impressions: number; ctr: number; position: number };
  bing_queries: DataRow[];
  bing_pages: DataRow[];
  bing_crawl: unknown;
  opportunities: Opportunity[];
  /** AI 搜索 / 聊天带来的访问：key＝日期，key2＝来源（近 90 天） */
  ga4_ai?: GaRow[];
}

// ---- 运营 ----
export interface OpsItem {
  /** AI 自己对这次改动的说明 */
  ai_summary?: string;
  /** 实际执行的模型（主 / 备用） */
  model_used?: string;
  /** 这次改动涉及的文件 */
  files?: string[];
  id: string;
  opp: Opportunity;
  /** open | working | proposed | merged | deployed | dismissed | rolled_back | failed */
  status: string;
  created_at: number;
  updated_at: number;
  conv_id: string;
  branch: string;
  base_sha: string;
  head_sha: string;
  pr_number: number;
  pr_url: string;
  change_kind: string;
  verdict: string;
  /** 托管自动上线前判断模型（Jev）的内容质检结果 */
  quality?: PageQuality[];
  merge_sha: string;
  deployed_at: number;
  review: string;
  review_note: string;
  note: string;
  auto: boolean;
  /** AI 报告的改动页面 / 新页面，上线后逐个验收 */
  pages?: string[];
  new_pages?: string[];
  /** 最近一次线上验收 */
  checks?: { label: string; ok: boolean; detail?: string; hard?: boolean }[];
  /** 上线了但有一般问题，等你核对（空＝没有） */
  attention?: string;
  retries?: number;
  retry_note?: string;
  watch_stage?: number;
  watched?: string;
  /** 复盘看的是哪个指标 */
  review_metric?: string;
  /** 上线前 / 复盘时这个页面近 28 天的 Google 数据 */
  before?: GscRow | null;
  after?: GscRow | null;
  /** 新页面有没有被 Google 编入索引：空＝还没查 | indexed | not_indexed | error；note 是 Search Console 的覆盖状态 */
  index_state?: string;
  index_note?: string;
  index_checked?: number;
}

// ---- 多站点 ----
export interface SiteSummary {
  gsc_clicks: number; gsc_clicks_prev: number; gsc_impressions: number; gsc_position: number;
  ga4_views: number; ga4_views_prev: number; bing_clicks: number; opportunities: number;
  data_end: string; fetched_at: number; errors: string[];
  /** 近 28 天 GA4 用户 / 前 28 天；近 28 天每天会话（旧的在前） */
  users?: number; users_prev?: number; sessions_trend?: number[];
  /** Google + Bing 近 28 天搜索点击 / 前 28 天 / 展示 */
  search_clicks?: number; search_clicks_prev?: number; search_impressions?: number;
  /** Google 近 28 天有展示的页面数、站点地图提交的网址数、Bing 索引里的网址数 */
  google_pages?: number; sitemap_urls?: number; bing_indexed?: number;
  /** AI 搜索 / 聊天带来的会话：近 28 天、前 28 天 */
  ai_sessions?: number; ai_sessions_prev?: number;
}
/** Search Console 一行（页面或查询词）近 28 天的合计 */
export interface GscRow { key: string; key2?: string; clicks: number; impressions: number; ctr: number; position: number }
/** 「最近上线」的一条：改动 + 这个页面现在近 28 天的数据 */
export interface ShipItem extends PendingItem { page_now?: GscRow | null; data_end: string }
/** 全部站点列表上的一个分发渠道 */
export interface ChannelBrief { kind: string; target_name: string; target_url: string; mode: string; daily_limit: number; enabled: boolean; published: number; failed: number }
/** 在线监测（首页每小时、证书每天、域名每周） */
export interface UptimePing { ts: number; ok: boolean; status: number; ms: number; err: string }
export interface MonitorView { last: UptimePing | null; down: boolean; down_since: number; uptime: number; avg_ms: number; checks: number; cert_expires: number; cert_err: string; cert_checked: number; domain_expires: number; domain_err: string; domain_checked: number }
/** 推广链接检查（每周一次，见 link_check） */
export interface LinkResult { href: string; url: string; pages: number; sample_page: string; status: number; final_url: string; hops: number; verdict: 'ok' | 'unverified' | 'broken'; note: string }
export interface LinkReport { checked_at: number; source: string; links: LinkResult[] }
export interface SiteRow {
  channels?: ChannelBrief[];
  /** 上次推广链接检查里失效的个数 */
  links_broken?: number;
  /** 在线监测：首页连续两次打不开（从什么时候）、证书 / 域名还剩几天、证书出错原因 */
  site_down?: boolean; down_since?: number; down_err?: string; cert_days?: number | null; cert_err?: string; domain_days?: number | null;
  id: string; name: string; domain: string; source: string; repo: string; pinned: boolean; tags: string[];
  level: number;
  /** 所在托管方案名（没托管为空） */
  plan: string;
  /** suggest / review / auto；方案暂停或站点停止时为 stopped；没托管为空 */
  autonomy: string; deploy_mode: string; has_data: boolean; summary: SiteSummary;
  proposed: number; failed: number; open: number; job: string;
  last_deploy_ts: number; last_deploy_ok: boolean | null; active_at: number;
  /** 收益（本月 / 上月，美元）和本月能估出的 AI 花费 */
  money?: Money; cost_month_usd?: number;
}
export interface PendingItem { site_id: string; site_name: string; item: OpsItem }
export interface DataSourceRow { conn: Connection; sites: string[] }
export interface BindReport { bound: [string, string][]; unmatched: string[]; kept: number; errors: string[] }

// ---- 托管运营 ----
export interface ManagedConfig {
  paused: boolean;
  brain: string;
  model: string;
  fallback_brain: string;
  fallback_model: string;
  /** auto 自动发布 | review 人审人发（运营人员审核后再发布） */
  new_content: 'auto' | 'review';
  daily_changes: number;
  tokens_per_week: number;
  digest: boolean;
  digest_hour: number;
  last_digest: string;
  groups: ManagedGroup[];
}
/** 托管方案：一组站点 + 这组站点自己的托管规则。 */
export interface ManagedGroup {
  id: string;
  name: string;
  sites: string[];
  /** 方案里单独停止托管的站点 */
  stopped: string[];
  paused: boolean;
  /** AI 自主程度：suggest 只出建议 | review 改好等你确认 | auto 全自动 */
  autonomy: string;
  brain: string;
  model: string;
  fallback_brain: string;
  fallback_model: string;
  new_content: 'auto' | 'review';
  /** 代码改动：review 留给你确认 | small 小改动自动上线 | all 都自动上线（依赖和部署配置除外） */
  code_changes: 'review' | 'small' | 'all';
  daily_changes: number;
  tokens_per_week: number;
  /** 运营目标（一两句话） */
  goal: string;
  /** 允许的动作：meta / content / internal_links / new_content / technical */
  actions: string[];
  /** 内容准则（运营人员编辑，原样交给 AI） */
  guidelines: string;
  /** 选题清单，一行一个 */
  topics: string;
  /** 衡量标准：clicks / ctr / impressions / position / conversions / health */
  metric: string;
  /** 新页面正文下限（字），低于它不自动上线；0＝不检查 */
  min_page_chars: number;
  /** 自动上线前用判断模型（Jev）做内容质检（接上 TypeSafe 才生效）；老数据没有＝开 */
  jev_gate?: boolean;
  /** 质检没过时先让 AI 按原因重写一次 */
  jev_rewrite?: boolean;
  /** 判断模型认为和站点定位基本无关的搜索词不交给 AI 处理；老数据没有＝开 */
  skip_offtopic?: boolean;
  created_at: number;
}
export interface ManagedSite {
  id: string; group_id: string; active: boolean; stage: string; stage_manual: boolean; name: string; domain: string; shipped_week: number; waiting: number; failed: number; queued: number;
  plan_date: string; plan_count: number; job: string;
  /** 下一次自动运行的任务和到点时间（秒）；托管停着为空 / 0 */
  next_task: string; next_at: number;
  budget: { changes_used: number; changes_limit: number; tokens_used: number; tokens_limit: number };
  /** 这个月的 AI 用量和花费、上线了几条 */
  cost: AiCost; shipped_month: number;
  /** 收益（AdSense + 手动记的） */
  money?: Money;
}
export interface ManagedOverview { config: ManagedConfig; sites: ManagedSite[]; groups: ManagedGroup[]; shipped_today: number; waiting: number; cost_month: AiCost; shipped_month: number; revenue_month?: number; revenue_tracked?: boolean }
/** AI 用量：tokens＝折算 token；usd＝能按官方价估出金额的部分（API 连接）；sub_tokens＝走订阅额度的；unpriced_tokens＝按量计费但认不出价格的 */
export interface AiCost { tokens: number; usd: number; sub_tokens: number; unpriced_tokens: number; rough: boolean; turns: number }
/** 后台停过的一段：closed＝客户端没开，asleep＝电脑睡着了 */
export interface BgGap { from: number; to: number; kind: string }
export interface BackgroundStatus { launch_at_login: boolean; can_keep_awake: boolean; keep_awake: boolean; awake_now: boolean; gaps: BgGap[]; stopped_week: number; notice: BgGap | null; exe: string }
export interface BackupSettings { enabled: boolean; conn_id: string; repo: string; last_at: number; last_ok: boolean; last_msg: string; last_files: number; last_bytes: number; last_try: number }
export interface BackupStatus { settings: BackupSettings; running: boolean; files: number; bytes: number }
export interface BackupManifest { at: number; version: string; host: string; sites: number; files: number; bytes: number }
/** 托管日报的结构化内容（新版日报才有） */
export interface DigestNeed { site_id: string; site_name: string; title: string; why: string }
export interface DigestEvent { kind: string; label: string; title: string; detail: string }
export interface DigestSite { site_id: string; site_name: string; events: DigestEvent[] }
export interface DigestTraffic { site_id: string; site_name: string; clicks: number; clicks_prev: number; views: number; views_prev: number }
export interface DigestData { date: string; generated_at: number; sites_total: number; shipped: number; planned: number; broken: number; needs: DigestNeed[]; active: DigestSite[]; quiet: string[]; traffic: DigestTraffic[]; no_traffic: string[] }
/** 日报日历里的一天（只有有动静的日子） */
export interface DayCount { date: string; shipped: number; planned: number; proposed: number; broken: number; ran: number }
export interface ManagedActivity { ts: number; site_id: string; site_name: string; kind: string; title: string; detail: string; item_id: string; change?: string; source?: string }
export interface PlanTask { title: string; page: string; change: string; why: string; priority: number; source?: string; query?: string; expect?: { metric?: string; query?: string; note?: string } }
export interface StageInfo { stage: string; auto_stage: string; reason: string; manual: string }
/** AI 归纳的站点定位；by=user 表示运营人员改过，AI 不再覆盖。 */
export interface Positioning { text: string; by: string; updated: number }
export interface SiteStageView extends StageInfo { positioning: Positioning }
export interface SitePlan { site_id: string; site_name: string; plan: { date: string; conv_id: string; brain?: string; model?: string; stage?: string; notes?: string; tasks: PlanTask[] }; positioning: Positioning; stage: StageInfo; task_status: string[]; task_item: string[] }
/** 判断模型（Jev）对一个页面的内容质检：分数 0–3，概率 0–1 */
export interface PageQuality {
  page: string;
  answers?: number | null;
  substance: number;
  on_topic: number;
  unsourced: number;
  leftover: number;
  reasons: string[];
}
/** 设置里的判断模型（TypeSafe） */
export interface TypeSafeStatus {
  configured: boolean;
  key_prefix: string;
  model: string;
  verified_at: number;
  month_tokens: number;
  month_cost_usd: number;
}

// ---- 收益（AdSense + 手动记的，美元） ----
export interface Money { month: number; last_month: number; adsense_month: number; adsense_last: number; rpm: number; tracked: boolean }
export interface RevenueEntry { id: string; month: string; amount: number; source: 'affiliate' | 'sponsor' | 'other' | string; note: string; created_at: number }
export interface PageRevenue { url: string; earnings: number; views: number }
export interface SiteRevenue { money: Money; entries: RevenueEntry[]; top_pages: PageRevenue[]; cost: AiCost; adsense: boolean }
export interface RevenueStatus { connected: boolean; email: string; account: string; account_name: string; fetched_at: number; error: string; has_client: boolean }
export interface RevenueImport { added: number; unknown: string[]; bad_lines: number[] }

// ---- 手机通知（Telegram） ----
export interface PushStatus { has_token: boolean; bot_name: string; chat_name: string; bound: boolean; enabled: boolean; allow_approve: boolean; pair_code: string; pair_until: number; last_err: string }

// ---- 页面速度（PageSpeed Insights） ----
export interface SpeedIssue { title: string; savings_ms: number }
export interface PageSpeed { url: string; score: number; lcp_ms: number; cls: number; tbt_ms: number; field: string; field_lcp_ms: number; field_inp_ms: number; field_cls: number; issues: SpeedIssue[]; error: string }
export interface SpeedReport { checked_at: number; pages: PageSpeed[] }

// ---- 页面体检（图片、结构化数据） ----
export interface ImgIssue { page: string; src: string; bytes: number }
export interface AuditReport { checked_at: number; pages: number; images: number; no_alt: ImgIssue[]; broken: ImgIssue[]; large: ImgIssue[]; schema_pages: number; article_pages: number; schema: { page: string; problem: string }[]; llms: string }

// ---- 竞品观察（Serper） ----
export interface SerpHit { pos: number; domain: string; title: string; link: string; snippet: string }
export interface SerpKeyword { q: string; manual: boolean; ours: number | null; our_url: string; prev: number | null; prev_checked: boolean; top: SerpHit[]; new_domains: string[]; error: string }
export interface SerpCompetitor { domain: string; keywords: number; above_us: number; avg_pos: number }
export interface SerpConfig { off: boolean; keywords: string[]; gl: string; hl: string }
export interface SerpView { has_key: boolean; config: SerpConfig; report: { checked_at: number; gl: string; hl: string; keywords: SerpKeyword[]; competitors: SerpCompetitor[] }; planned: string[] }
export interface ApiKeys { pagespeed: boolean; serper: boolean }
