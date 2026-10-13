/**
 * Starts the real server against a fresh sample stash for every run.
 *
 * 1. scripts/dev-fixture.sh builds the stash in a new temp folder.
 * 2. ffmpeg makes the small clips the upload and Inbox tests bring in.
 * 3. `popcornstash serve` starts with STASH_ROOT/STASH_CONFIG in that folder and
 *    no supervisor (so torrents are off), and the setup code is read from its log.
 *
 * Tests find everything through E2E_* environment variables (see helpers/env.ts).
 * The returned teardown stops the server and deletes the folder (E2E_KEEP=1 keeps it).
 */
import {execFileSync, spawn} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import type {FullConfig} from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');

/** Strips terminal colour codes from the server log. */
export function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[0-9;]*m/g, '');
}

function makeClip(out: string, seconds: number, hue: number) {
  execFileSync('ffmpeg', [
    '-nostdin', '-hide_banner', '-loglevel', 'error', '-y',
    '-f', 'lavfi', '-i', `testsrc2=size=320x180:rate=24:duration=${seconds},hue=h=${hue}`,
    '-f', 'lavfi', '-i', `sine=frequency=660:sample_rate=48000:duration=${seconds}`,
    '-map', '0:v', '-map', '1:a',
    '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '30', '-pix_fmt', 'yuv420p',
    '-c:a', 'aac', '-b:a', '64k', '-shortest', '-movflags', '+faststart', out,
  ]);
}

async function waitForHealth(url: string, server: {exited: boolean}, log: string, timeoutMs: number) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (server.exited) throw new Error(`The server exited during startup. Log:\n${readLog(log)}`);
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  throw new Error(`The server did not answer ${url} within ${timeoutMs / 1000}s. Log:\n${readLog(log)}`);
}

function readLog(log: string): string {
  try {
    return stripAnsi(fs.readFileSync(log, 'utf8'));
  } catch {
    return '';
  }
}

async function waitForSetupCode(log: string, timeoutMs: number): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const match = /\b([A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4})\b/.exec(readLog(log));
    if (match?.[1]) return match[1];
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  throw new Error(`No setup code in the server log. Log:\n${readLog(log)}`);
}

export default async function globalSetup(config: FullConfig) {
  const baseURL = config.projects[0]?.use.baseURL ?? 'http://localhost:8788';
  const port = new URL(baseURL).port || '80';
  const binary = process.env.E2E_SERVER_BIN ?? path.join(repoRoot, 'target', 'debug', 'popcornstash');
  if (!fs.existsSync(binary)) {
    throw new Error(`No server binary at ${binary}. Run \`cargo build -p popcornstash\` first.`);
  }
  if (!fs.existsSync(path.join(repoRoot, 'web', 'dist', 'index.html'))) {
    throw new Error('No web build. Run `npm --prefix web run build` first.');
  }
  try {
    await fetch(`${baseURL}/api/health`);
    throw new Error(`Something is already listening on port ${port}. Stop it or set E2E_PORT.`);
  } catch (error) {
    if (error instanceof Error && error.message.startsWith('Something')) throw error;
  }

  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'popcornstash-e2e-'));
  const stash = path.join(work, 'stash');
  const configDir = path.join(work, 'config');
  const files = path.join(work, 'files');
  const log = path.join(work, 'server.log');
  fs.mkdirSync(configDir);
  fs.mkdirSync(files);

  execFileSync(path.join(repoRoot, 'scripts', 'dev-fixture.sh'), [stash], {stdio: 'inherit'});
  makeClip(path.join(files, 'Sunset.Drive.2020.720p.WEB.mp4'), 3, 30);
  makeClip(path.join(files, 'Kite.Festival.2021.1080p.WEB.mp4'), 3, 150);
  makeClip(path.join(files, 'Copper.Trail.2018.720p.WEB.mp4'), 3, 270);

  const env: NodeJS.ProcessEnv = {...process.env};
  // Without the supervisor's variables the server runs alone and torrents are off.
  for (const key of Object.keys(env)) {
    if (key.startsWith('STASH_')) delete env[key];
  }
  Object.assign(env, {
    STASH_ROOT: stash,
    STASH_CONFIG: configDir,
    STASH_PUBLIC_URL: baseURL,
    STASH_BIND: `127.0.0.1:${port}`,
    STASH_LOG: process.env.E2E_LOG ?? 'info,tower_http=warn',
    NO_COLOR: '1',
  });
  const out = fs.openSync(log, 'a');
  const child = spawn(binary, ['serve'], {cwd: repoRoot, env, stdio: ['ignore', out, out]});
  const state = {exited: false};
  child.on('exit', () => {
    state.exited = true;
  });

  const stop = async () => {
    if (!state.exited) {
      child.kill('SIGTERM');
      const deadline = Date.now() + 10_000;
      while (!state.exited && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 100));
      if (!state.exited) child.kill('SIGKILL');
    }
    fs.closeSync(out);
    if (process.env.E2E_KEEP === '1') {
      console.log(`E2E files kept in ${work}`);
    } else {
      fs.rmSync(work, {recursive: true, force: true});
    }
  };

  try {
    await waitForHealth(`${baseURL}/api/health`, state, log, 30_000);
    const setupCode = await waitForSetupCode(log, 10_000);
    Object.assign(process.env, {
      E2E_SERVER_BIN: binary,
      E2E_WORK: work,
      E2E_STASH: stash,
      E2E_CONFIG: configDir,
      E2E_FILES: files,
      E2E_SERVER_LOG: log,
      E2E_SETUP_CODE: setupCode,
    });
  } catch (error) {
    await stop();
    throw error;
  }
  return stop;
}
