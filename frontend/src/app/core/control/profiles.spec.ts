import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildControls, ControlActions } from './controls';

/** The controller profiles shipped with the app (`apps/prisme-desktop/controllers`). */
const FOLDER = '../apps/prisme-desktop/controllers';

interface Profile {
  name: string;
  controls: { target: string; encoding?: string }[];
  lights?: { target: string }[];
}

const profiles: Profile[] = readdirSync(FOLDER)
  .filter((f) => f.endsWith('.json'))
  .map((f) => JSON.parse(readFileSync(join(FOLDER, f), 'utf8')));

const controls = buildControls(new Proxy({}, { get: () => () => true }) as ControlActions);
const ids = new Set(controls.map((c) => c.id));
const encoders = new Set(controls.filter((c) => c.kind === 'encoder').map((c) => c.id));

describe('controller profiles', () => {
  it('are shipped', () => {
    expect(profiles.length).toBeGreaterThan(0);
  });

  it('drive the controls of the app', () => {
    for (const p of profiles) {
      const unknown = [...p.controls, ...(p.lights ?? [])].filter((c) => !ids.has(c.target));
      expect(unknown.map((c) => `${p.name}: ${c.target}`)).toEqual([]);
    }
  });

  it('read the steps of encoders only', () => {
    for (const p of profiles) {
      const misread = p.controls.filter(
        (c) => (c.encoding !== undefined) !== encoders.has(c.target),
      );
      expect(misread.map((c) => `${p.name}: ${c.target}`)).toEqual([]);
    }
  });
});
