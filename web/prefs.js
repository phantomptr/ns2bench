// Remember the person's choices (theme, text size, language, notifications, layout…) in the
// engine's folder (~/.ns2bench/ui.json), like ps5upload keeps its settings in a file: the same
// in the desktop app and in a browser, and still there if the browser's storage is cleared.
// localStorage stays the fast copy every module reads; this keeps the two in step.
//
// Only the page that is the person at the bench (it has the app's key, or the link
// `ns2bench serve` printed) reads and writes the file. A watch-only tab keeps its own choices.
// Only changed keys are sent and the engine merges them, so two open windows don't undo each other.
import { apiFetch, hasUiKey } from './client.js';

const mine = (k) => typeof k === 'string' && k.startsWith('ns2bench.');
const local = () => {
  const out = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (mine(k)) out[k] = localStorage.getItem(k);
    }
  } catch { /* storage blocked */ }
  return out;
};
const dirty = new Set();
const sent = {};                       // what the file holds, as far as this page knows
let timer = 0, ready = false, refused = false;
const save = () => {
  timer = 0;
  if (refused || !dirty.size) return Promise.resolve();
  const change = {};
  for (const k of dirty) {
    let v = null;
    try { v = localStorage.getItem(k); } catch { /* blocked */ }
    if (sent[k] !== v) change[k] = v;  // null: remove
  }
  dirty.clear();
  if (!Object.keys(change).length) return Promise.resolve();
  Object.assign(sent, change);
  return apiFetch('/api/prefs', { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(change) })
    .then((r) => { if (r.status === 403) refused = true; })
    .catch(() => { for (const k in change) { dirty.add(k); delete sent[k]; } });
};
// Nothing is saved before the file has been read: the defaults a fresh browser writes while
// starting must not replace the person's choices.
const later = (k) => { dirty.add(k); if (!ready) return; clearTimeout(timer); timer = setTimeout(save, 400); };

try {
  const { setItem, removeItem } = Storage.prototype;
  Storage.prototype.setItem = function (k, v) { setItem.call(this, k, v); if (this === localStorage && mine(k)) later(k); };
  Storage.prototype.removeItem = function (k) { removeItem.call(this, k); if (this === localStorage && mine(k)) later(k); };
} catch { /* storage blocked */ }
/** Save now (before a reload that must see the change). */
export const savePrefsNow = () => { clearTimeout(timer); return save(); };
addEventListener('pagehide', () => { if (timer) { clearTimeout(timer); save(); } });

let done;
/** Resolves once saved choices are applied: read stored state after this, not before. */
export const prefsReady = new Promise((r) => { done = () => { window.__ns2benchPrefs = 'ready'; r(); }; });

// At start: the saved file wins. If this browser had something else (a new browser, cleared
// storage, or changes made in another window), take the saved values and load once more.
if (!hasUiKey) done();
else apiFetch('/api/prefs').then((r) => (r.ok ? r.json() : null)).then((saved) => {
  if (!saved) return done();
  ready = true;
  for (const [k, v] of Object.entries(saved)) if (mine(k)) sent[k] = v;
  const here = local();
  // first run, or choices only this browser has: send them
  for (const k of Object.keys(here)) if (!(k in saved)) dirty.add(k);
  if (dirty.size) later([...dirty][0]);
  const differs = Object.keys(saved).filter((k) => mine(k) && here[k] !== saved[k]);
  let again = false;
  try {
    if (!differs.length) { sessionStorage.removeItem('ns2bench.prefs.loaded'); return done(); }
    again = sessionStorage.getItem('ns2bench.prefs.loaded') === '1';
    sessionStorage.setItem('ns2bench.prefs.loaded', '1');
    for (const k of differs) Storage.prototype.setItem.call(localStorage, k, saved[k]);
  } catch { return done(); }
  if (!again) location.reload(); else done();
}).catch(() => done());
