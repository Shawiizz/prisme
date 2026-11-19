import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** The profiles shipped with the app: read at build time, so the site never lags behind. */
export const CONTROLLERS_DIR = resolve(process.cwd(), '../apps/prisme-desktop/controllers');

/** Brands named in more than one word; the others are a model name's first word. */
const BRANDS = [
	'Allen & Heath',
	'American Audio',
	'DJ TechTools',
	'Keith McMillen',
	'Kontrol DJ',
	'Native Instruments',
];
const NOT_PROFILES = new Set(['index.json', 'targets.json', 'profile.schema.json']);

export interface ControllerModel {
	file: string;
	name: string;
	brand: string;
	model: string;
	credits: string;
	controls: number;
	lights: number;
	meters: number;
	displays: number;
	outputs: number;
	jog: boolean;
	/** Lights given values of their own: controls naming theirs, and the profile's other lights. */
	litControls: number;
	/** What its screens show, from its displays' sources: `tempo`, `time`, `pitch`. */
	screens: string[];
	/** What its rings of lights show, from its outputs' sources: `jog`, `position`, `knobs`. */
	rings: string[];
	/** Tried on the hardware: the others are faithful conversions not tried yet. */
	verified: boolean;
}

const SCREEN_OF: Record<string, string> = {
	bpm: 'tempo',
	elapsed: 'time',
	remaining: 'time',
	duration: 'time',
	tempo: 'pitch',
	tempoRange: 'pitch',
};

/** What a display's or an output's `source` shows, without its deck. */
const measure = (source: string) => source.replace(/^deck\d\./, '');

const kinds = (sources: string[], kindOf: (measure: string) => string) => [
	...new Set(sources.map((s) => kindOf(measure(s)))),
];

export function controllerModels(): ControllerModel[] {
	return readdirSync(CONTROLLERS_DIR)
		.filter((f) => f.endsWith('.json') && !NOT_PROFILES.has(f))
		.map((file) => {
			const p = JSON.parse(readFileSync(resolve(CONTROLLERS_DIR, file), 'utf8'));
			const brand = BRANDS.find((b) => p.name.startsWith(b)) ?? p.name.split(' ')[0];
			const controls: { target?: string; light?: object; palette?: object }[] = p.controls ?? [];
			const displays: { source: string }[] = p.displays ?? [];
			const outputs: { source: string }[] = p.outputs ?? [];
			return {
				file,
				name: p.name,
				brand,
				model: p.name.slice(brand.length).trim(),
				credits: p.credits ?? '',
				controls: controls.length,
				lights: (p.lights ?? []).length,
				meters: (p.meters ?? []).length,
				displays: (p.displays ?? []).length,
				outputs: (p.outputs ?? []).length,
				jog: controls.some((c) => /\.jog/.test(c.target ?? '')),
				litControls:
					controls.filter((c) => c.light !== undefined || c.palette !== undefined).length +
					(p.lights ?? []).length,
				screens: kinds(
					displays.map((d) => d.source),
					(m) => SCREEN_OF[m] ?? m,
				),
				rings: kinds(
					outputs.map((o) => o.source),
					(m) => (m === 'turn' ? 'jog' : m === 'position' ? 'position' : 'knobs'),
				),
				verified: p.verified === true,
			};
		})
		.sort((a, b) => a.name.localeCompare(b.name));
}

export function brands(models = controllerModels()): string[] {
	return [...new Set(models.map((m) => m.brand))];
}

export interface Target {
	id: string;
	group: string;
	label: string;
	kind: 'button' | 'fader' | 'encoder' | 'light';
}

export function targets(): Target[] {
	return JSON.parse(readFileSync(resolve(CONTROLLERS_DIR, 'targets.json'), 'utf8'));
}

/** An action as the reference lists it: those numbered alike (`deckN.hotcue1`…) in one row. */
export interface ActionRow {
	/** `deckN.` for a deck's action; `<n>` where a numbered action's number goes. */
	id: string;
	label: string;
	kind: Target['kind'];
	/** The numbers `<n>` takes, first and last. */
	numbers?: [number, number];
}

export interface ActionGroup {
	name: string;
	/** The actions of each deck, listed once. */
	deck: boolean;
	rows: ActionRow[];
}

/** The deck actions numbered one by one, as `deckN.<name><n><rest>`. */
const NUMBERED = /^deckN\.(hotcue|sample|slice)(\d+)((?:\.[a-z]+)*)$/;

/** The actions a control can drive, by group: those of deck A for every deck, as `deckN.`. */
export function actionGroups(all = targets()): ActionGroup[] {
	const groups = new Map<string, ActionGroup>();
	for (const t of all) {
		const deck = /^deck\d\./.test(t.id);
		if (deck && !t.id.startsWith('deck1.')) continue;
		const name = deck ? 'Decks' : t.group;
		const group = groups.get(name) ?? { name, deck, rows: [] };
		groups.set(name, group);
		const id = t.id.replace(/^deck1\./, 'deckN.');
		const numbered = NUMBERED.exec(id);
		if (!numbered) {
			group.rows.push({ id, label: t.label, kind: t.kind });
			continue;
		}
		const [, action, n, rest] = numbered;
		const template = `deckN.${action}<n>${rest}`;
		const row = group.rows.find((r) => r.id === template);
		if (row?.numbers) row.numbers[1] = Number(n);
		else {
			const label = t.label.replace(new RegExp(`\\b${n}\\b`), '<n>');
			group.rows.push({ id: template, label, kind: t.kind, numbers: [Number(n), Number(n)] });
		}
	}
	return [...groups.values()];
}
