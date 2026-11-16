// Theme, language, text size and accessibility: applied at once, remembered per browser.
import { LANGUAGES, initI18n, language, setLanguage, t } from './i18n.js';

const $ = (s) => document.querySelector(s);
const THEMES = ['system', 'light', 'peach', 'dark', 'oled'];
const store = {
  get(k) { try { return localStorage.getItem(`ns2bench.${k}`); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(`ns2bench.${k}`, v); } catch { /* private mode */ } },
};
const dark = () => matchMedia('(prefers-color-scheme: dark)').matches;

export function applyTheme(theme) {
  if (!THEMES.includes(theme)) theme = 'system';
  if (theme === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  store.set('theme', theme);
  // Remember the last light and dark picks for the toolbar's light/dark toggle.
  if (theme === 'light' || theme === 'peach') store.set('theme.light', theme);
  if (theme === 'dark' || theme === 'oled') store.set('theme.dark', theme);
  for (const b of document.querySelectorAll('[data-theme-pick]')) b.setAttribute('aria-checked', String(b.dataset.themePick === theme));
}
const currentTheme = () => store.get('theme') || 'system';
const isDark = (t) => t === 'dark' || t === 'oled' || (t === 'system' && dark());

// The toolbar button flips light <-> dark and remembers which light (Light/Peach)
// and which dark (Dark/OLED) you picked last.
$('#btn-theme')?.addEventListener('click', () => {
  const t = currentTheme();
  if (isDark(t)) { if (t !== 'system') store.set('theme.dark', t); applyTheme(store.get('theme.light') || 'light'); }
  else { if (t !== 'system') store.set('theme.light', t); applyTheme(store.get('theme.dark') || 'dark'); }
});
for (const b of document.querySelectorAll('[data-theme-pick]')) b.addEventListener('click', () => applyTheme(b.dataset.themePick));
applyTheme(currentTheme());

const langs = $('#lang-select');
if (langs) {
  langs.replaceChildren(...Object.entries(LANGUAGES).map(([code, name]) => new Option(name, code)));
  langs.addEventListener('change', () => setLanguage(langs.value));
  document.addEventListener('ns2bench:language', (e) => { langs.value = e.detail; });
}
initI18n().then(() => { if (langs) langs.value = language(); });

// Text size: one zoom factor for the whole page (text, spacing and controls).
export const SCALES = [0.7, 0.8, 0.9, 1, 1.1, 1.25, 1.4];
export function applyScale(scale) {
  scale = SCALES.includes(Number(scale)) ? Number(scale) : 1;
  document.documentElement.style.zoom = scale === 1 ? '' : String(scale);
  store.set('scale', String(scale));
  for (const b of document.querySelectorAll('[data-scale]')) b.setAttribute('aria-pressed', String(Number(b.dataset.scale) === scale));
}
const scales = $('#scale-choices');
if (scales) {
  scales.replaceChildren(...SCALES.map((v) => {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'choice'; b.dataset.scale = String(v);
    b.textContent = `${Math.round(v * 100)}%`;
    if (v === 1) { const d = document.createElement('span'); d.className = 'dim'; d.textContent = t('default'); b.append(' ', d); }
    b.addEventListener('click', () => applyScale(v));
    return b;
  }));
}
applyScale(store.get('scale'));

// Accessibility: data attributes on <html> that style.css reads.
const MOTION = ['auto', 'full', 'reduced', 'none'];
function applyMotion(m) {
  if (!MOTION.includes(m)) m = 'auto';
  if (m === 'auto') delete document.documentElement.dataset.motion; else document.documentElement.dataset.motion = m;
  store.set('motion', m);
  for (const b of document.querySelectorAll('#motion-choices [data-motion]')) b.setAttribute('aria-pressed', String(b.dataset.motion === m));
}
for (const b of document.querySelectorAll('#motion-choices [data-motion]')) b.addEventListener('click', () => applyMotion(b.dataset.motion));
applyMotion(store.get('motion'));
for (const [id, key, attr, on] of [['#a11y-transparency', 'transparency', 'transparency', 'reduced'], ['#a11y-contrast', 'contrast', 'contrast', 'high'], ['#a11y-dyslexia', 'dyslexia', 'dyslexia', 'true']]) {
  const set = (v) => { if (v) document.documentElement.dataset[attr] = on; else delete document.documentElement.dataset[attr]; store.set(key, String(v)); };
  const box = $(id);
  set(store.get(key) === 'true');
  if (box) { box.checked = store.get(key) === 'true'; box.addEventListener('change', () => set(box.checked)); }
}
