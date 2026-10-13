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

// Chrome accepts only one to four numbers (0 to 65535) as an extension's version, so a
// pre-release such as 0.2.0-rc.1 gets version 0.2.0 and shows the full one as version_name.
const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
const semver = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(pkg.version);
if (!semver || semver.slice(1, 4).some((part) => Number(part) > 65535)) {
  throw new Error(`package.json version ${pkg.version} is not MAJOR.MINOR.PATCH[-pre-release] with parts up to 65535`);
}
const manifest = JSON.parse(await readFile(join(root, 'static', 'manifest.json'), 'utf8'));
manifest.version = semver.slice(1, 4).map(Number).join('.');
if (semver[4]) manifest.version_name = pkg.version;
else delete manifest.version_name;
await writeFile(join(dist, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`Built PopcornStash extension ${pkg.version} into dist/`);
