/**
 * Options page: pair with a PopcornStash server (device code flow), show the
 * connection, and the two optional behaviours (take over all downloads,
 * include the download site's cookies). Every permission is requested only
 * when the person turns the matching thing on.
 */

import {pollPairing, requestPairing} from '../lib/api';
import {byId, h, mount} from '../lib/dom';
import {toProblem} from '../lib/errors';
import {currentClientName} from '../lib/platform';
import {hasHostAccess} from '../lib/send';
import {displayHost, parseServerAddress, resolveVerifyUrl} from '../lib/server-url';
import {
  clearPairing,
  forgetConnection,
  getConnection,
  getLastServer,
  getNotice,
  getPairing,
  getPrefs,
  savePairing,
  saveConnection,
  setPref,
  type PendingPairing,
  type StoredConnection,
} from '../lib/storage';

const MIN_POLL_S = 3;
const MAX_POLL_S = 30;
const ALL_SITES = '<all_urls>';

const title = byId('page-title');
const lede = byId('page-lede');
const serverSection = byId('server-body');
const interceptToggle = byId<HTMLInputElement>('intercept');
const cookiesToggle = byId<HTMLInputElement>('cookies');
const interceptMessage = byId('intercept-message');
const cookiesMessage = byId('cookies-message');

let clientName = 'Chrome';
let pollTimer: number | undefined;
let pollGeneration = 0;

type Notice = {tone: 'info' | 'error'; text: string; action?: {label: string; run: () => void}} | null;

const timeFormat = new Intl.DateTimeFormat(undefined, {hour: 'numeric', minute: '2-digit'});
const dateFormat = new Intl.DateTimeFormat(undefined, {dateStyle: 'long'});

function stopPolling(): void {
  pollGeneration += 1;
  if (pollTimer !== undefined) window.clearTimeout(pollTimer);
  pollTimer = undefined;
}

function noticeBlock(notice: Notice): HTMLElement | null {
  if (!notice) return null;
  return h(
    'div',
    {class: `notice ${notice.tone === 'error' ? 'notice-error' : 'notice-info'}`, role: notice.tone === 'error' ? 'alert' : 'status'},
    h('p', {class: 'notice-text'}, notice.text),
    notice.action
      ? h('button', {class: 'button button-secondary button-small', type: 'button', onclick: notice.action.run}, notice.action.label)
      : null,
  );
}

function openDevices(server: string): void {
  void chrome.tabs.create({url: `${server}/settings/devices`});
}

// ---------------------------------------------------------------------------
// Not connected: the address form
// ---------------------------------------------------------------------------

function renderForm(notice: Notice, prefill: string): void {
  stopPolling();
  title.textContent = 'Connect Chrome to PopcornStash';
  lede.textContent = 'Send links, videos and magnet links from any page to your PopcornStash server.';

  const input = h('input', {
    id: 'server',
    class: 'input',
    type: 'text',
    inputmode: 'url',
    autocomplete: 'url',
    autocapitalize: 'off',
    spellcheck: 'false',
    placeholder: 'https://stash.example.com',
    value: prefill,
    'aria-describedby': 'server-hint server-error',
  });
  const error = h('p', {class: 'field-message field-message-error', id: 'server-error', role: 'alert'});
  const submit = h('button', {class: 'button button-primary', type: 'submit'}, 'Connect');
  const form = h(
    'form',
    {class: 'field', novalidate: true},
    h('label', {class: 'label', for: 'server'}, 'Server address'),
    h('p', {class: 'hint', id: 'server-hint'}, 'The address you open PopcornStash at. Plain http only works for localhost.'),
    h('div', {class: 'field-row'}, input, submit),
    error,
  );

  const showError = (text: string): void => {
    error.textContent = text;
    input.setAttribute('aria-invalid', 'true');
    input.focus();
  };

  form.addEventListener('submit', event => {
    event.preventDefault();
    const parsed = parseServerAddress(input.value);
    if (!parsed.ok) {
      showError(parsed.reason);
      return;
    }
    input.value = parsed.origin;
    input.removeAttribute('aria-invalid');
    error.textContent = '';
    submit.disabled = true;
    submit.textContent = 'Connecting…';
    // The permission prompt must be the first thing this click does.
    void chrome.permissions
      .request({origins: [parsed.pattern]})
      .then(async granted => {
        if (!granted) {
          showError(`Chrome needs your permission to reach ${parsed.host}. Click Connect again and choose Allow.`);
          return;
        }
        try {
          const started = await requestPairing(parsed.origin, clientName);
          const pairing: PendingPairing = {
            server: parsed.origin,
            pairingId: started.pairing_id,
            pollSecret: started.poll_secret,
            userCode: started.user_code,
            verifyUrl: resolveVerifyUrl(started.verify_url, parsed.origin, started.user_code),
            expiresAt: started.expires_at,
            interval: Math.max(MIN_POLL_S, Number(started.interval) || MIN_POLL_S),
          };
          await savePairing(pairing);
          renderPairing(pairing, null);
          schedulePoll(pairing, pairing.interval);
        } catch (failure) {
          const problem = toProblem(failure, parsed.host);
          showError(`${problem.title}. ${problem.message}`);
        }
      })
      .catch(() => showError('Chrome could not ask for permission. Reload this page and try again.'))
      .finally(() => {
        if (submit.isConnected) {
          submit.disabled = false;
          submit.textContent = 'Connect';
        }
      });
  });

  input.addEventListener('input', () => {
    if (input.getAttribute('aria-invalid') === 'true') {
      input.removeAttribute('aria-invalid');
      error.textContent = '';
    }
  });

  mount(
    serverSection,
    noticeBlock(notice),
    form,
    h(
      'p',
      {class: 'footnote'},
      'Chrome asks to let the extension reach that address. Your server is the only site it needs for sending links.',
    ),
  );
}

// ---------------------------------------------------------------------------
// Waiting for approval
// ---------------------------------------------------------------------------

function spelled(code: string): string {
  return code
    .split('-')
    .map(group => group.split('').join(' '))
    .join(', ');
}

function renderPairing(pairing: PendingPairing, trouble: string | null): void {
  title.textContent = 'Approve this browser in PopcornStash';
  lede.textContent = 'One step left. Someone signed in to PopcornStash approves the code below.';
  const expires = timeFormat.format(new Date(pairing.expiresAt * 1000));
  mount(
    serverSection,
    h(
      'div',
      {class: 'pairing'},
      h('p', {class: 'label', id: 'code-label'}, `Code for ${displayHost(pairing.server)}`),
      h('p', {class: 'code', 'aria-labelledby': 'code-label', 'aria-label': spelled(pairing.userCode)}, pairing.userCode),
      h(
        'p',
        {class: 'hint'},
        'PopcornStash opens with the code filled in. Check it matches, then approve. You can also enter it in Settings, Devices.',
      ),
      h(
        'div',
        {class: 'actions'},
        h(
          'button',
          {class: 'button button-primary', type: 'button', onclick: () => void chrome.tabs.create({url: pairing.verifyUrl})},
          'Open PopcornStash to approve',
        ),
        h(
          'button',
          {
            class: 'button button-secondary',
            type: 'button',
            onclick: () => {
              stopPolling();
              void clearPairing().then(() => renderForm(null, pairing.server));
            },
          },
          'Cancel',
        ),
      ),
      h(
        'p',
        {class: trouble ? 'status status-trouble' : 'status', role: 'status'},
        trouble ?? `Waiting for approval. The code works until ${expires}.`,
      ),
    ),
  );
}

function schedulePoll(pairing: PendingPairing, delayS: number): void {
  stopPolling();
  const generation = pollGeneration;
  pollTimer = window.setTimeout(() => void poll(pairing, delayS, generation), delayS * 1000);
}

async function poll(pairing: PendingPairing, delayS: number, generation: number): Promise<void> {
  if (generation !== pollGeneration) return;
  if (Date.now() / 1000 > pairing.expiresAt) {
    await clearPairing();
    renderForm({tone: 'error', text: 'The code expired before anyone approved it. Connect again for a new code.'}, pairing.server);
    return;
  }
  try {
    const answer = await pollPairing(pairing.server, pairing.pairingId, pairing.pollSecret);
    if (generation !== pollGeneration) return;
    switch (answer.status) {
      case 'pending':
        renderPairing(pairing, null);
        schedulePoll(pairing, pairing.interval);
        return;
      case 'approved': {
        const connection: StoredConnection = {
          server: pairing.server,
          token: answer.token,
          serverName: answer.server_name || displayHost(pairing.server),
          scopes: answer.scopes,
          pairedAt: Date.now(),
        };
        await saveConnection(connection);
        stopPolling();
        await renderConnected(connection, {tone: 'info', text: 'Connected. You can close this tab and send links from the toolbar or the right-click menu.'});
        return;
      }
      case 'denied':
        await clearPairing();
        renderForm({tone: 'error', text: 'Pairing was declined in PopcornStash. Connect again if that was a mistake.'}, pairing.server);
        return;
      case 'expired':
        await clearPairing();
        renderForm({tone: 'error', text: 'The code expired before anyone approved it. Connect again for a new code.'}, pairing.server);
        return;
    }
  } catch (failure) {
    if (generation !== pollGeneration) return;
    const problem = toProblem(failure, displayHost(pairing.server));
    if (problem.kind === 'rejected') {
      // The server no longer knows this pairing request.
      await clearPairing();
      renderForm({tone: 'error', text: `${problem.message} Connect again for a new code.`}, pairing.server);
      return;
    }
    const next = problem.kind === 'rate_limited' ? Math.min(delayS * 2, MAX_POLL_S) : pairing.interval;
    const trouble =
      problem.kind === 'rate_limited'
        ? 'PopcornStash asked this browser to check less often. Still waiting for approval.'
        : `Lost contact with ${displayHost(pairing.server)}. Still trying.`;
    renderPairing(pairing, trouble);
    schedulePoll(pairing, next);
  }
}

// ---------------------------------------------------------------------------
// Connected
// ---------------------------------------------------------------------------

async function renderConnected(connection: StoredConnection, notice: Notice): Promise<void> {
  stopPolling();
  const host = displayHost(connection.server);
  title.textContent = 'Chrome is connected to PopcornStash';
  lede.textContent = 'Right-click a link, video or image and choose Send to PopcornStash, or paste links in the toolbar popup.';

  const access = await hasHostAccess(connection.server);
  const accessNotice: Notice = access
    ? notice
    : {
        tone: 'error',
        text: `Chrome no longer lets this extension reach ${host}, so nothing can be sent. This can happen when site access changes in Chrome.`,
        action: {
          label: `Allow access to ${host}`,
          run: () => {
            void chrome.permissions.request({origins: [`${connection.server}/*`]}).then(() => void onPermissionsChanged());
          },
        },
      };

  const canRead = connection.scopes.includes('downloads:read') || connection.scopes.includes('full');
  mount(
    serverSection,
    noticeBlock(accessNotice),
    h(
      'dl',
      {class: 'facts'},
      h('div', {class: 'fact'}, h('dt', {}, 'Server'), h('dd', {}, host)),
      h('div', {class: 'fact'}, h('dt', {}, 'Paired'), h('dd', {}, dateFormat.format(new Date(connection.pairedAt)))),
      h(
        'div',
        {class: 'fact'},
        h('dt', {}, 'Access'),
        h('dd', {}, canRead ? 'Add downloads and see their progress' : 'Add downloads'),
      ),
    ),
    h(
      'div',
      {class: 'actions'},
      h('button', {class: 'button button-secondary', type: 'button', onclick: () => void chrome.tabs.create({url: connection.server})}, 'Open PopcornStash'),
      h('button', {class: 'button button-secondary', type: 'button', onclick: () => void disconnect(connection)}, 'Disconnect'),
    ),
    h(
      'p',
      {class: 'footnote'},
      'Disconnect forgets the access key on this computer. To cut off the key on the server as well, remove this browser in PopcornStash, Settings, Devices.',
    ),
  );
}

async function disconnect(connection: StoredConnection): Promise<void> {
  await forgetConnection(null);
  try {
    await chrome.permissions.remove({origins: [`${connection.server}/*`]});
  } catch {
    // Access granted through another option stays until that option is turned off.
  }
  renderForm(
    {
      tone: 'info',
      text: 'Disconnected. This browser no longer has an access key. The key still appears in PopcornStash until you remove it in Settings, Devices.',
      action: {label: 'Open Settings, Devices', run: () => openDevices(connection.server)},
    },
    connection.server,
  );
}

// ---------------------------------------------------------------------------
// Optional behaviours
// ---------------------------------------------------------------------------

async function syncToggles(): Promise<void> {
  const prefs = await getPrefs();
  const downloads = await chrome.permissions.contains({permissions: ['downloads']});
  const cookies = await chrome.permissions.contains({permissions: ['cookies'], origins: [ALL_SITES]});
  // A permission removed in chrome://extensions turns its option off.
  if (prefs.interceptDownloads && !downloads) await setPref('interceptDownloads', false);
  if (prefs.includeCookies && !cookies) await setPref('includeCookies', false);
  interceptToggle.checked = prefs.interceptDownloads && downloads;
  cookiesToggle.checked = prefs.includeCookies && cookies;
}

function say(element: HTMLElement, text: string): void {
  element.textContent = text;
  element.hidden = text === '';
}

interceptToggle.addEventListener('change', () => {
  say(interceptMessage, '');
  if (interceptToggle.checked) {
    void chrome.permissions
      .request({permissions: ['downloads']})
      .then(async granted => {
        await setPref('interceptDownloads', granted);
        interceptToggle.checked = granted;
        if (!granted) say(interceptMessage, 'Chrome did not allow it, so this stays off.');
      })
      .catch(() => {
        interceptToggle.checked = false;
        say(interceptMessage, 'Chrome could not ask for permission. Reload this page and try again.');
      });
  } else {
    void setPref('interceptDownloads', false).then(() =>
      chrome.permissions.remove({permissions: ['downloads']}).catch(() => false),
    );
  }
});

cookiesToggle.addEventListener('change', () => {
  say(cookiesMessage, '');
  if (cookiesToggle.checked) {
    void chrome.permissions
      .request({permissions: ['cookies'], origins: [ALL_SITES]})
      .then(async granted => {
        await setPref('includeCookies', granted);
        cookiesToggle.checked = granted;
        if (!granted) say(cookiesMessage, 'Chrome did not allow it, so this stays off.');
      })
      .catch(() => {
        cookiesToggle.checked = false;
        say(cookiesMessage, 'Chrome could not ask for permission. Reload this page and try again.');
      });
  } else {
    void (async () => {
      await setPref('includeCookies', false);
      try {
        await chrome.permissions.remove({permissions: ['cookies'], origins: [ALL_SITES]});
      } catch {
        // Nothing to remove.
      }
      // Removing access to every site can take the server's own access with it.
      await onPermissionsChanged();
    })();
  }
});

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

async function refresh(): Promise<void> {
  const connection = await getConnection();
  if (connection) {
    await renderConnected(connection, null);
  } else {
    const pairing = await getPairing();
    if (pairing && pairing.expiresAt > Date.now() / 1000) {
      if (pollTimer === undefined) {
        renderPairing(pairing, null);
        schedulePoll(pairing, MIN_POLL_S);
      }
    } else {
      if (pairing) await clearPairing();
      const notice = await getNotice();
      const last = (await getLastServer()) ?? '';
      renderForm(
        notice?.kind === 'revoked'
          ? {
              tone: 'error',
              text: `PopcornStash at ${displayHost(notice.server)} stopped accepting this browser. Its access may have been removed in Settings, Devices. Connect again to pair it.`,
            }
          : null,
        last,
      );
    }
  }
  await syncToggles();
}

/** Permissions changed here or in chrome://extensions: update the switches and the connection's access. */
async function onPermissionsChanged(): Promise<void> {
  await syncToggles();
  const connection = await getConnection();
  if (connection) await renderConnected(connection, null);
}

chrome.storage.onChanged.addListener((changes, area) => {
  // The background forgets the token when the server rejects it (a 401).
  if (area !== 'local' || !('connection' in changes) || changes.connection?.newValue) return;
  void getNotice().then(notice => {
    if (notice?.kind === 'revoked' && pollTimer === undefined) void refresh();
  });
});
chrome.permissions.onRemoved.addListener(() => void onPermissionsChanged());

void (async () => {
  clientName = await currentClientName();
  await refresh();
})();
