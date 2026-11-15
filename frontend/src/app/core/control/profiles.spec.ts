import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildControls, ControlActions, measuredControl } from './controls';

/** The controller profiles shipped with the app (`apps/prisme-desktop/controllers`). */
const FOLDER = '../apps/prisme-desktop/controllers';

interface Profile {
  name: string;
  controls: { target: string; encoding?: string }[];
  lights?: { target: string }[];
  modes?: { group: string; modes: string[]; select: { mode: string }[] }[];
  meters?: { source: string }[];
}

const profiles: Profile[] = readdirSync(FOLDER)
  .filter((f) => f.endsWith('.json'))
  .map((f) => JSON.parse(readFileSync(join(FOLDER, f), 'utf8')));

const actions = new Proxy({}, { get: () => () => true }) as ControlActions;
const controls = buildControls(actions);
const ids = new Set(controls.map((c) => c.id));
/** An action of the app, a measured one (`deck1.jump.-8`), or a mode of the profile's. */
const known = (p: Profile, target: string) =>
  ids.has(target) ||
  measuredControl(actions, target) !== undefined ||
  (p.modes ?? []).some((g) => g.modes.some((m) => target === `mode.${g.group}.${m}`));
const encoders = new Set(controls.filter((c) => c.kind === 'encoder').map((c) => c.id));

describe('controller profiles', () => {
  it('are shipped', () => {
    expect(profiles.length).toBeGreaterThan(0);
  });

  it('drive the controls of the app', () => {
    for (const p of profiles) {
      const unknown = [...p.controls, ...(p.lights ?? [])].filter((c) => !known(p, c.target));
      expect(unknown.map((c) => `${p.name}: ${c.target}`)).toEqual([]);
    }
  });

  it('choose among the modes they have, and show levels the app has', () => {
    for (const p of profiles) {
      const strays = (p.modes ?? []).flatMap((g) =>
        g.select.filter((s) => !g.modes.includes(s.mode)).map((s) => `${g.group}.${s.mode}`),
      );
      const sources = (p.meters ?? []).filter((m) => !/^(deck[1-4]|main)$/.test(m.source));
      expect([...strays, ...sources.map((m) => m.source)]).toEqual([]);
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
