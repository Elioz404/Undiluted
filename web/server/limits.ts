/**
 * Keeps a public relay from being drained. Every call it relays costs it gas or
 * test tokens, so each caller gets a few calls per window, and all callers
 * together get a daily spending budget. Both live in memory: a restart resets
 * them, which is acceptable for a demo relay and keeps it free of storage.
 */

export class RateLimiter {
  private readonly windows = new Map<string, { started: number; used: number }>();

  constructor(
    private readonly limit: number,
    private readonly windowMs: number,
    private readonly now: () => number = Date.now,
  ) {}

  /** Counts one call from `caller`. False once it has used up its window. */
  allow(caller: string): boolean {
    const now = this.now();
    const current = this.windows.get(caller);
    if (!current || now - current.started >= this.windowMs) {
      this.forgetExpired(now);
      this.windows.set(caller, { started: now, used: 1 });
      return true;
    }
    if (current.used >= this.limit) {
      return false;
    }
    current.used += 1;
    return true;
  }

  private forgetExpired(now: number): void {
    for (const [caller, window] of this.windows) {
      if (now - window.started >= this.windowMs) {
        this.windows.delete(caller);
      }
    }
  }
}

const DAY_MS = 24 * 60 * 60 * 1000;

export class Budget {
  private day = -1;
  private spent = 0n;

  constructor(
    private readonly dailyMicros: bigint,
    private readonly now: () => number = Date.now,
  ) {}

  /** Reserves `micros` from today's budget. False if that would exceed it. */
  reserve(micros: bigint): boolean {
    const day = Math.floor(this.now() / DAY_MS);
    if (day !== this.day) {
      this.day = day;
      this.spent = 0n;
    }
    if (this.spent + micros > this.dailyMicros) {
      return false;
    }
    this.spent += micros;
    return true;
  }

  /** Gives back a reservation whose spending did not happen. */
  release(micros: bigint): void {
    this.spent = this.spent > micros ? this.spent - micros : 0n;
  }
}
