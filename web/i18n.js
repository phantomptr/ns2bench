// Interface language. English text is the key: the page is written in English and
// every text node or attribute whose English text has a translation is swapped in
// place, including text the app sets later (a MutationObserver watches the page).
// Locale files: /static/i18n/<code>.json = { "English text": "translation" }.
export const LANGUAGES = {
  en: 'English',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  ja: '日本語',
  ko: '한국어',
  es: 'Español',
  'pt-BR': 'Português (Brasil)',
  fr: 'Français',
  de: 'Deutsch',
  it: 'Italiano',
  ru: 'Русский',
};
const KEY = 'ns2bench.lang';
const ATTRS = ['title', 'aria-label', 'placeholder'];
let dict = {};
let current = 'en';
// Original English per node/attribute, so switching languages never compounds.
const source = new WeakMap();
// The language before this one, translated text -> English: text the app set through t() is
// already translated when it reaches the page, and would otherwise stay in the old language.
let reverse = new Map();
const english = (text) => {
  const key = text.replace(/\s+/g, ' ').trim();
  if (key in dict || !reverse.has(key)) return text;
  return text.match(/^\s*/)[0] + reverse.get(key) + text.match(/\s*$/)[0];
};

const store = {
  get() { try { return localStorage.getItem(KEY); } catch { return null; } },
  set(v) { try { localStorage.setItem(KEY, v); } catch { /* private mode */ } },
};

export function t(english) {
  return dict[english] ?? english;
}
export const language = () => current;

export function pick(wanted = navigator.languages || [navigator.language]) {
  for (const w of wanted) {
    if (!w) continue;
    if (LANGUAGES[w]) return w;
    const lower = w.toLowerCase();
    if (lower.startsWith('zh')) return /tw|hk|mo|hant/.test(lower) ? 'zh-TW' : 'zh-CN';
    if (lower.startsWith('pt')) return 'pt-BR';
    const base = lower.split('-')[0];
    if (LANGUAGES[base]) return base;
  }
  return 'en';
}

function swapText(node) {
  const text = node.nodeValue;
  if (!text || !text.trim()) return;
  let orig = source.get(node);
  // Text the app replaced since we last saw it is new English source.
  if (orig === undefined || (text !== orig.text && text !== orig.shown)) orig = { text: english(text), shown: text };
  // Same normalisation as scripts/i18n.mjs: collapsed whitespace, trimmed.
  const key = orig.text.replace(/\s+/g, ' ').trim();
  const lead = orig.text.match(/^\s*/)[0], trail = orig.text.match(/\s*$/)[0];
  const shown = key in dict ? lead + dict[key] + trail : orig.text;
  orig.shown = shown;
  source.set(node, orig);
  if (text !== shown) node.nodeValue = shown;
}

function swapAttrs(el) {
  let origs = source.get(el);
  for (const a of ATTRS) {
    const v = el.getAttribute(a);
    if (v === null) continue;
    origs ??= {};
    const o = origs[a];
    if (!o || (v !== o.text && v !== o.shown)) origs[a] = { text: english(v), shown: v };
    const want = t(origs[a].text.replace(/\s+/g, ' ').trim());
    origs[a].shown = want;
    if (v !== want) el.setAttribute(a, want);
  }
  if (origs) source.set(el, origs);
}

export function translate(root = document.body) {
  if (!root) return;
  if (root.nodeType === Node.TEXT_NODE) return swapText(root);
  if (root.nodeType !== Node.ELEMENT_NODE) return;
  if (root.closest('pre, code, .logview, .log, [data-no-i18n]')) return;
  swapAttrs(root);
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: (n) => n.nodeType === Node.ELEMENT_NODE && n.matches('pre, code, script, style, .logview, .log, [data-no-i18n]')
      ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    if (n.nodeType === Node.TEXT_NODE) swapText(n); else swapAttrs(n);
  }
}

export async function setLanguage(code, { save = true } = {}) {
  if (!LANGUAGES[code]) code = 'en';
  let next = {};
  if (code !== 'en') {
    try {
      const r = await fetch(`/static/i18n/${code}.json`);
      if (r.ok) next = await r.json();
    } catch { /* stay readable in English */ }
  }
  reverse = new Map(Object.entries(dict).map(([en, shown]) => [shown, en]));
  dict = next;
  current = code;
  document.documentElement.lang = code;
  if (save) store.set(code);
  translate(document.body);
  document.dispatchEvent(new CustomEvent('ns2bench:language', { detail: code }));
}

let observing = false;
export async function initI18n() {
  await setLanguage(store.get() || pick(), { save: false });
  if (observing) return;
  observing = true;
  new MutationObserver((records) => {
    if (current === 'en') return;
    for (const r of records) {
      if (r.type === 'characterData') swapText(r.target);
      else if (r.type === 'attributes') swapAttrs(r.target);
      else r.addedNodes.forEach((n) => translate(n));
    }
  }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRS });
}
