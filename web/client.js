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
  // (kept only when this tab has none yet: a link from another site must not swap a working key)
  if (!uiKey.get()) uiKey.set(uiFromUrl);
  const u = new URL(location.href);
  u.searchParams.delete('ui');
  history.replaceState(null, '', u);
}
const UI = typeof window.__NS2BENCH_UI__ === 'string' ? window.__NS2BENCH_UI__ : (uiKey.get() || uiFromUrl);
/** This page is the person at the bench (it can save preferences, drive, turn Block agents off). */
export const hasUiKey = !!UI;
export function setToken(value) { token = value.trim(); session.set(token); location.reload(); }
// Several consoles: the one this page shows. Its picture, sound, screenshots, clips and screen
// reading come from that console; '' is the engine's default (the one the controller is on).
let consoleId = (() => { try { return localStorage.getItem('ns2bench.console') || ''; } catch { return ''; } })();
export function currentConsole() { return consoleId; }
export function setConsole(id) {
  consoleId = id || '';
  try { if (consoleId) localStorage.setItem('ns2bench.console', consoleId); else localStorage.removeItem('ns2bench.console'); } catch { /* fine */ }
}
export function authURL(path) {
  const url = new URL(path, location.href);
  if (token && url.host === location.host) url.searchParams.set('token', token);
  if (UI && url.host === location.host && url.pathname === '/ws') url.searchParams.set('ui', UI);
  if (consoleId && url.host === location.host && ['/stream.mjpg', '/video', '/audio'].includes(url.pathname)) url.searchParams.set('console', consoleId);
  return url.href;
}
export function apiFetch(path, init = {}) {
  const headers = new Headers(init.headers);
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (UI) headers.set('X-NS2Bench-UI', UI);
  // This page is the person at the bench, not an agent (the engine checks that claim).
  if (!headers.has('X-NS2Bench-Source')) headers.set('X-NS2Bench-Source', 'ui');
  if (consoleId && !headers.has('X-NS2Bench-Console')) headers.set('X-NS2Bench-Console', consoleId);
  return fetch(path, { ...init, headers });
}
