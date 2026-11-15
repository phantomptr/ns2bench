// Remember the person's choices (theme, text size, language, notifications, layout…) in the
// engine's folder (~/.ns2bench/ui.json), like ps5upload keeps its settings in a file: the same
// in the desktop app and in a browser, and still there if the browser's storage is cleared.
// localStorage stays the fast copy every module reads; this keeps the two in step.
import { apiFetch } from './client.js';

const mine = (k) => typeof k === 'string' && k.startsWith('ns2bench.');
function snapshot() {
  const out = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (mine(k)) out[k] = localStorage.getItem(k);
    }
  } catch { /* storage blocked */ }
  return out;
}
let timer = 0, ready = false;
const save = () => apiFetch('/api/prefs', { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(snapshot()) }).catch(() => {});
// Nothing is saved before the saved file has been read: the defaults a fresh browser writes
// while starting must not replace the person's choices.
const later = () => { if (!ready) return; clearTimeout(timer); timer = setTimeout(save, 400); };

// Every change the page stores is saved to the engine shortly after.
try {
  const { setItem, removeItem } = Storage.prototype;
  Storage.prototype.setItem = function (k, v) { setItem.call(this, k, v); if (this === localStorage && mine(k)) later(); };
  Storage.prototype.removeItem = function (k) { removeItem.call(this, k); if (this === localStorage && mine(k)) later(); };
} catch { /* storage blocked */ }
addEventListener('pagehide', () => { if (timer) { clearTimeout(timer); save(); } });

// At start: the saved file wins. If this browser had something else (a new browser, cleared
// storage, or changes made in the app), take the saved values and load once more with them.
apiFetch('/api/prefs').then((r) => (r.ok ? r.json() : null)).then((saved) => {
  if (!saved) { window.__ns2benchPrefs = 'ready'; return; }
  ready = true;
  const local = snapshot();
  const differs = Object.keys(saved).filter((k) => mine(k) && local[k] !== saved[k]);
  if (!Object.keys(saved).length || Object.keys(local).some((k) => !(k in saved))) later();   // first run, or new local-only choices
  if (!differs.length) { window.__ns2benchPrefs = 'ready'; return; }
  let again = false;
  try {
    again = sessionStorage.getItem('ns2bench.prefs.loaded') === '1';
    sessionStorage.setItem('ns2bench.prefs.loaded', '1');
    for (const k of differs) Storage.prototype.setItem.call(localStorage, k, saved[k]);
  } catch { window.__ns2benchPrefs = 'ready'; return; }
  if (!again) location.reload(); else window.__ns2benchPrefs = 'ready';
}).catch(() => { window.__ns2benchPrefs = 'ready'; });
