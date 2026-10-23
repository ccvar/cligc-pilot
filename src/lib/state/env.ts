import type { DialogGeometryOptions } from '$lib/dialogGeometry';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { SheetWidthOptions } from '$lib/sheetWidth';
import type { Connection } from '$lib/types';

export const DIALOG_GEOMETRY = {
  ssh: { id: 'ssh-connection', minWidth: 460, minHeight: 420, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
  connectionRemark: { id: 'connection-remark', minWidth: 320, minHeight: 160, draggable: true, resizable: false, rememberPosition: true },
  fileOperation: { id: 'file-operation', minWidth: 320, minHeight: 160, draggable: true, resizable: false, rememberPosition: true },
  fileEditor: { id: 'file-editor', storageVersion: 2, minWidth: 560, minHeight: 380, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
  github: { id: 'github-connect', minWidth: 400, minHeight: 360, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
  cloudflare: { id: 'cloudflare-connect', minWidth: 380, minHeight: 360, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
  templateSave: { id: 'template-save', minWidth: 320, minHeight: 180, draggable: true, resizable: false, rememberPosition: true },
  templateUse: { id: 'template-use', minWidth: 320, minHeight: 180, draggable: true, resizable: false, rememberPosition: true },
  promptEditor: { id: 'prompt-editor', minWidth: 420, minHeight: 320, draggable: true, resizable: true, rememberPosition: true, rememberSize: true },
} satisfies Record<string, DialogGeometryOptions>;

function currentPilotWindowLabel(): string {
  try { return getCurrentWindow().label; }
  catch { return 'main'; }
}
export const pilotWindowLabel = currentPilotWindowLabel();

export const SHEET_WIDTH = {
  setup: { id: 'connection-settings', defaultWidth: 400, minWidth: 360, maxWidth: 720, label: '调整连接与模型面板宽度' },
  updateCenter: { id: 'update-center', defaultWidth: 540, minWidth: 440, maxWidth: 900, label: '调整更新中心面板宽度' },
} satisfies Record<string, SheetWidthOptions>;

// 本地对话是一个不落 connections.json 的虚拟连接：没有凭据，只给模型一个隔离的工作目录。
export const WORKSPACE_CONN_ID = '__workspace__';
// 「站点」也是一个不落盘的虚拟工作区：左栏是全部站点，主区是总览；选中某个站点时切到 site:<id>。
export const SITES_CONN_ID = '__sites__';
export const SITES_CONN: Connection = { id: SITES_CONN_ID, name: '站点', kind: 'sites', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
/** 切换器里「远程连接」这一行（右键菜单用；新窗口打开时落到上次用过的那台机器）。 */
export const REMOTES_CONN: Connection = { id: '__remotes__', name: '远程连接', kind: 'remotes', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
export const LOCAL_CONN: Connection = { id: WORKSPACE_CONN_ID, name: '本地对话', kind: 'workspace', skill_dir: '', key_prefix: '', key_kind: '', account_id: '', created_at: '' };
// Claude Code 用官方原生安装器（独立二进制，不需要 Node/npm）——npm 命令对新手是三重门槛。
// 注意：不能引用 isWindows（声明在后面，TDZ），这里自足判断。
export const CLAUDE_INSTALL_CMD = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  ? 'irm https://claude.ai/install.ps1 | iex'
  : 'curl -fsSL https://claude.ai/install.sh | sh';
// Grok 官方脚本（独立二进制，不需要 Node）；Windows 官方要求 Git-Bash/MSYS2 跑同一条命令。
export const GROK_INSTALL_CMD = 'curl -fsSL https://x.ai/cli/install.sh | bash';

// Windows 无 macOS 红绿灯（窗口控件在右侧），顶部工具按钮改靠左对齐，别飘在中间。
export const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
export const isMac = typeof navigator !== 'undefined' && /Macintosh|Mac OS X/i.test(navigator.userAgent);
/** 密钥存储的平台名称（文案用）：mac=钥匙串，win=凭据管理器。 */
export const keystoreName = isWindows ? 'Windows 凭据管理器' : 'macOS 钥匙串';

// 「新窗口打开」开出来的窗口，label 是 `conn-<id>`：这一份前端就固定开在那个连接上。
// （连接/对话/SSH 会话都在 Rust 侧同一份 AppState 里，多个窗口看到的是同一份数据。）
// 用 label 而不是 URL 参数：带 query 的 WebviewUrl::App 在 dev 下会指到 /index.html，
// 而 SvelteKit 只认 /，开出来是白屏。详见 lib.rs::open_conn_window。
export const CONN_WIN_PREFIX = 'conn-';
export const LAST_CONN_KEY = 'cligc.pilot.lastConn';
export const isMainWindow = pilotWindowLabel === 'main';
