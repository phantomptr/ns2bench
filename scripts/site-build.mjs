// Build the website into _site/: site/ as is, the version stamped in, and the FAQ and
// setup guide rendered from Markdown in every language (English from FAQ.md and
// docs/setup/README.md, the others from docs/i18n/<code>/FAQ.md and setup.md, English where a
// translation is missing). The site's own words are checked first (scripts/site-i18n.mjs), so
// a language that misses a key fails the build.
//   npm i --no-save --no-package-lock marked@15.0.12 && node scripts/site-build.mjs
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';
import { LANGS, check } from './site-i18n.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, '_site');
const repo = 'https://github.com/phantomptr/ns2bench';
const site = 'https://ns2bench.phantomptr.com';

// GitHub-style heading ids, so README/FAQ anchors work on the site too.
export const slug = (s) => s.toLowerCase().replace(/<[^>]+>/g, '').replace(/[^\p{L}\p{N}\s-]/gu, '').trim().replace(/\s/g, '-');

const esc = (s) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');

// Links written for GitHub, rewritten for the site. `from` is the Markdown file's folder in the
// repo ('' for FAQ.md, 'docs/setup', 'docs/i18n/de').
export function link(href, from = '', englishFrom = null) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  const [path, hash = ''] = href.split('#');
  const h = hash ? `#${hash}` : '';
  let file = posix.normalize(posix.join(from, path)).replace(/^(\.\.\/)+/, '');
  // A translation that kept the English file's relative link: resolve it as English would.
  if (!existsSync(join(root, file)) && from.startsWith('docs/i18n/')) {
    // Try the English original's folder first (FAQ.md: repo root; setup: docs/setup).
    const order = englishFrom === null ? ['docs/setup', ''] : [englishFrom, ...['docs/setup', ''].filter((d) => d !== englishFrom)];
    for (const alt of order) {
      const f = posix.normalize(posix.join(alt, path)).replace(/^(\.\.\/)+/, '');
      if (existsSync(join(root, f))) { file = f; break; }
    }
  }
  if (/^docs\/setup\/(README\.md)?$/.test(file) || /^docs\/i18n\/[^/]+\/setup\.md$/.test(file)) return `/setup/${h}`;
  if (file === 'FAQ.md' || /^docs\/i18n\/[^/]+\/FAQ\.md$/.test(file)) return `/faq/${h}`;
  if (file === 'README.md') return hash === 'support' ? '/#thanks' : `${repo}#${hash}`;
  if (/^docs\/setup\/[^/]+\.svg$/.test(file)) return `/setup/${posix.basename(file)}`;
  return `${repo}/blob/main/${file}${h}`;
}

/** Markdown to HTML. `ids` (the English file's heading ids, in order) names a translation's
 *  headings by position, so #anchors work in every language. `dropTitle` leaves out a leading
 *  # heading (the page header has the title); `tocDepth` picks the headings listed in the TOC. */
export function render(md, { from = '', englishFrom = null, ids = null, dropTitle = false, tocDepth = 1 } = {}) {
  const toc = [];
  const headingIds = [];
  let n = 0;
  if (dropTitle) md = md.replace(/^\s*#\s[^\n]*\n/, '');
  const marked = new Marked({ gfm: true });
  marked.use({
    renderer: {
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        const id = ids?.[n] ?? slug(text);
        n += 1;
        headingIds.push(id);
        if (depth === tocDepth) toc.push({ id, text });
        return `<h${depth} id="${id}">${text}</h${depth}>\n`;
      },
    },
    walkTokens(t) {
      if (t.type === 'link' || t.type === 'image') t.href = link(t.href, from, englishFrom);
    },
  });
  return { html: marked.parse(md), toc, ids: headingIds };
}

// The head script: the language (?lang=, saved choice, browser) and theme before anything is
// drawn. The same as site/index.html's.
const headScript = `<script>
  (function () {
    var codes = ${JSON.stringify(LANGS.map(([c]) => c))};
    var pick = function (c) {
      if (!c) return null;
      c = String(c).toLowerCase();
      for (var i = 0; i < codes.length; i++) if (codes[i].toLowerCase() === c) return codes[i];
      if (/^zh-(tw|hk|mo|hant)/.test(c)) return "zh-TW";
      if (/^zh/.test(c)) return "zh-CN";
      if (/^pt/.test(c)) return "pt-BR";
      var p = c.split("-")[0];
      return codes.indexOf(p) >= 0 ? p : null;
    };
    var lang = null, root = document.documentElement;
    try { lang = pick(new URLSearchParams(location.search).get("lang")) || pick(localStorage.getItem("ns2bench.site.lang")); } catch (e) {}
    if (!lang) {
      var nav = navigator.languages || [navigator.language];
      for (var j = 0; j < nav.length && !lang; j++) lang = pick(nav[j]);
    }
    lang = lang || "en";
    root.lang = lang;
    if (/[?&]lang=/.test(location.search)) {
      var link = document.getElementById("canonical");
      if (link) link.href = link.href.split("?")[0] + "?lang=" + lang;
    }
    if (lang !== "en") {
      root.classList.add("i18n-wait");
      setTimeout(function () { root.classList.remove("i18n-wait"); }, 2500);
    }
    try { var t = localStorage.getItem("ns2bench.site.theme"); if (t) root.dataset.theme = t; } catch (e) {}
  })();
</script>`;

const githubIcon = '<path fill="currentColor" d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.56-.29-5.25-1.28-5.25-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.7 5.39-5.27 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z"/>';

/** A page with the home page's header, footer, theme switch and language picker around `main`.
 *  `key` names its words in i18n/<code>.json (<key>.meta_title, .title, ...). */
export function page({ key, path, title, description, kicker, lead, main }) {
  const up = '../'.repeat(path.split('/').filter(Boolean).length);
  const cur = (p) => (p === path ? ' aria-current="page"' : '');
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title data-i18n="${key}.meta_title">${title} · NS2Bench</title>
<meta name="description" data-i18n-attr="content:${key}.meta_description" content="${esc(description)}">
<meta name="color-scheme" content="light dark">
<meta property="og:title" content="${title} · NS2Bench">
<meta property="og:description" content="${esc(description)}">
<meta property="og:image" content="${site}/assets/og.png">
<meta property="og:url" content="${site}/${path}">
<meta name="twitter:card" content="summary_large_image">
<link rel="canonical" id="canonical" href="${site}/${path}">
<link rel="icon" href="${up}assets/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="${up}style.css">
${headScript}
</head>
<body>
<a class="skip" href="#main" data-i18n="nav.skip">Skip to content</a>

<header class="top">
  <div class="wrap top-row">
    <a class="brand" href="${up}" data-i18n-attr="aria-label:nav.home" aria-label="NS2Bench home"><span class="mark" aria-hidden="true"></span><span>NS2Bench</span></a>
    <nav class="nav" data-i18n-attr="aria-label:nav.sections" aria-label="Sections">
      <a href="${up}#features" data-i18n="nav.features">Features</a>
      <a href="${up}setup/"${cur('setup/')} data-i18n="nav.setup">Setup</a>
      <a href="${up}#agents" data-i18n="nav.agents">Agents</a>
      <a href="${up}faq/"${cur('faq/')} data-i18n="nav.faq">FAQ</a>
      <a href="${up}#download" data-i18n="nav.download">Download</a>
      <a href="${up}#thanks" data-i18n="nav.support">Support</a>
    </nav>
    <div class="top-actions">
      <a class="btn small coffee" href="${up}#thanks" data-i18n-attr="title:nav.coffee_title" title="NS2Bench is free. A coffee for me, or tokens for the AI that writes the code, keeps it going.">☕ <span data-i18n="nav.coffee">Coffee or tokens</span></a>
      <label class="lang" data-i18n-attr="title:nav.language" title="Language">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm-9-9h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3Z"/></svg>
        <select id="lang" data-i18n-attr="aria-label:nav.language" aria-label="Language"><option value="en">English</option></select>
      </label>
      <a class="icon-btn" href="${repo}" data-i18n-attr="aria-label:nav.github" aria-label="NS2Bench on GitHub" title="GitHub">
        <svg viewBox="0 0 24 24" aria-hidden="true">${githubIcon}</svg>
      </a>
      <button class="icon-btn" id="theme" type="button" data-i18n-attr="aria-label:nav.theme_label;title:nav.theme_title" aria-label="Switch light or dark" title="Light or dark">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></svg>
      </button>
    </div>
  </div>
</header>

<main id="main">
  <section class="doc-hero" aria-labelledby="page-title">
    <div class="hero-glow" aria-hidden="true"></div>
    <div class="wrap">
      <p class="kicker" data-i18n="${key}.kicker">${kicker}</p>
      <h1 id="page-title" data-i18n="${key}.title">${title}</h1>
      <p class="section-lead" data-i18n="${key}.lead">${lead}</p>
    </div>
  </section>
${main}
</main>

<footer class="foot">
  <div class="wrap foot-row">
    <span data-i18n="foot.made">Created by <a href="https://phantomptr.com/">PhantomPtr</a>. NS2Bench is free software under the GPL-3.0 license.</span>
    <nav class="foot-links" data-i18n-attr="aria-label:foot.links" aria-label="More">
      <a href="${repo}">GitHub</a>
      <a href="${up}setup/" data-i18n="nav.setup">Setup</a>
      <a href="${up}faq/" data-i18n="nav.faq">FAQ</a>
      <a href="${repo}/issues/new/choose" data-i18n="community.bug.title">Report a problem</a>
    </nav>
  </div>
  <div class="wrap foot-row">
    <span class="muted" data-i18n="foot.nintendo">Not affiliated with or endorsed by Nintendo. Nintendo Switch is a trademark of Nintendo.</span>
  </div>
</footer>
<script src="${up}app.js" defer></script>
</body>
</html>
`;
}

/** One language's TOC and article. */
function docBlock(lang, { html, toc }) {
  return `<div class="wrap doc-layout" data-doc lang="${lang}">
  <nav class="doc-toc" data-i18n-attr="aria-label:doc.toc" aria-label="On this page"><p data-i18n="doc.toc">On this page</p><ul>${toc.map((t) => `<li><a href="#${t.id}">${t.text}</a></li>`).join('')}</ul></nav>
  <article class="doc" lang="${lang}" data-lang="${lang}">
${html}  </article>
</div>`;
}

/** The English article (shown without JavaScript) and every translation in a <template>, so
 *  the page has one set of heading ids at a time. A missing translation falls back to English. */
function docMain(englishFile, translated, opts) {
  const en = render(readFileSync(join(root, englishFile), 'utf8'), { ...opts, from: dirname(englishFile) === '.' ? '' : dirname(englishFile) });
  const parts = [docBlock('en', en)];
  const found = [];
  for (const [code] of LANGS.slice(1)) {
    const file = join(root, 'docs/i18n', code, translated);
    if (!existsSync(file)) continue;
    const tr = render(readFileSync(file, 'utf8'), { ...opts, from: `docs/i18n/${code}`, englishFrom: dirname(englishFile) === '.' ? '' : dirname(englishFile), ids: en.ids });
    if (tr.ids.length !== en.ids.length) {
      console.warn(`warning: docs/i18n/${code}/${translated} has ${tr.ids.length} headings, English has ${en.ids.length}`);
    }
    parts.push(`<template data-lang="${code}">\n${docBlock(code, tr)}\n</template>`);
    found.push(code);
  }
  const note = `<div class="wrap"><p class="doc-note muted" data-doc-fallback hidden data-i18n="doc.untranslated">This page isn't translated into your language yet, so it's shown in English.</p></div>\n`;
  return { main: note + parts.join('\n'), found };
}

export const pages = {
  setup: {
    key: 'setup', path: 'setup/', title: 'Setup guide', kicker: 'Guide',
    description: 'How to connect a Nintendo Switch 2, a capture card and an input device to NS2Bench.',
    lead: 'How to connect a Nintendo Switch 2, a capture card and an input device. The console stays offline.',
  },
  faq: {
    key: 'faq', path: 'faq/', title: 'Frequently asked questions', kicker: 'FAQ',
    description: 'Answers about NS2Bench setup, capture, input devices, the offline network, agents and updates.',
    lead: 'Answers about setup, capture, input devices, the offline network, agents and updates.',
  },
};

export function build({ checkWords = true } = {}) {
  if (checkWords) check({ pageHtml: Object.values(pages).map((p) => page({ ...p, main: '' })) });
  // Download links name a release that exists: the published one when the workflow passes it
  // (VERSION on main is bumped before the release's files are built).
  const version = (process.env.NS2BENCH_SITE_VERSION || '').replace(/^v/, '').match(/^\d+\.\d+\.\d+$/)?.[0]
    ?? readFileSync(join(root, 'VERSION'), 'utf8').trim();
  rmSync(out, { recursive: true, force: true });
  cpSync(join(root, 'site'), out, { recursive: true });
  const index = readFileSync(join(out, 'index.html'), 'utf8')
    .replaceAll('__VERSION__', version)
    .replace('</head>', `<meta name="ns2bench-version" content="${version}">\n</head>`);
  writeFileSync(join(out, 'index.html'), index);

  mkdirSync(join(out, 'setup'), { recursive: true });
  for (const f of readdirSync(join(root, 'docs/setup')).filter((f) => f.endsWith('.svg'))) {
    cpSync(join(root, 'docs/setup', f), join(out, 'setup', f));
  }
  const setup = docMain('docs/setup/README.md', 'setup.md', { dropTitle: true, tocDepth: 2 });
  writeFileSync(join(out, 'setup/index.html'), page({ ...pages.setup, main: setup.main }));

  const faq = docMain('FAQ.md', 'FAQ.md', { tocDepth: 1 });
  mkdirSync(join(out, 'faq'), { recursive: true });
  writeFileSync(join(out, 'faq/index.html'), page({ ...pages.faq, main: faq.main }));
  const langs = (l) => (l.length ? l.join(', ') : 'none yet');
  console.log(`site built: _site/ (v${version}); setup translations: ${langs(setup.found)}; FAQ translations: ${langs(faq.found)}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) build();
