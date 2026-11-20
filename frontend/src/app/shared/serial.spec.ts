import { Serial } from './serial';

describe('Serial', () => {
  it('runs each job once the one before is over, a failure included', async () => {
    const serial = new Serial();
    const ran: string[] = [];
    let release!: () => void;
    const first = serial.run(
      () =>
        new Promise<void>((resolve) => {
          ran.push('first');
          release = resolve;
        }),
    );
    const failing = serial
      .run(async () => {
        ran.push('second');
        throw new Error('no');
      })
      .catch((e: Error) => e.message);
    const third = serial.run(async () => ran.push('third'));
    await Promise.resolve();
    expect(ran).toEqual(['first']);
    expect(serial.busy()).toBe(true);
    release();
    await first;
    expect(await failing).toBe('no');
    await third;
    await serial.settled();
    expect(ran).toEqual(['first', 'second', 'third']);
    expect(serial.busy()).toBe(false);
  });
});
