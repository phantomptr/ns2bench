// Theme toggle and platform-aware download links. Assets follow publish.yml's names.
(() => {
  const root = document.documentElement;
  const btn = document.getElementById('theme');
  const dark = () => (root.dataset.theme || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')) === 'dark';
  btn?.addEventListener('click', () => {
    const next = dark() ? 'light' : 'dark';
    root.dataset.theme = next;
    try { localStorage.setItem('ns2bench.site.theme', next); } catch { /* private mode */ }
  });
  const version = document.querySelector('meta[name="ns2bench-version"]')?.content;
  if (!version) return;
  const base = `https://github.com/phantomptr/ns2bench/releases/download/v${version}/`;
  for (const a of document.querySelectorAll('[data-asset]')) a.href = `${base}NS2Bench-${version}-${a.dataset.asset}`;
  for (const a of document.querySelectorAll('[data-file]')) a.href = base + a.dataset.file;
  const ua = navigator.userAgent;
  const asset = /Windows/.test(ua) ? 'win-x64-setup.exe' : /Mac/.test(ua) ? 'mac-arm64.dmg' : /Linux/.test(ua) ? 'linux-x64.AppImage' : null;
  const main = document.getElementById('dl-main');
  if (main && asset) {
    main.href = `${base}NS2Bench-${version}-${asset}`;
    main.textContent = /Windows/.test(ua) ? 'Download for Windows' : /Mac/.test(ua) ? 'Download for Mac' : 'Download for Linux';
  }
})();
