// Build the extension into dist/: bundle TypeScript and CSS with esbuild,
// copy the static files, and stamp the manifest with package.json's version.
import {build} from 'esbuild';
import {cp, mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist');
const watch = process.argv.includes('--watch');

await rm(dist, {recursive: true, force: true});
await mkdir(dist, {recursive: true});

await build({
  absWorkingDir: root,
  entryPoints: {
    background: 'src/background.ts',
    popup: 'src/popup/popup.ts',
    options: 'src/options/options.ts',
    'popup-style': 'src/styles/popup.css',
    'options-style': 'src/styles/options.css',
  },
  outdir: dist,
  bundle: true,
  format: 'esm',
  target: ['chrome116'],
  minify: !watch,
  sourcemap: false,
  legalComments: 'none',
  logLevel: 'info',
  charset: 'utf8',
});

// esbuild names CSS outputs after their entry; the pages link popup.css / options.css.
for (const name of ['popup', 'options']) {
  const css = await readFile(join(dist, `${name}-style.css`));
  await writeFile(join(dist, `${name}.css`), css);
  await rm(join(dist, `${name}-style.css`));
}

await cp(join(root, 'static'), dist, {recursive: true});

const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
const manifest = JSON.parse(await readFile(join(root, 'static', 'manifest.json'), 'utf8'));
manifest.version = pkg.version;
await writeFile(join(dist, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`Built PopcornStash extension ${pkg.version} into dist/`);
