/** Only explicit input advances a browser session; fetches and media never do. */
export const BROWSER_IDLE_MS = 15 * 60 * 1000;
export const ACTIVITY_INTERVAL_MS = 10_000;

export interface IdleDeadline {server_time: number; idle_expires_at: number}

export class BrowserIdle {
  private deadline: number;
  private lastSent = -Infinity;
  private expired = false;
  constructor(now: number, initial?: IdleDeadline) {
    this.deadline = initial ? now + Math.max(0, initial.idle_expires_at - initial.server_time) * 1000 : now + BROWSER_IDLE_MS;
  }
  remaining(now: number): number { return Math.max(0, this.deadline - now); }
  check(now: number): boolean {
    if (now >= this.deadline) this.expired = true;
    return this.expired;
  }
  input(now: number): boolean {
    if (this.check(now) || now - this.lastSent < ACTIVITY_INTERVAL_MS) return false;
    this.lastSent = now;
    return true;
  }
  acknowledge(now: number, next: IdleDeadline): void {
    // A delayed response must never reopen a screen that has already locked.
    if (this.check(now)) return;
    this.deadline = Math.max(this.deadline, now + Math.max(0, next.idle_expires_at - next.server_time) * 1000);
  }
}
