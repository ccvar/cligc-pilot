// 系统滚动条宽度写到 :root 的 --sb（总显示滚动条时约 15px，自动隐藏的覆盖式滚动条为 0）。
// 站点页面的内容区一直给滚动条留位置（scrollbar-gutter: stable），右边距要扣掉这一截，左右才一样宽（见 site.css .page-body）。
export function initScrollbarWidth() {
  const measure = () => {
    const d = document.createElement('div');
    d.style.cssText = 'position:absolute;top:-9999px;width:100px;height:100px;overflow:scroll';
    document.body.appendChild(d);
    document.documentElement.style.setProperty('--sb', `${d.offsetWidth - d.clientWidth}px`);
    d.remove();
  };
  measure();
  // 系统设置里改了「显示滚动条」，切回窗口时重新量
  window.addEventListener('focus', measure);
}
