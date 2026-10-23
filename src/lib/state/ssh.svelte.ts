import type { Connection } from '$lib/types';
import { invoke } from '@tauri-apps/api/core';
import { tick } from 'svelte';
import { open } from '@tauri-apps/plugin-dialog';
import { say, refreshConns, app } from './app.svelte.js';
import { probeOs, selectConn } from './connections.svelte.js';
import { disposeTerm, workbench, startTerm } from './workbench.svelte.js';
import { sftp, sftpGo } from './sftp.svelte.js';

const SSH_BLANK = { name: '', host: '', port: 22, user: '', auth: 'password', password: '', keyPath: '', keyPass: '' };

class SshFormState {
  // ---------- 新建 / 编辑远程连接（SSH） ----------
  // 安全：密码/私钥口令只在这个表单的内存里过一下，随 connect_ssh/update_ssh 进钥匙串，绝不落盘。
  // 主机指纹走 TOFU：必须先「测试连接」拿到指纹、由你确认后才能保存（后端无指纹一律拒）。
  sshOpen = $state(false);
  sshTesting = $state(false);
  sshConnecting = $state(false);
  sshErr = $state('');
  sshFp = $state(''); // 试连拿到的主机指纹；非空 = 认证已通过、待你确认指纹
  sshEditId = $state(''); // 非空 = 编辑已有连接
  sshCredentialRepair = $state(false); // 从“凭据缺失”入口进入：必须补填，不能空保存
  sshOldFp = $state(''); // 编辑时：连接里存着的旧指纹，用来发现「机器的指纹变了」
  sshF = $state({ ...SSH_BLANK });
  sshF0 = $state({ ...SSH_BLANK }); // 编辑时的初始值：用来判断「有没有动过连接相关的字段」
  // 改了连接相关的字段（地址/端口/用户/认证方式/密钥路径/新密码）→ 必须重新试连拿指纹再存。
  // 只改名字则不必：那和「能不能连上」无关，别为了改个名逼用户去连一次机器。
  sshDirty = $derived(
    this.sshF.host.trim() !== this.sshF0.host || Number(this.sshF.port) !== Number(this.sshF0.port) ||
    this.sshF.user.trim() !== this.sshF0.user || this.sshF.auth !== this.sshF0.auth ||
    this.sshF.keyPath.trim() !== this.sshF0.keyPath || !!this.sshF.password || !!this.sshF.keyPass
  );
  sshRenameOnly = $derived(!!this.sshEditId && !this.sshDirty && !this.sshCredentialRepair);
  // 试连看到的指纹和存着的不一样 = 这台机器换了主机密钥（重装？也可能是中间人）——必须让人看见。
  sshFpChanged = $derived(!!this.sshEditId && !!this.sshFp && !!this.sshOldFp && this.sshFp !== this.sshOldFp);
  // 编辑时密码可以留空（＝沿用钥匙串里那条），新建时必须填。
  sshCanTest = $derived(
    !!this.sshF.host.trim() && !!this.sshF.user.trim() &&
    (this.sshF.auth === 'key' ? !!this.sshF.keyPath.trim() : (!!this.sshF.password || (!!this.sshEditId && !this.sshCredentialRepair && this.sshF0.auth === 'password')))
  );
}

export const sshForm = new SshFormState();

export const sshAuthOpts = [
  { value: 'password', label: '密码', sub: '用账号密码登录' },
  { value: 'key', label: '密钥', sub: '用私钥文件登录（更安全）' },
];
export function openSshConnect() {
  sshForm.sshOpen = true; sshForm.sshErr = ''; sshForm.sshFp = ''; sshForm.sshEditId = ''; sshForm.sshCredentialRepair = false; sshForm.sshOldFp = '';
  sshForm.sshF = { ...SSH_BLANK };
  sshForm.sshF0 = { ...SSH_BLANK };
}
export function openSshEdit(c: Connection) {
  sshForm.sshOpen = true; sshForm.sshErr = ''; sshForm.sshFp = ''; sshForm.sshEditId = c.id; sshForm.sshCredentialRepair = false; sshForm.sshOldFp = c.ssh_fingerprint || '';
  // 密码/口令不回显（我们自己也读不到明文以外的用途）——留空即保持钥匙串里那条不变。
  sshForm.sshF = {
    name: c.name, host: c.ssh_host ?? '', port: c.ssh_port || 22, user: c.ssh_user ?? '',
    auth: c.ssh_auth || 'password', password: '', keyPath: c.ssh_key_path ?? '', keyPass: '',
  };
  sshForm.sshF0 = { ...sshForm.sshF };
}
export function openSshCredentialRepair(c: Connection) {
  openSshEdit(c);
  sshForm.sshCredentialRepair = true;
}
export async function saveSshEdit() {
  if (sshForm.sshConnecting) return;
  if (sshForm.sshCredentialRepair && sshForm.sshF.auth === 'password' && !sshForm.sshF.password) {
    sshForm.sshErr = '请输入这条连接的密码，并先测试连接。';
    return;
  }
  sshForm.sshConnecting = true; sshForm.sshErr = '';
  try {
    const conn = await invoke<Connection>('update_ssh', {
      connId: sshForm.sshEditId,
      name: sshForm.sshF.name.trim(), host: sshForm.sshF.host.trim(), port: Number(sshForm.sshF.port) || 22,
      user: sshForm.sshF.user.trim(), auth: sshForm.sshF.auth, keyPath: sshForm.sshF.keyPath,
      // 留空＝不动钥匙串（只改了名字/地址时不必重填密码）
      secret: sshForm.sshF.auth === 'key' ? sshForm.sshF.keyPass : sshForm.sshF.password,
      // 只改名字时没试连，沿用原指纹
      fingerprint: sshForm.sshFp || sshForm.sshOldFp,
    });
    sshForm.sshOpen = false; say(`已更新「${conn.name}」`);
    await refreshConns();
    if (app.activeConnId === conn.id) {
      disposeTerm();
      void probeOs(conn.id);
      // 补录凭据后原地恢复工作台，不要求用户再点终端和文件区的刷新按钮。
      await tick();
      if (app.view === 'remote' && workbench.wb.term && workbench.termEl) void startTerm(conn.id, workbench.termEl);
      if (app.view === 'remote' && workbench.wb.files && sftp.sftpConnId === conn.id) void sftpGo(sftp.sftpPath);
    }
  } catch (e) { sshForm.sshErr = String(e); }
  finally { sshForm.sshConnecting = false; }
}
export async function pickSshKey() {
  try {
    const p = await open({ multiple: false, title: '选择私钥文件（如 ~/.ssh/id_ed25519）' });
    if (typeof p === 'string') sshForm.sshF.keyPath = p;
  } catch (e) { sshForm.sshErr = String(e); }
}
export async function testSsh() {
  if (sshForm.sshTesting || !sshForm.sshCanTest) return;
  sshForm.sshTesting = true; sshForm.sshErr = ''; sshForm.sshFp = '';
  try {
    const p = await invoke<{ fingerprint: string; auth_ok: boolean; error: string }>('verify_ssh', {
      host: sshForm.sshF.host.trim(), port: Number(sshForm.sshF.port) || 22, user: sshForm.sshF.user.trim(), auth: sshForm.sshF.auth,
      password: sshForm.sshF.password, keyPath: sshForm.sshF.keyPath, keyPass: sshForm.sshF.keyPass, expectFingerprint: '',
      // 编辑时密码/口令留空 = 用钥匙串里存着的那条来试（后端取，前端永远看不到明文）
      connId: sshForm.sshEditId,
    });
    sshForm.sshFp = p.fingerprint;
  } catch (e) { sshForm.sshErr = String(e); }
  finally { sshForm.sshTesting = false; }
}
export async function confirmSsh() {
  if (sshForm.sshConnecting || !sshForm.sshFp) return;
  sshForm.sshConnecting = true; sshForm.sshErr = '';
  try {
    const conn = await invoke<Connection>('connect_ssh', {
      name: sshForm.sshF.name.trim(), host: sshForm.sshF.host.trim(), port: Number(sshForm.sshF.port) || 22,
      user: sshForm.sshF.user.trim(), auth: sshForm.sshF.auth, keyPath: sshForm.sshF.keyPath,
      secret: sshForm.sshF.auth === 'key' ? sshForm.sshF.keyPass : sshForm.sshF.password,
      fingerprint: sshForm.sshFp,
    });
    sshForm.sshOpen = false; say(`已连接「${conn.name}」`);
    await refreshConns(); await selectConn(conn.id);
    app.view = 'remote';
  } catch (e) { sshForm.sshErr = String(e); }
  finally { sshForm.sshConnecting = false; }
}
