// Theme and language: applied at once, remembered per browser.
import { LANGUAGES, initI18n, language, setLanguage } from './i18n.js';

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
  const select = $('#theme-select');
  if (select) select.value = theme;
}
const currentTheme = () => store.get('theme') || 'system';
const isDark = (t) => t === 'dark' || t === 'oled' || (t === 'system' && dark());

// The toolbar button flips light <-> dark and remembers which light (Light/Peach)
// and which dark (Dark/OLED) you picked last, like ps5upload.
$('#btn-theme')?.addEventListener('click', () => {
  const t = currentTheme();
  if (isDark(t)) { if (t !== 'system') store.set('theme.dark', t); applyTheme(store.get('theme.light') || 'light'); }
  else { if (t !== 'system') store.set('theme.light', t); applyTheme(store.get('theme.dark') || 'dark'); }
});
$('#theme-select')?.addEventListener('change', (e) => applyTheme(e.target.value));
applyTheme(currentTheme());

const langs = $('#lang-select');
if (langs) {
  langs.replaceChildren(...Object.entries(LANGUAGES).map(([code, name]) => new Option(name, code)));
  langs.addEventListener('change', () => setLanguage(langs.value));
  document.addEventListener('ns2bench:language', (e) => { langs.value = e.detail; });
}
initI18n().then(() => { if (langs) langs.value = language(); });
