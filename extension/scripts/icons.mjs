// Render the toolbar and store icons from the brand mark (assets/brand-mark.svg,
// the same popcorn mark as web/brand/icon.svg). Needs rsvg-convert (librsvg2-bin).
// The PNGs are committed, so building the extension does not need this tool.
import {execFileSync} from 'node:child_process';
import {mkdir} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'static', 'icons');
await mkdir(out, {recursive: true});

// 16/32/48 fill the square; 128 keeps Chrome's 16px transparent margin around 96px artwork.
const jobs = [
  {size: 16, source: 'brand-mark.svg'},
  {size: 32, source: 'brand-mark.svg'},
  {size: 48, source: 'brand-mark.svg'},
  {size: 128, source: 'brand-mark-padded.svg'},
];
for (const {size, source} of jobs) {
  const target = join(out, `icon-${size}.png`);
  execFileSync('rsvg-convert', ['-w', String(size), '-h', String(size), '-o', target, join(root, 'assets', source)]);
  console.log(`icons/icon-${size}.png`);
}
