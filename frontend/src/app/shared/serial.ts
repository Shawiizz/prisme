import { computed, signal } from '@angular/core';

/** Jobs run one after the other, each once the one before is over (failed or not). */
export class Serial {
  private last: Promise<unknown> = Promise.resolve();
  private readonly waiting = signal(0);
  /** A job is running or waits. */
  readonly busy = computed(() => this.waiting() > 0);

  run<T>(job: () => Promise<T>): Promise<T> {
    this.waiting.update((n) => n + 1);
    const done = this.last.then(job).finally(() => this.waiting.update((n) => n - 1));
    this.last = done.catch(() => undefined);
    return done;
  }

  /** Once every job asked for so far is over. */
  settled(): Promise<void> {
    return this.last.then(() => undefined);
  }
}
