/** The device name shown in PopcornStash, Settings, Devices: "Chrome on macOS". */

const OS_NAMES: Record<string, string> = {
  mac: 'macOS',
  win: 'Windows',
  linux: 'Linux',
  cros: 'ChromeOS',
  android: 'Android',
  openbsd: 'OpenBSD',
  fuchsia: 'Fuchsia',
};

export function clientName(os: string | null | undefined): string {
  const name = os ? OS_NAMES[os] : undefined;
  return name ? `Chrome on ${name}` : 'Chrome';
}

export async function currentClientName(): Promise<string> {
  try {
    const info = await chrome.runtime.getPlatformInfo();
    return clientName(info.os);
  } catch {
    return clientName(null);
  }
}
