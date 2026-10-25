#!/usr/bin/env node
// Pilot 随附的安卓手机工具：通过 adb 截图、读屏幕上的元素、点击、滑动、输入、按键、打开 app。
// 每条命令输出一行 JSON：{"ok":true,...} 或 {"ok":false,"code":"...","detail":"..."}。
// 用法见 Pilot 写给 AI 的说明（agent::phone_prompt）。
'use strict';
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const args = process.argv.slice(2);
const cmd = args.shift() || 'help';
function opt(name, def) {
  const i = args.indexOf('--' + name);
  if (i === -1) return def;
  const v = args[i + 1];
  return v === undefined || v.startsWith('--') ? true : v;
}
const positional = args.filter((a, i) => !a.startsWith('--') && !(i > 0 && args[i - 1].startsWith('--') && !['--all'].includes(args[i - 1])));
function out(obj, code) { process.stdout.write(JSON.stringify(obj) + '\n'); process.exit(code ?? (obj.ok ? 0 : 1)); }
function fail(code, detail, extra) { out({ ok: false, code, detail, ...(extra || {}) }, 1); }

// ---- 找 adb：Pilot 装的（脚本旁边的 platform-tools）> PATH > 常见安装位置 ----
function findAdb() {
  const exe = process.platform === 'win32' ? 'adb.exe' : 'adb';
  const cands = [
    process.env.PILOT_ADB,
    path.join(__dirname, 'platform-tools', exe),
    ...String(process.env.PATH || '').split(path.delimiter).map((d) => d && path.join(d, exe)),
    path.join(os.homedir(), 'Library', 'Android', 'sdk', 'platform-tools', exe),
    path.join(os.homedir(), 'Android', 'Sdk', 'platform-tools', exe),
    '/opt/homebrew/bin/adb', '/usr/local/bin/adb',
  ].filter(Boolean);
  return cands.find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } });
}
const ADB = findAdb();
if (!ADB && cmd !== 'help' && cmd !== 'mail-info') fail('adb_missing', '没有找到 adb。请在 Pilot「设置 → 安卓手机」里一键安装。');

function adb(a, opts = {}) {
  const serial = opt('serial', process.env.ANDROID_SERIAL || '');
  const full = (serial && serial !== true ? ['-s', serial] : []).concat(a);
  const r = spawnSync(ADB, full, { encoding: opts.binary ? 'buffer' : 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: opts.timeout || 30000 });
  if (r.error) fail('adb_error', String(r.error.message || r.error));
  const err = (opts.binary ? r.stderr.toString('utf8') : r.stderr || '').trim();
  if (r.status !== 0) {
    if (/no devices|device not found|offline/i.test(err)) fail('no_device', '没有连上手机：用 USB 线连接，在手机上打开「开发者选项 → USB 调试」，并在手机弹窗里点「允许」。');
    if (/unauthorized/i.test(err)) fail('unauthorized', '手机还没授权这台电脑：请在手机上的「允许 USB 调试吗？」弹窗里点「允许」。');
    if (/more than one/i.test(err)) fail('many_devices', '连了不止一台手机：加 --serial 指定一台（用 devices 命令看序列号）。');
    fail('adb_failed', err || ('adb 退出码 ' + r.status));
  }
  return r.stdout;
}
const sh = (s, o) => adb(['shell', s], o);

// ---- 屏幕元素：uiautomator dump ----
function uiNodes() {
  const dumped = sh('uiautomator dump /sdcard/pilot_ui.xml 2>&1');
  if (/ERROR|null root/i.test(dumped)) fail('ui_unavailable', '读不到屏幕元素（有的页面或动画中会这样）：稍等再试，或者用截图看屏幕、按坐标点。', { raw: dumped.trim().slice(0, 200) });
  const xml = adb(['exec-out', 'cat', '/sdcard/pilot_ui.xml']);
  const nodes = [];
  const re = /<node\b([^>]*?)\/?>/g;
  let m;
  while ((m = re.exec(xml))) {
    const a = {};
    for (const am of m[1].matchAll(/([\w-]+)="([^"]*)"/g)) a[am[1]] = am[2].replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'");
    const b = /\[(\d+),(\d+)\]\[(\d+),(\d+)\]/.exec(a.bounds || '');
    if (!b) continue;
    const [x1, y1, x2, y2] = b.slice(1).map(Number);
    if (x2 <= x1 || y2 <= y1) continue;
    nodes.push({
      text: a.text || '', desc: a['content-desc'] || '', id: (a['resource-id'] || '').replace(/^.*:id\//, ''),
      cls: (a.class || '').split('.').pop(), clickable: a.clickable === 'true', password: a.password === 'true',
      focused: a.focused === 'true', editable: /EditText/.test(a.class || ''), pkg: a.package || '',
      x: Math.round((x1 + x2) / 2), y: Math.round((y1 + y2) / 2), w: x2 - x1, h: y2 - y1,
    });
  }
  return nodes;
}
const label = (n) => n.text || n.desc;

function findNodes(q) {
  const nodes = uiNodes();
  const qq = String(q).trim().toLowerCase();
  const exact = nodes.filter((n) => label(n).trim().toLowerCase() === qq || n.id.toLowerCase() === qq);
  const hits = exact.length ? exact : nodes.filter((n) => label(n).toLowerCase().includes(qq));
  return { nodes, hits };
}

// 整屏是不是一种颜色（多半是 app 禁止截图出来的黑屏，或者屏幕熄灭）：解开 PNG 的图像数据，
// 一张纯色图经过 PNG 的行过滤后，除了每行开头和第一个像素，几乎全是 0。
function looksUniform(buf, size) {
  try {
    const zlib = require('zlib');
    const parts = [];
    let colorType = 2;
    for (let p = 8; p + 8 <= buf.length;) {
      const len = buf.readUInt32BE(p);
      const type = buf.toString('ascii', p + 4, p + 8);
      if (type === 'IHDR') colorType = buf[p + 8 + 9];
      if (type === 'IDAT') parts.push(buf.subarray(p + 8, p + 8 + len));
      p += 12 + len;
    }
    const raw = zlib.inflateSync(Buffer.concat(parts));
    const bpp = colorType === 6 ? 4 : colorType === 2 ? 3 : colorType === 4 ? 2 : 1;
    const stride = size.w * bpp + 1;
    let nonzero = 0, total = 0;
    // 每隔几行抽一行，跳过每行开头的过滤字节和第一个像素；有透明通道时跳过透明度字节
    for (let row = 0; row < size.h; row += 7) {
      const start = row * stride + 1 + bpp;
      for (let i = start; i < (row + 1) * stride && i < raw.length; i++) {
        if (bpp === 4 && (i - row * stride - 1) % 4 === 3) continue;
        total++;
        if (raw[i] !== 0) nonzero++;
      }
    }
    return total > 0 && nonzero / total < 0.002;
  } catch { return false; }
}

function pngSize(buf) {
  if (buf.length < 24 || buf.readUInt32BE(0) !== 0x89504e47) return null;
  return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
}

// 输入文字：adb 的 input text 只认 ASCII，空格写成 %s，shell 特殊字符要转义
function shellText(t) {
  return t.replace(/ /g, '%s').replace(/([\\'"`$&|;<>(){}\[\]*?!~#])/g, '\\$1');
}

// ---- 邮箱验证码：交给 Pilot 去邮箱里取、直接填进手机（脚本和 AI 都拿不到邮件内容）----
function pilotMail(req, timeoutMs) {
  let bridge;
  try { bridge = JSON.parse(fs.readFileSync(path.join(__dirname, 'mail-bridge.json'), 'utf8')); }
  catch { fail('pilot_not_running', 'Pilot 没在运行（邮箱验证码要 Pilot 开着）。'); }
  const net = require('net');
  const sock = net.createConnection({ host: '127.0.0.1', port: bridge.port });
  let buf = '';
  const timer = setTimeout(() => { sock.destroy(); fail('timeout', 'Pilot 没有响应'); }, timeoutMs);
  sock.on('connect', () => sock.write(JSON.stringify({ ...req, token: bridge.token }) + '\n'));
  sock.on('data', (d) => {
    buf += d;
    const i = buf.indexOf('\n');
    if (i === -1) return;
    clearTimeout(timer);
    sock.destroy();
    let r;
    try { r = JSON.parse(buf.slice(0, i)); } catch { fail('bad_reply', 'Pilot 的回复看不懂'); }
    out(r);
  });
  sock.on('error', (e) => { clearTimeout(timer); fail('pilot_not_running', 'Pilot 没在运行（邮箱验证码要 Pilot 开着）：' + e.message); });
}
const serialOpt = () => { const s = opt('serial', process.env.ANDROID_SERIAL || ''); return s === true ? '' : String(s); };

switch (cmd) {
  case 'help':
    out({ ok: true, usage: 'devices | shot --out <png> | ui [--all] | tap --text <文字> [--index n] | tap --x <x> --y <y> | swipe up|down|left|right | type --text <ASCII 文字> | key back|home|enter|recent|tab|delete | apps [--search 关键词] | launch <包名> | open --url <链接> | wait <毫秒> | mail-info | mail-code --from <网站域名> [--wait 秒] | mail-link --from <网站域名>（都可加 --serial）' });
    break;
  case 'devices': {
    const raw = adb(['devices', '-l']);
    const devices = raw.split('\n').slice(1).map((l) => l.trim()).filter(Boolean).map((l) => {
      const [serial, state, ...rest] = l.split(/\s+/);
      const kv = Object.fromEntries(rest.map((x) => x.split(':')).filter((p) => p.length === 2));
      return { serial, state, model: (kv.model || '').replace(/_/g, ' '), device: kv.device || '' };
    });
    out({ ok: true, devices });
    break;
  }
  case 'shot': {
    const file = opt('out');
    if (!file || file === true) fail('bad_args', '缺 --out <保存路径.png>');
    const buf = adb(['exec-out', 'screencap', '-p'], { binary: true, timeout: 30000 });
    const size = pngSize(buf);
    if (!size) fail('shot_failed', '截图失败：手机返回的不是图片（屏幕可能锁着）。');
    fs.mkdirSync(path.dirname(path.resolve(file)), { recursive: true });
    fs.writeFileSync(file, buf);
    const blank = looksUniform(buf, size);
    out({ ok: true, file: path.resolve(file), width: size.w, height: size.h, bytes: buf.length,
      ...(blank ? { warning: 'likely_blank', detail: '这张图几乎是纯色：可能是 app 禁止截图（交易所、银行常这样，截出来是黑的），或者屏幕熄灭了。请打开图片确认。' } : {}) });
    break;
  }
  case 'ui': {
    const nodes = uiNodes();
    const all = opt('all', false);
    const list = nodes.filter((n) => all || label(n) || n.clickable || n.editable)
      .map((n, i) => ({ i, text: label(n).slice(0, 60), id: n.id || undefined, type: n.editable ? 'input' : n.clickable ? 'button' : 'text', ...(n.password ? { password: true } : {}), ...(n.focused ? { focused: true } : {}), x: n.x, y: n.y }))
      .slice(0, 150);
    out({ ok: true, app: nodes[0]?.pkg || '', count: list.length, elements: list });
    break;
  }
  case 'tap': {
    const q = opt('text');
    if (q && q !== true) {
      const { hits } = findNodes(q);
      if (!hits.length) fail('not_found', `屏幕上没找到「${q}」：先用 ui 命令看看现在有哪些元素，或者滑动后再找。`);
      const idx = Number(opt('index', NaN));
      const pick = Number.isFinite(idx) ? hits[idx] : hits.length === 1 ? hits[0] : (hits.filter((n) => n.clickable).length === 1 ? hits.find((n) => n.clickable) : null);
      if (!pick) fail('ambiguous', `「${q}」有 ${hits.length} 处，用 --index 指定第几个`, { candidates: hits.slice(0, 10).map((n, i) => ({ index: i, text: label(n).slice(0, 40), x: n.x, y: n.y })) });
      sh(`input tap ${pick.x} ${pick.y}`);
      out({ ok: true, tapped: label(pick), x: pick.x, y: pick.y });
    }
    const x = Number(opt('x')), y = Number(opt('y'));
    if (!Number.isFinite(x) || !Number.isFinite(y)) fail('bad_args', '用 --text <文字> 或 --x <x> --y <y>');
    sh(`input tap ${Math.round(x)} ${Math.round(y)}`);
    out({ ok: true, x, y });
    break;
  }
  case 'swipe': {
    const dir = positional[0];
    const m = /Physical size: (\d+)x(\d+)/.exec(sh('wm size')) || [];
    const W = Number(m[1]) || 1080, H = Number(m[2]) || 2400;
    const from = opt('from'), to = opt('to');
    let p;
    if (from && to && from !== true && to !== true) p = [...String(from).split(','), ...String(to).split(',')].map(Number);
    else p = ({ up: [W / 2, H * 0.75, W / 2, H * 0.3], down: [W / 2, H * 0.3, W / 2, H * 0.75], left: [W * 0.85, H / 2, W * 0.15, H / 2], right: [W * 0.15, H / 2, W * 0.85, H / 2] })[dir];
    if (!p || p.some((v) => !Number.isFinite(v))) fail('bad_args', 'swipe up|down|left|right，或 --from x,y --to x,y');
    sh(`input swipe ${p.map(Math.round).join(' ')} ${Number(opt('ms', 350))}`);
    out({ ok: true, swipe: dir || 'custom' });
    break;
  }
  case 'type': {
    const t = opt('text');
    if (!t || t === true) fail('bad_args', '缺 --text');
    const focused = uiNodes().find((n) => n.focused && n.editable);
    if (focused && focused.password) fail('password_field', '当前是密码输入框：密码由用户自己在手机上输入，不能代填。');
    if (/[^\x20-\x7e]/.test(t)) fail('non_ascii', 'adb 只能输入英文、数字和符号，中文请让用户在手机上输入，或换用英文关键词。');
    sh(`input text ${shellText(String(t))}`);
    out({ ok: true, typed: String(t).length });
    break;
  }
  case 'key': {
    const k = ({ back: 4, home: 3, enter: 66, recent: 187, tab: 61, delete: 67, menu: 82 })[positional[0]];
    if (!k) fail('bad_args', 'key back|home|enter|recent|tab|delete|menu');
    sh(`input keyevent ${k}`);
    out({ ok: true, key: positional[0] });
    break;
  }
  case 'apps': {
    const q = opt('search', '');
    const pkgs = sh('pm list packages -3').split('\n').map((l) => l.replace(/^package:/, '').trim()).filter(Boolean)
      .filter((p) => !q || q === true || p.toLowerCase().includes(String(q).toLowerCase())).sort();
    out({ ok: true, count: pkgs.length, packages: pkgs.slice(0, 200) });
    break;
  }
  case 'launch': {
    const pkg = positional[0];
    if (!pkg || !/^[\w.]+$/.test(pkg)) fail('bad_args', 'launch <包名>（用 apps --search 关键词 找包名）');
    const r = sh(`monkey -p ${pkg} -c android.intent.category.LAUNCHER 1 2>&1`);
    if (/No activities found|not found/i.test(r)) fail('not_installed', `手机上没有装 ${pkg}，或者它没有启动入口。`);
    out({ ok: true, launched: pkg });
    break;
  }
  case 'open': {
    const url = opt('url');
    if (!url || url === true || /['"\s]/.test(url)) fail('bad_args', 'open --url <链接>');
    sh(`am start -a android.intent.action.VIEW -d '${url}'`);
    out({ ok: true, opened: url });
    break;
  }
  case 'wait': {
    const ms = Math.min(Number(positional[0]) || 1000, 30000);
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
    out({ ok: true, waited: ms });
    break;
  }
  case 'mail-info':
    pilotMail({ op: 'info' }, 10000);
    break;
  case 'mail-code':
  case 'mail-link': {
    const from = opt('from');
    if (!from || from === true) fail('bad_args', '缺 --from <发邮件的网站域名>，比如 --from okx.com');
    const wait = Math.min(Math.max(Number(opt('wait', 90)) || 90, 10), 180);
    if (cmd === 'mail-code' && !opt('force', false)) {
      // 先确认光标在输入框里（不是密码框），否则验证码会填到别处去
      const focused = uiNodes().find((n) => n.focused && n.editable);
      if (focused && focused.password) fail('password_field', '当前是密码输入框，不能往里填验证码。');
      if (!focused) fail('no_input_focused', '先点一下验证码输入框（tap），再取验证码；确实是输入框但认不出时加 --force。');
    }
    pilotMail({ op: cmd === 'mail-code' ? 'code' : 'link', from: String(from), serial: serialOpt(), wait, reuse: !!opt('reuse', false) }, (wait + 60) * 1000);
    break;
  }
  default:
    fail('bad_args', `不认识的命令 ${cmd}，用 help 看用法`);
}
