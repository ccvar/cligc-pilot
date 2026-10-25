// 安卓手机连接状态：左上角的手机图标和设置里的「安卓手机」共用。窗口在前台时每 15 秒查一次（adb devices，很轻）。
import { invoke } from '@tauri-apps/api/core';
import { untrack } from 'svelte';

export type PhoneDevice = { serial: string; state: string; model: string; stay_on: boolean };
export type PhoneStatus = { adb: string; managed: boolean; devices: PhoneDevice[]; error: string };

class PhoneState {
  status = $state<PhoneStatus | null>(null);
  loading = $state(false);
  /** 设置里滚到哪一组（从左上角的手机图标、或「+」菜单进来） */
  focus = $state<'' | 'phone' | 'mail'>('');
  /** 还没装 adb / 没接邮箱时，组默认不显示；从「+」菜单选了才显示 */
  showPhone = $state(false);
  showMail = $state(false);
  /** 可以用的手机（已授权） */
  ready = $derived(this.status?.devices.filter((d) => d.state === 'device') ?? []);
  /** 连着但还没在手机上点「允许」 */
  pending = $derived(this.status?.devices.filter((d) => d.state === 'unauthorized') ?? []);
}
export const phone = new PhoneState();

/** 「+」菜单里选「安卓手机」/「邮箱验证码」：显示那一组、展开、滚过去。 */
export function showPhoneGroup(which: 'phone' | 'mail') {
  if (which === 'phone') phone.showPhone = true; else phone.showMail = true;
  phone.focus = which;
}

export async function loadPhone() {
  if (phone.loading) return;
  phone.loading = true;
  try { phone.status = await invoke<PhoneStatus>('phone_status'); } catch { /* 查不到就保持上次的 */ }
  finally { phone.loading = false; }
}

export function initPhone() {
  $effect(() => {
    // untrack：loadPhone 会读写 phone.loading，放在 effect 里直接调会让 effect 无限重跑
    untrack(() => { void loadPhone(); });
    // 没装 adb 时不轮询（装好后设置里会刷新）
    const t = setInterval(() => { if (phone.status?.adb && document.visibilityState === 'visible') void loadPhone(); }, 15_000);
    const onVis = () => { if (document.visibilityState === 'visible' && phone.status?.adb) void loadPhone(); };
    document.addEventListener('visibilitychange', onVis);
    return () => { clearInterval(t); document.removeEventListener('visibilitychange', onVis); };
  });
}
