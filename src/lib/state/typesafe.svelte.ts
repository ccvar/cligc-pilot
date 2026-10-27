// 判断模型（TypeSafe Jev）接没接上：托管方案的「上线前内容质检」「跳过无关搜索词」、站点定位卡上的提示都看它。
// 设置里保存 / 删除 Key 后由 TypeSafeSection 更新，其余地方第一次用到时读一次。
import { invoke } from '@tauri-apps/api/core';
import type { TypeSafeStatus } from '$lib/types';

export const jev = $state({ configured: false, loaded: false });

let loading: Promise<void> | null = null;
export function loadJev(force = false): Promise<void> {
  if (jev.loaded && !force) return Promise.resolve();
  loading ??= invoke<TypeSafeStatus>('typesafe_status')
    .then((s) => { jev.configured = s.configured; })
    .catch(() => { /* 读不到就当没接上：所有地方都按原来的规则走 */ })
    .finally(() => { jev.loaded = true; loading = null; });
  return loading;
}
export function setJevConfigured(on: boolean) { jev.configured = on; jev.loaded = true; }
