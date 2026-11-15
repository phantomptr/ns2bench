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
export function setToken(value) { token = value.trim(); session.set(token); location.reload(); }
export function authURL(path) {
  const url = new URL(path, location.href);
  if (token && url.host === location.host) url.searchParams.set('token', token);
  return url.href;
}
export function apiFetch(path, init = {}) {
  const headers = new Headers(init.headers);
  if (token) headers.set('Authorization', `Bearer ${token}`);
  return fetch(path, { ...init, headers });
}
