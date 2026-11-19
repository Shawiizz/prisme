import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildControls, ControlActions, measuredControl } from './controls';

/** The controller profiles shipped with the app. */
const SHIPPED = '../apps/prisme-desktop/controllers';
/** Those checked: the shipped ones, or those of the folder `PRISME_PROFILES` (a profile being
 * written). */
const FOLDER = process.env['PRISME_PROFILES'] ?? SHIPPED;

interface ProfileControl {
  target: string;
  encoding?: string;
  also?: string[];
  down?: string;
  hold?: { target: string };
  double?: { target: string };
  chord?: { target: string };
  set?: unknown;
}

interface Profile {
  name: string;
  controls: ProfileControl[];
  lights?: { target: string }[];
  modes?: { group: string; modes: string[]; select: { mode: string }[] }[];
  meters?: { source: string }[];
  outputs?: { source: string }[];
  displays?: { source: string }[];
}

/** Every action a control drives: its own, those along, its other uses. */
const targetsOf = (c: ProfileControl) =>
  [c.target, ...(c.also ?? []), c.down, c.hold?.target, c.double?.target, c.chord?.target].filter(
    (t): t is string => t !== undefined,
  );

/** The files beside the profiles: their index, their schema and the actions it names. */
const NOT_PROFILES = new Set(['index.json', 'profile.schema.json', 'targets.json']);

const profiles: Profile[] = readdirSync(FOLDER)
  .filter((f) => f.endsWith('.json') && !NOT_PROFILES.has(f))
  .map((f) => JSON.parse(readFileSync(join(FOLDER, f), 'utf8')));

const actions = new Proxy({}, { get: () => () => true }) as ControlActions;
const controls = buildControls(actions);
const ids = new Set(controls.map((c) => c.id));
/** An action of the app, a measured one (`deck1.jump.-8`), a named shift (`shift.left`), or a
 * mode of the profile's. */
const known = (p: Profile, target: string) =>
  ids.has(target) ||
  measuredControl(actions, target) !== undefined ||
  /^shift\.[^.]+$/.test(target) ||
  (p.modes ?? []).some((g) => g.modes.some((m) => target === `mode.${g.group}.${m}`));
const encoders = new Set(controls.filter((c) => c.kind === 'encoder').map((c) => c.id));

describe('controller profiles', () => {
  it('name the actions of the app in their schema (PRISME_WRITE_TARGETS=1 writes them)', () => {
    const targets = controls.map(({ id, group, label, kind }) => ({ id, group, label, kind }));
    const text = JSON.stringify(targets, null, 1) + '\n';
    const file = join(SHIPPED, 'targets.json');
    if (process.env['PRISME_WRITE_TARGETS']) writeFileSync(file, text);
    expect(readFileSync(file, 'utf8').replace(/\r\n/g, '\n')).toEqual(text);
  });

  it('are shipped', () => {
    expect(profiles.length).toBeGreaterThan(0);
  });

  it('drive the controls of the app', () => {
    for (const p of profiles) {
      const targets = [...p.controls.flatMap(targetsOf), ...(p.lights ?? []).map((l) => l.target)];
      const unknown = targets.filter((t) => !known(p, t));
      expect(unknown.map((t) => `${p.name}: ${t}`)).toEqual([]);
    }
  });

  it('choose among the modes they have, and show levels and values the app has', () => {
    const positioned = new Set(controls.filter((c) => c.position !== undefined).map((c) => c.id));
    for (const p of profiles) {
      const strays = (p.modes ?? []).flatMap((g) =>
        g.select.filter((s) => !g.modes.includes(s.mode)).map((s) => `${g.group}.${s.mode}`),
      );
      const levels = (p.meters ?? [])
        .map((m) => m.source)
        .filter((s) => !/^(deck[1-4]|main|main\.left|main\.right)$/.test(s));
      const values = (p.outputs ?? [])
        .map((o) => o.source)
        .filter((s) => !positioned.has(s) && !/^deck[1-4]\.(position|turn)$/.test(s));
      const numbers = (p.displays ?? [])
        .map((d) => d.source)
        .filter((s) => !/^deck[1-4]\.(bpm|elapsed|remaining|duration|tempo|tempoRange)$/.test(s));
      expect([...strays, ...levels, ...values, ...numbers]).toEqual([]);
    }
  });

  it('read steps for encoders, and for faders and keys an encoder turns, never for keys alone', () => {
    for (const p of profiles) {
      const misread = p.controls.filter((c) => {
        const kind = controls.find((k) => k.id === c.target)?.kind;
        if (encoders.has(c.target)) return c.encoding === undefined;
        if (c.down !== undefined) return c.encoding === undefined;
        return c.encoding !== undefined && kind !== 'fader';
      });
      expect(misread.map((c) => `${p.name}: ${c.target}`)).toEqual([]);
    }
  });
});
