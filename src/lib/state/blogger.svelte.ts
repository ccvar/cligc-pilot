// Blogger 账号（「连接 → Blogger」）：浏览器里授权一个 Google 账号，只申请 Blogger；博客列表按账号缓存。
import { invoke } from '@tauri-apps/api/core';
import type { Connection } from '$lib/types';
import { app, say, refreshConns } from './app.svelte.js';

export type Blog = { id: string; name: string; url: string };

class BloggerState {
  /** 正在等浏览器授权 */
  busy = $state(false);
  /** connId → 博客列表 */
  blogs = $state<Record<string, Blog[]>>({});
  blogsErr = $state<Record<string, string>>({});
}
export const blogger = new BloggerState();

export const bloggerAccounts = () => app.conns.filter((c) => c.kind === 'blogger');

/** 授权一个 Blogger 账号；updateId 非空时重新授权那一条。 */
export async function bloggerLogin(updateId: string) {
  if (blogger.busy) return;
  blogger.busy = true;
  say('在浏览器里选要发博客的 Google 账号，勾上 Blogger 权限后点「继续」');
  try {
    const c = await invoke<Connection>('blogger_login', { updateId });
    await refreshConns();
    delete blogger.blogs[c.id];
    say(`已接上 Blogger：${c.login || c.name}`);
  } catch (e) {
    if (!String(e).startsWith('已取消')) say(String(e), 'err');
  } finally { blogger.busy = false; }
}

export function cancelBloggerLogin() { invoke('google_oauth_cancel').catch(() => {}); }

export async function loadBlogs(connId: string, force = false) {
  if (!connId || (!force && blogger.blogs[connId])) return;
  try {
    const list = await invoke<Blog[]>('blogger_blogs', { connId });
    blogger.blogs = { ...blogger.blogs, [connId]: list };
    blogger.blogsErr = { ...blogger.blogsErr, [connId]: '' };
  } catch (e) { blogger.blogsErr = { ...blogger.blogsErr, [connId]: String(e) }; }
}
