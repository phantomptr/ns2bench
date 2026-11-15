// Same-origin API authentication, shared by HTTP, MJPEG and WebSocket clients.
// A token in the page URL (the desktop app passes one) is kept for this session
// and removed from the address bar.
const KEY = 'ns2bench.token';
const session = {
  get() { try { return sessionStorage.getItem(KEY) || ''; } catch { return ''; } },
  set(v) { try { sessionStorage.setItem(KEY, v); } catch { /* storage blocked: memory only */ } },
};
let token = session.get();
const fromUrl = new URLSearchParams(location.search).get('token');
if (fromUrl) {
  token = fromUrl;
  session.set(token);
  const u = new URL(location.href);
  u.searchParams.delete('token');
  history.replaceState(null, '', u);
}
// The key that makes this page the person at the bench (can drive by socket, turn Block agents
// off). The desktop app gives it to its own window; with `ns2bench serve` it comes in the link
// the engine prints (?ui=…) and is kept for this tab only, out of the address bar.
const uiKey = {
  get() { try { return sessionStorage.getItem('ns2bench.ui') || ''; } catch { return ''; } },
  set(v) { try { sessionStorage.setItem('ns2bench.ui', v); } catch { /* storage blocked: this page load only */ } },
};
const uiFromUrl = new URLSearchParams(location.search).get('ui');
if (uiFromUrl) {
  uiKey.set(uiFromUrl);
  const u = new URL(location.href);
  u.searchParams.delete('ui');
  history.replaceState(null, '', u);
}
const UI = typeof window.__NS2BENCH_UI__ === 'string' ? window.__NS2BENCH_UI__ : (uiFromUrl || uiKey.get());
export function setToken(value) { token = value.trim(); session.set(token); location.reload(); }
export function authURL(path) {
  const url = new URL(path, location.href);
  if (token && url.host === location.host) url.searchParams.set('token', token);
  if (UI && url.host === location.host && url.pathname === '/ws') url.searchParams.set('ui', UI);
  return url.href;
}
export function apiFetch(path, init = {}) {
  const headers = new Headers(init.headers);
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (UI) headers.set('X-NS2Bench-UI', UI);
  // This page is the person at the bench, not an agent (the engine checks that claim).
  if (!headers.has('X-NS2Bench-Source')) headers.set('X-NS2Bench-Source', 'ui');
  return fetch(path, { ...init, headers });
}
