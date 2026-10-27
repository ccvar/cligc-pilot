<script lang="ts">
  // 输入框里的「+ 添加文件」（和 Claude 客户端一样，也可以直接拖进来或粘贴）：选好的文件交给 onfiles。
  let { onfiles, disabled = false }: { onfiles: (files: File[]) => void; disabled?: boolean } = $props();
  let input = $state<HTMLInputElement | undefined>();
</script>

<button type="button" class="attach-add" {disabled} onclick={() => input?.click()} aria-label="添加文件或图片" data-tip="添加文件或图片（也可以拖进来、粘贴）">
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
</button>
<input bind:this={input} type="file" multiple hidden
  onchange={(e) => { const files = Array.from(e.currentTarget.files ?? []); e.currentTarget.value = ''; if (files.length) onfiles(files); }} />

<style>
  .attach-add { width: 30px; height: 30px; display: inline-flex; align-items: center; justify-content: center; flex: none; border: none; background: none; border-radius: 8px; color: var(--dim); cursor: pointer; transition: background-color .12s, color .12s; }
  .attach-add:hover:not(:disabled) { background: var(--hover, rgba(0, 0, 0, .05)); color: var(--text); }
  .attach-add:disabled { opacity: .45; cursor: default; }
</style>
