// Publish only installers the desktop updater knows how to install.
import { createHash } from 'node:crypto';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
export function platformFor(name) {
  const arch = /(?:aarch64|arm64)/i.test(name) ? 'aarch64' : /(?:x64|x86_64|amd64)/i.test(name) ? 'x86_64' : null;
  if (!arch) return null;
  const os = name.endsWith('.dmg') ? 'darwin' : name.endsWith('-setup.exe') ? 'windows' : name.endsWith('.AppImage') ? 'linux' : null;
  return os ? `${os}-${arch}` : null;
}
export async function manifest(dir, tag) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error('Stable vMAJOR.MINOR.PATCH tag required');
  const assets = {}, sha256 = {};
  for (const name of await readdir(dir)) {
    const platform = platformFor(name); if (!platform) continue;
    if (assets[platform]) throw new Error(`Duplicate installer for ${platform}`);
    assets[platform] = `https://github.com/phantomptr/ns2bench/releases/download/${tag}/${encodeURIComponent(name)}`;
    sha256[platform] = createHash('sha256').update(await readFile(join(dir, name))).digest('hex');
  }
  for (const platform of ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'windows-aarch64', 'linux-x86_64']) {
    if (!assets[platform]) throw new Error(`Missing installer for ${platform}`);
  }
  return { version: tag.slice(1), pub_date: new Date().toISOString(), notes: process.env.RELEASE_NOTES_TEXT || `NS2Bench ${tag}. See the GitHub release for full release notes.`, assets, sha256 };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [dir, tag] = process.argv.slice(2);
  if (!dir || !tag) throw new Error('Usage: node scripts/release-manifest.mjs <installer-directory> <tag>');
  await writeFile(join(dir, 'latest.json'), JSON.stringify(await manifest(dir, tag), null, 2) + '\n');
}
