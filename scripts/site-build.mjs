// Build the website into _site/: site/ as is, the version stamped in, and the FAQ and
// setup guide rendered from FAQ.md and docs/setup/README.md.
//   npm i --no-save marked@15.0.12 && node scripts/site-build.mjs
import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, '_site');
const repo = 'https://github.com/phantomptr/ns2bench';
const version = readFileSync(join(root, 'VERSION'), 'utf8').trim();

// GitHub-style heading ids, so README/FAQ anchors work on the site too.
export const slug = (s) => s.toLowerCase().replace(/<[^>]+>/g, '').replace(/[^\p{L}\p{N}\s-]/gu, '').trim().replace(/\s/g, '-');

// Links written for GitHub, rewritten for the site.
export function link(href) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  const [path, hash = ''] = href.split('#');
  const h = hash ? `#${hash}` : '';
  const file = path.replace(/^(\.\.\/|\.\/)+/, '').replace(/^docs\/setup\//, 'setup/');
  if (/(^|\/)setup\/(README\.md)?$/.test(file) || file === 'README.md' && href.startsWith('../')) return `/setup/${h}`;
  if (file === 'FAQ.md') return `/faq/${h}`;
  if (file === 'README.md') return hash === 'support' ? '/#thanks' : `${repo}#${hash}`;
  if (/\.svg$/.test(file)) return file.replace(/^setup\//, '');
  return `${repo}/blob/main/${path.startsWith('../') ? `docs/${file}` : file}${h}`;
}

function render(md) {
  const toc = [];
  const marked = new Marked({ gfm: true });
  marked.use({
    renderer: {
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        const id = slug(text);
        if (depth === 1) toc.push({ id, text });
        return `<h${depth} id="${id}">${text}</h${depth}>\n`;
      },
    },
    walkTokens(t) {
      if (t.type === 'link' || t.type === 'image') t.href = link(t.href);
    },
  });
  return { html: marked.parse(md), toc };
}

function page(title, description, body, depth) {
  const up = '../'.repeat(depth);
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title} · NS2Bench</title>
<meta name="description" content="${description}">
<meta name="color-scheme" content="light dark">
<meta property="og:image" content="https://ns2bench.phantomptr.com/assets/og.png">
<link rel="icon" href="${up}assets/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="${up}style.css">
<script>try{const t=localStorage.getItem('ns2bench.site.theme');if(t)document.documentElement.dataset.theme=t;}catch{}</script>
</head>
<body>
<header class="top">
  <a class="brand" href="${up}" aria-label="NS2Bench home"><span class="mark" aria-hidden="true"></span><span>NS2Bench</span></a>
  <nav aria-label="Site"><a href="${up}#features">Features</a><a href="${up}setup/">Setup</a><a href="${up}faq/">FAQ</a><a href="${up}#download">Download</a><a href="${up}#thanks">Support</a><a href="${repo}" rel="noopener">GitHub</a></nav>
  <button id="theme" type="button" aria-label="Switch between light and dark" title="Switch between light and dark">◐</button>
</header>
<main class="doc">
${body}
</main>
<footer><p>MIT License · <a href="${repo}">GitHub</a> · <a href="${up}faq/">FAQ</a> · <a href="${up}setup/">Setup</a></p>
<p class="small">Not affiliated with or endorsed by Nintendo. Nintendo Switch is a trademark of Nintendo.</p></footer>
<script src="${up}app.js"></script>
</body>
</html>
`;
}

function main() {
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
  const setup = render(readFileSync(join(root, 'docs/setup/README.md'), 'utf8'));
  writeFileSync(join(out, 'setup/index.html'),
    page('Setup guide', 'How to connect a Nintendo Switch 2, a capture card and an input device to NS2Bench.', setup.html, 1));

  const faq = render(readFileSync(join(root, 'FAQ.md'), 'utf8'));
  const toc = `<h1 id="faq">Frequently asked questions</h1>\n<nav class="toc" aria-label="FAQ sections"><p>${faq.toc.map((t) => `<a href="#${t.id}">${t.text}</a>`).join(' · ')}</p></nav>\n`;
  mkdirSync(join(out, 'faq'), { recursive: true });
  writeFileSync(join(out, 'faq/index.html'),
    page('FAQ', 'Answers about NS2Bench setup, capture, input devices, the offline network, agents and updates.', toc + faq.html, 1));
  console.log(`site built: _site/ (v${version})`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
