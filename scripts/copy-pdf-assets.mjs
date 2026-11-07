// Copies the pdf.js standard fonts, CMaps and wasm decoders into public/pdf-assets so the
// reader works offline. Run automatically before dev and build.
import { cpSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const src = join(root, 'node_modules', 'pdfjs-dist');
const out = join(root, 'public', 'pdf-assets');
mkdirSync(out, { recursive: true });
for (const dir of ['standard_fonts', 'cmaps', 'wasm']) {
  const from = join(src, dir);
  if (existsSync(from)) cpSync(from, join(out, dir), { recursive: true });
}
cpSync(join(src, 'LICENSE'), join(out, 'LICENSE'));
console.log('pdf.js assets copied to', out);
