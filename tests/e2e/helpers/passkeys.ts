/**
 * Passkeys through Chrome's virtual WebAuthn authenticator (DevTools protocol).
 *
 * Each page gets its own authenticator: a platform authenticator with
 * resident keys and user verification, which always says yes. Credentials can
 * be exported after registration and imported into another page, which is how
 * the same "device" signs in again from a fresh browser context.
 */
import fs from 'node:fs';
import type {CDPSession, Page} from '@playwright/test';

export interface StoredCredential {
  credentialId: string;
  isResidentCredential: boolean;
  rpId?: string;
  privateKey: string;
  userHandle?: string;
  signCount: number;
}

export class VirtualAuthenticator {
  private constructor(
    private readonly cdp: CDPSession,
    readonly id: string,
  ) {}

  static async attach(page: Page): Promise<VirtualAuthenticator> {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('WebAuthn.enable', {enableUI: false});
    const {authenticatorId} = await cdp.send('WebAuthn.addVirtualAuthenticator', {
      options: {
        protocol: 'ctap2',
        transport: 'internal',
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });
    return new VirtualAuthenticator(cdp, authenticatorId);
  }

  async credentials(): Promise<StoredCredential[]> {
    const {credentials} = await this.cdp.send('WebAuthn.getCredentials', {authenticatorId: this.id});
    return credentials as StoredCredential[];
  }

  async add(credential: StoredCredential): Promise<void> {
    await this.cdp.send('WebAuthn.addCredential', {authenticatorId: this.id, credential});
  }

  /** Saves this authenticator's credentials (with their current sign counts) to a file. */
  async save(file: string): Promise<void> {
    fs.writeFileSync(file, JSON.stringify(await this.credentials(), null, 2));
  }

  /** Loads credentials saved with `save`. */
  async load(file: string): Promise<void> {
    const stored = JSON.parse(fs.readFileSync(file, 'utf8')) as StoredCredential[];
    for (const credential of stored) await this.add(credential);
  }
}
