// Publish only installers the desktop updater knows how to install.
import { createHash, createPrivateKey, sign } from 'node:crypto';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
export function platformFor(name) {
  const arch = /(?:aarch64|arm64)/i.test(name) ? 'aarch64' : /(?:x64|x86_64|amd64)/i.test(name) ? 'x86_64' : null;
  if (!arch) return null;
  const os = name.endsWith('.dmg') ? 'darwin' : name.endsWith('-setup.exe') ? 'windows' : name.endsWith('.AppImage') ? 'linux' : null;
  return os ? `${os}-${arch}` : null;
}
// What the app verifies for each platform (native/crates/app/src/updates.rs signed_text).
export const signedText = (version, platform, url, hash) => `ns2bench update\n${version}\n${platform}\n${url}\n${hash}\n`;
// keyPem: the Ed25519 release key (CI secret NS2BENCH_UPDATE_KEY). No key, no manifest: an
// unsigned release would be refused by every app that checks signatures.
export async function manifest(dir, tag, keyPem = process.env.NS2BENCH_UPDATE_KEY) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error('Stable vMAJOR.MINOR.PATCH tag required');
  if (!keyPem) throw new Error('NS2BENCH_UPDATE_KEY (the release signing key) is not set');
  const key = createPrivateKey(keyPem);
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
  const version = tag.slice(1), signatures = {};
  for (const p of Object.keys(assets)) signatures[p] = sign(null, Buffer.from(signedText(version, p, assets[p], sha256[p])), key).toString('base64');
  return { version, pub_date: new Date().toISOString(), notes: process.env.RELEASE_NOTES_TEXT || `NS2Bench ${tag}. See the GitHub release for full release notes.`, assets, sha256, signatures };
}
// The release's SHA256SUMS, signed: the app checks it before installing the board daemon, the
// Pico firmware or the network helper (each listed there by checksum).
export function signFile(text, keyPem = process.env.NS2BENCH_UPDATE_KEY) {
  if (!keyPem) throw new Error('NS2BENCH_UPDATE_KEY (the release signing key) is not set');
  return sign(null, Buffer.from(text), createPrivateKey(keyPem)).toString('base64');
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === 'sign') {
    // node scripts/release-manifest.mjs sign release/SHA256SUMS  ->  release/SHA256SUMS.sig
    const file = process.argv[3];
    if (!file) throw new Error('Usage: node scripts/release-manifest.mjs sign <file>');
    await writeFile(`${file}.sig`, signFile(await readFile(file, 'utf8')) + '\n');
    process.exit(0);
  }
  const [dir, tag] = process.argv.slice(2);
  if (!dir || !tag) throw new Error('Usage: node scripts/release-manifest.mjs <installer-directory> <tag>');
  await writeFile(join(dir, 'latest.json'), JSON.stringify(await manifest(dir, tag), null, 2) + '\n');
}
