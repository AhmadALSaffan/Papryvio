// Downloads the PDFium build that matches the pdfium-render feature `pdfium_8125`
// and places pdfium.dll + its license under src-tauri/resources/pdfium/.
// Source: https://github.com/bblanchon/pdfium-binaries (the distribution pdfium-render documents).
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync, existsSync, copyFileSync, rmSync, readdirSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';

const CHROMIUM_BUILD = '8125';
const ASSET = 'pdfium-win-x64.tgz';
// SHA-256 of the archive, pinned after the first verified download.
const EXPECTED_SHA256 = process.env.PDFIUM_SHA256 || '9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'src-tauri', 'resources', 'pdfium');
if (existsSync(join(outDir, 'pdfium.dll')) && !process.argv.includes('--force')) {
  console.log('pdfium.dll already present:', outDir);
  process.exit(0);
}
const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/${CHROMIUM_BUILD}/${ASSET}`;
console.log('Downloading', url);
const res = await fetch(url, { redirect: 'follow' });
if (!res.ok) throw new Error(`Download failed: ${res.status}`);
const buf = Buffer.from(await res.arrayBuffer());
const sha = createHash('sha256').update(buf).digest('hex');
console.log('sha256', sha);
if (EXPECTED_SHA256 && EXPECTED_SHA256 !== sha) throw new Error('Checksum mismatch');

const work = join(tmpdir(), `pdfium-${Date.now()}`);
mkdirSync(work, { recursive: true });
const archive = join(work, ASSET);
writeFileSync(archive, buf);
// Windows ships bsdtar in System32; GNU tar (e.g. from Git Bash) misreads drive letters.
const tar = process.platform === 'win32' ? join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
execFileSync(tar, ['-xzf', archive, '-C', work]);
mkdirSync(outDir, { recursive: true });
copyFileSync(join(work, 'bin', 'pdfium.dll'), join(outDir, 'pdfium.dll'));
for (const f of readdirSync(work)) {
  if (/^LICENSE/i.test(f) && statSync(join(work, f)).isFile()) copyFileSync(join(work, f), join(outDir, f));
}
if (existsSync(join(work, 'licenses'))) {
  mkdirSync(join(outDir, 'licenses'), { recursive: true });
  for (const f of readdirSync(join(work, 'licenses'))) copyFileSync(join(work, 'licenses', f), join(outDir, 'licenses', f));
}
writeFileSync(join(outDir, 'VERSION.txt'), `chromium/${CHROMIUM_BUILD}\n${ASSET}\nsha256 ${sha}\n`);
rmSync(work, { recursive: true, force: true });
console.log('PDFium ready in', outDir);
