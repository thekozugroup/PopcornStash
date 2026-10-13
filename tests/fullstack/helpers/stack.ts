/**
 * The running stack, as scripts/fullstack-e2e.sh describes it in FS_* variables,
 * and the checks made outside the browser: the PopcornStash container's
 * internal engine API, the tracker's announce log, the seeder, and the files
 * in the stash folder on the host.
 */
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; run this spec with scripts/fullstack-e2e.sh.`);
  return value;
}

export const stack = {
  get setupCode() {
    return required('FS_SETUP_CODE');
  },
  /** The WireGuard file the owner uploads (it points at wgserver). */
  get wireguardFile() {
    return required('FS_WG_FILE');
  },
  get magnet() {
    return required('FS_MAGNET');
  },
  get infoHash() {
    return required('FS_INFO_HASH');
  },
  /** Torrent name, as the magnet's dn: "Lantern.Bay.2018.720p.WEB.webm". */
  get torrentName() {
    return required('FS_TORRENT_NAME');
  },
  get torrentBytes() {
    return Number(required('FS_TORRENT_BYTES'));
  },
  get title() {
    return required('FS_TITLE');
  },
  get year() {
    return required('FS_YEAR');
  },
  get stashDir() {
    return required('FS_STASH');
  },
  get evidenceDir() {
    return required('FS_EVIDENCE');
  },
  get trackerLog() {
    return required('FS_TRACKER_LOG');
  },
  get mainContainer() {
    return required('FS_MAIN');
  },
  get wgContainer() {
    return required('FS_WG');
  },
  get seederContainer() {
    return required('FS_SEEDER');
  },
  get mainIp() {
    return required('FS_MAIN_IP');
  },
  get wgIp() {
    return required('FS_WG_IP');
  },
  get seederIp() {
    return required('FS_SEEDER_IP');
  },
  get puid() {
    return Number(required('FS_PUID'));
  },
  get pgid() {
    return Number(required('FS_PGID'));
  },
  /** STASH_TORRENT_UID: the torrent engine's user. */
  get engineUid() {
    return Number(required('FS_ENGINE_UID'));
  },
};

export const OWNER_NAME = 'Robin Example';

export function docker(args: string[], input?: string): string {
  return execFileSync('docker', args, {encoding: 'utf8', input, stdio: ['pipe', 'pipe', 'pipe'], timeout: 60_000});
}

/** Saves a text file of evidence next to the screenshots. */
export function saveEvidence(name: string, content: string) {
  fs.mkdirSync(stack.evidenceDir, {recursive: true});
  fs.writeFileSync(path.join(stack.evidenceDir, name), content);
}

export function screenshotPath(name: string): string {
  fs.mkdirSync(stack.evidenceDir, {recursive: true});
  return path.join(stack.evidenceDir, name);
}

// The internal token lives only in the children's environment. This shell code runs
// inside the container as the web server's user, finds `popcornstash serve` by its
// executable and arguments, and calls the engine with the token it reads there; the
// token is never printed.
const ENGINE_CALL = `
  token=""
  for d in /proc/[0-9]*; do
    [ "$(readlink "$d/exe" 2>/dev/null)" = /usr/local/bin/popcornstash ] || continue
    [ "$(tr "\\0" "\\n" < "$d/cmdline" 2>/dev/null | sed -n 2p)" = serve ] || continue
    token=$(tr "\\0" "\\n" < "$d/environ" | sed -n "s/^STASH_INTERNAL_TOKEN=//p")
    break
  done
  [ -n "$token" ] || { echo "serve is not running" >&2; exit 1; }
  curl -fsS -m 10 -H "Authorization: Bearer $token" "http://127.0.0.1:9091$API_PATH"`;

export interface EngineHealth {
  vpn: {state: string; interface: string | null; error: string | null; kill_switch: boolean; updated_at: number};
  engine: {state: 'running' | 'blocked'; reason: string | null; allow_no_vpn: boolean};
}

/** GET on the engine's internal API inside the PopcornStash container. */
export function engineGet<T>(apiPath: string): T {
  const out = docker([
    'exec',
    '--user',
    `${stack.puid}:${stack.pgid}`,
    '-e',
    `API_PATH=${apiPath}`,
    stack.mainContainer,
    'bash',
    '-c',
    ENGINE_CALL,
  ]);
  return JSON.parse(out) as T;
}

export function engineHealth(): EngineHealth {
  return engineGet<EngineHealth>('/health');
}

export interface Announce {
  at: number;
  ip: string;
  port: number;
  event: string;
  left: number;
  info_hash: string;
  peer_id: string;
  claimed_ip: string | null;
}

/** Every announce the tracker logged for the test torrent. */
export function announces(): Announce[] {
  if (!fs.existsSync(stack.trackerLog)) return [];
  return fs
    .readFileSync(stack.trackerLog, 'utf8')
    .split('\n')
    .filter(line => line.trim() !== '')
    .map(line => JSON.parse(line) as Announce)
    .filter(entry => entry.info_hash === stack.infoHash);
}

/** Remote addresses of the seeder's peer connections (TCP; uTP has no socket per peer). */
export function seederPeers(): string[] {
  const out = docker(['exec', stack.seederContainer, 'ss', '-Htn', 'state', 'established', '( sport = :6881 )']);
  return out
    .split('\n')
    .map(line => line.trim().split(/\s+/))
    .filter(cols => cols.length >= 4)
    .map(cols => (cols[3] ?? '').replace(/:\d+$/, ''));
}

export function stashPath(rel: string): string {
  return path.join(stack.stashDir, ...rel.split('/'));
}

/** Every entry under `rel` (inclusive) with its owner, as "uid:gid path". */
export function ownersUnder(rel: string): {path: string; uid: number; gid: number}[] {
  const result: {path: string; uid: number; gid: number}[] = [];
  const walk = (abs: string, relPath: string) => {
    const stat = fs.lstatSync(abs);
    result.push({path: relPath, uid: stat.uid, gid: stat.gid});
    if (stat.isDirectory()) {
      for (const name of fs.readdirSync(abs)) walk(path.join(abs, name), `${relPath}/${name}`);
    }
  };
  walk(stashPath(rel), rel);
  return result;
}

/** Every file or folder in the stash owned by `uid` (to find anything left to the engine user). */
export function ownedBy(uid: number): string[] {
  return ownersUnder('.')
    .filter(entry => entry.uid === uid)
    .map(entry => entry.path);
}
