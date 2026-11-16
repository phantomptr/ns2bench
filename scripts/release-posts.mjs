// Draft release announcements from CHANGELOG.md (`make release-post`).
//   node scripts/release-posts.mjs [X.Y.Z]   -> .release_posts/vX.Y.Z/ (git-ignored)
// Writes x-1-main.txt + x-2-reply.txt (280 chars; links count as 23), discord.md (2000
// chars), reddit-title.txt, reddit-body.md, and README.md saying what goes where.
// Drafts are for a person to read and post; nothing is posted from here.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { section } from './changelog.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const SITE = 'https://ns2bench.phantomptr.com';
const RELEASE = 'https://github.com/phantomptr/ns2bench/releases/latest';
const ICONS = { see: '👀', drive: '🎮', agents: '🤖', tests: '🧪', offline: '🔒', desktop: '🖥️' };

// X counts every link as 23 characters.
export const xLength = (t) => [...t.replace(/https?:\/\/\S+/g, 'x'.repeat(23))].length;

export function bullets(notes) {
  return [...notes.matchAll(/^- \*\*(.+?):\*\*\s*([\s\S]*?)(?=^- |\n\n|$(?![\s\S]))/gm)]
    .map((m) => ({ lead: m[1].trim(), text: m[2].replace(/\s+/g, ' ').trim() }));
}
const icon = (lead) => ICONS[Object.keys(ICONS).find((k) => lead.toLowerCase().includes(k))] || '✨';
const short = (text) => text.split(/[.:;(]/)[0].trim();

export function posts(version, notes) {
  const items = bullets(notes);
  const first = version === '0.0.1';
  const head = first ? '🎉 NS2Bench is out: see, hear and drive a Nintendo Switch 2 from your computer.'
    : `🎉 NS2Bench ${version} is out.`;
  let lines = items.map((b) => `${icon(b.lead)} ${b.lead}`);
  let x1 = '';
  for (let n = lines.length; n >= 0; n--) {
    x1 = [head, '', ...lines.slice(0, n), '', 'Free & open source 👇', RELEASE].join('\n');
    if (xLength(x1) <= 280) break;
  }
  const x2 = [`By hand or by AI agents (MCP), and the console stays offline. Setup guide for Linux boards, Raspberry Pi Pico and CH9329:`, `${SITE}/setup/`].join('\n');
  const discord = [`## NS2Bench ${version}`, '', first ? 'See, hear and drive a Nintendo Switch 2 from your computer, by hand or by AI agents. The console stays offline.' : '',
    '', ...items.map((b) => `- **${b.lead}:** ${b.text}`), '', `**Download:** [GitHub release](<${RELEASE}>) · **Setup:** [guide](<${SITE}/setup/>) · **FAQ:** [answers](<${SITE}/faq/>)`]
    .filter((l, i, a) => !(l === '' && a[i - 1] === '')).join('\n');
  const redditTitle = first ? 'NS2Bench: an open-source bench to see, hear and drive a Nintendo Switch 2 from your PC (by hand or with AI agents, console stays offline)'
    : `NS2Bench ${version}: ${items.slice(0, 3).map((b) => short(b.lead).toLowerCase()).join(', ')}`;
  const redditBody = [first ? 'I made NS2Bench, a free and open-source desktop app (macOS, Windows, Linux) for working with a Switch 2 on your desk.' : `NS2Bench ${version} is out.`,
    '', ...items.map((b) => `- **${b.lead}:** ${b.text}`), '', `It doesn't modify the console or need homebrew: it sees what a TV sees and presses what a wired controller presses.`,
    '', `- Download: ${RELEASE}`, `- Setup guide: ${SITE}/setup/`, `- FAQ: ${SITE}/faq/`, '', 'Feedback and hardware reports (which boards and capture cards work for you) are very welcome.'].join('\n');
  return { x1, x2, discord, redditTitle, redditBody };
}

function main() {
  const version = process.argv[2] || readFileSync(join(root, 'VERSION'), 'utf8').trim();
  const notes = section(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), version);
  if (!notes) throw new Error(`no CHANGELOG section for ${version}`);
  const p = posts(version, notes);
  const dir = join(root, '.release_posts', `v${version}`);
  mkdirSync(dir, { recursive: true });
  const files = { 'x-1-main.txt': p.x1, 'x-2-reply.txt': p.x2, 'discord.md': p.discord, 'reddit-title.txt': p.redditTitle, 'reddit-body.md': p.redditBody };
  for (const [f, t] of Object.entries(files)) writeFileSync(join(dir, f), `${t}\n`);
  const sizes = [`x-1-main.txt: ${xLength(p.x1)}/280`, `x-2-reply.txt: ${xLength(p.x2)}/280 (reply to the first post)`,
    `discord.md: ${p.discord.length}/2000 (#announcements)`, `reddit-title.txt: ${p.redditTitle.length}/300`, 'reddit-body.md: Markdown body'];
  writeFileSync(join(dir, 'README.md'), `# Posts for v${version}\n\nRead, edit and post by hand. Add a screenshot from site/assets/shots/ to the X and Reddit posts.\n\n${sizes.map((s) => `- ${s}`).join('\n')}\n`);
  console.log(`drafts in .release_posts/v${version}/\n${sizes.join('\n')}`);
  if (xLength(p.x1) > 280 || xLength(p.x2) > 280 || p.discord.length > 2000) process.exit(1);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
