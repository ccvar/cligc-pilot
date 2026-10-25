#!/usr/bin/env node
// render.js — Cligc Pilot 站点迁移用：用浏览器打开一批网页，等脚本跑完后把渲染出来的 HTML 存下来（JS 渲染的站直接抓 HTML 是空的）。
// 用法: node render.js --list <urls.json> --out <目录> [--wait 2500] [--timeout 30000]
// urls.json 是网址数组；第 i 个存成 <目录>/<i>.html。每完成一个打印一行 JSON {i, ok, final_url, error}。
// 只用 Pilot 自己的浏览器配置（数据目录下 browser-profile/migrate），不读取日常 Chrome 的账号、Cookie 或历史记录。
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");

const args = process.argv.slice(2);
const opt = {};
for (let i = 0; i < args.length; i += 2) opt[String(args[i]).replace(/^--/, "")] = args[i + 1];
if (!opt.list || !opt.out) {
  process.stdout.write(JSON.stringify({ ok: false, error: "用法: node render.js --list <urls.json> --out <目录>" }) + "\n");
  process.exit(1);
}
const urls = JSON.parse(fs.readFileSync(opt.list, "utf8"));
const outDir = path.resolve(opt.out);
fs.mkdirSync(outDir, { recursive: true });
const wait = parseInt(opt.wait || "2500", 10) || 2500;
const timeout = parseInt(opt.timeout || "30000", 10) || 30000;
const profile = path.resolve(path.join(__dirname, "..", "browser-profile", "migrate"));
fs.mkdirSync(profile, { recursive: true });

function emit(v) { process.stdout.write(JSON.stringify(v) + "\n"); }
function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

function findBrowser() {
  const c = [];
  if (process.platform === "darwin") {
    c.push(
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
      "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
      "/Applications/Chromium.app/Contents/MacOS/Chromium",
      "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
    );
  } else if (process.platform === "win32") {
    for (const base of [process.env["ProgramFiles"], process.env["ProgramFiles(x86)"], process.env.LOCALAPPDATA].filter(Boolean)) {
      c.push(path.join(base, "Google", "Chrome", "Application", "chrome.exe"), path.join(base, "Microsoft", "Edge", "Application", "msedge.exe"));
    }
  } else {
    c.push("/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/microsoft-edge");
  }
  return c.find(p => { try { return fs.existsSync(p); } catch { return false; } }) || null;
}

class Cdp {
  constructor(child) {
    this.child = child; this.nextId = 1; this.pending = new Map(); this.buffer = Buffer.alloc(0);
    child.stdio[4].on("data", chunk => this.onData(chunk));
  }
  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const end = this.buffer.indexOf(0);
      if (end < 0) break;
      const raw = this.buffer.subarray(0, end).toString("utf8");
      this.buffer = this.buffer.subarray(end + 1);
      let msg; try { msg = JSON.parse(raw); } catch { continue; }
      const p = msg.id && this.pending.get(msg.id);
      if (!p) continue;
      this.pending.delete(msg.id); clearTimeout(p.timer);
      if (msg.error) p.reject(new Error(msg.error.message || "CDP error")); else p.resolve(msg.result || {});
    }
  }
  send(method, params = {}, sessionId, waitMs = 15000) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(method + " timed out")); }, waitMs);
      this.pending.set(id, { resolve, reject, timer });
      const msg = { id, method, params }; if (sessionId) msg.sessionId = sessionId;
      this.child.stdio[3].write(JSON.stringify(msg) + "\0");
    });
  }
}

async function main() {
  const browser = findBrowser();
  if (!browser) { emit({ ok: false, error: "没找到可用浏览器。请安装 Google Chrome、Microsoft Edge、Chromium 或 Brave。" }); process.exitCode = 1; return; }
  const child = spawn(browser, [
    "--headless=new", "--remote-debugging-pipe", "--disable-gpu", "--hide-scrollbars", "--mute-audio",
    "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--use-mock-keychain", "--password-store=basic",
    "--user-data-dir=" + profile, "--window-size=1280,900", "about:blank",
  ], { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"], windowsHide: true });
  const cdp = new Cdp(child);
  try {
    await cdp.send("Browser.getVersion");
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, sessionId);
    await cdp.send("Runtime.enable", {}, sessionId);
    for (let i = 0; i < urls.length; i++) {
      try {
        const nav = await cdp.send("Page.navigate", { url: urls[i] }, sessionId, timeout);
        if (nav.errorText) throw new Error(nav.errorText);
        const deadline = Date.now() + timeout;
        for (;;) {
          const r = await cdp.send("Runtime.evaluate", { expression: "document.readyState", returnByValue: true }, sessionId);
          if ((r.result && r.result.value) === "complete" || Date.now() > deadline) break;
          await sleep(300);
        }
        await sleep(wait);
        const r = await cdp.send("Runtime.evaluate", { expression: "({html:'<!doctype html>'+document.documentElement.outerHTML,href:location.href})", returnByValue: true }, sessionId, 20000);
        const v = (r.result && r.result.value) || {};
        fs.writeFileSync(path.join(outDir, i + ".html"), v.html || "");
        emit({ i, ok: true, final_url: v.href || urls[i] });
      } catch (e) {
        emit({ i, ok: false, error: String(e && e.message || e) });
      }
    }
  } finally {
    try { await cdp.send("Browser.close", {}, undefined, 1000); } catch { /* ignore */ }
    try { child.kill("SIGTERM"); } catch { /* ignore */ }
  }
}
main().catch(e => { emit({ ok: false, error: String(e && e.stack || e) }); process.exitCode = 1; });
