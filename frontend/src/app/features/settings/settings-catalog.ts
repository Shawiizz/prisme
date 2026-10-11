import { t } from '../../core/i18n/i18n';
import { SettingsCategory } from '../../core/services/navigation.service';

/** A page of the settings, in the sidebar. */
export interface SettingsPage {
  id: SettingsCategory;
  label: string;
  description: string;
  /** More words the search finds the page by. */
  keywords: string;
}

/** A setting: one row of a page, found by the search and the command palette. */
export interface SettingEntry {
  id: string;
  page: SettingsCategory;
  label: string;
  description: string;
}

export const SETTINGS_PAGES: readonly SettingsPage[] = [
  {
    id: 'audio',
    label: t('Audio'),
    description: t('The sound card and how fast it answers.'),
    keywords: 'asio wasapi latency',
  },
  {
    id: 'turntables',
    label: t('Turntables'),
    description: t('The timecode signal of each record, and how it drives its deck.'),
    keywords: 'dvs vinyl timecode serato scope',
  },
  {
    id: 'mixing',
    label: t('Mixing'),
    description: t('Your mixer or the one in Prisme, and how it sounds.'),
    keywords: 'crossfader limiter mixer',
  },
  {
    id: 'dj',
    label: 'DJ',
    description: t('Sync, tempo fader and quantize.'),
    keywords: 'sync pitch quantize',
  },
  {
    id: 'library',
    label: t('Library'),
    description: t('Your music files and what Prisme writes into them.'),
    keywords: 'tags bpm key',
  },
  {
    id: 'controllers',
    label: t('MIDI controllers'),
    description: t('Map the controls of a controller by moving them.'),
    keywords: 'midi learn mapping clock',
  },
  {
    id: 'shortcuts',
    label: t('Keyboard shortcuts'),
    description: t('Every key of the keyboard, as printed on yours.'),
    keywords: 'keys keyboard',
  },
  {
    id: 'interface',
    label: t('Interface'),
    description: t('Language of Prisme.'),
    keywords: 'language french english',
  },
  {
    id: 'about',
    label: t('About'),
    description: t('Version, updates and help with a problem.'),
    keywords: 'version update diagnostic',
  },
];

export const SETTINGS: readonly SettingEntry[] = [
  {
    id: 'audio.driver',
    page: 'audio',
    label: t('Driver type'),
    description: t('ASIO answers fastest; WASAPI works with every Windows device.'),
  },
  {
    id: 'audio.output',
    page: 'audio',
    label: t('Output'),
    description: t('Where the decks come out.'),
  },
  {
    id: 'audio.buffer',
    page: 'audio',
    label: t('Buffer'),
    description: t('Smaller is more responsive; raise it if the sound crackles.'),
  },
  {
    id: 'turntables.input',
    page: 'turntables',
    label: t('Input (DVS interface)'),
    description: t('Deck A reads inputs 1-2, deck B inputs 3-4.'),
  },
  {
    id: 'turntables.vinyl',
    page: 'turntables',
    label: t('Control vinyl'),
    description: t('The timecode pressed on your records.'),
  },
  {
    id: 'turntables.simulate',
    page: 'turntables',
    label: t('Simulated turntable'),
    description: t('A turntable played by Prisme on deck A, to try vinyl control without one.'),
  },
  {
    id: 'mixing.mode',
    page: 'mixing',
    label: t('Mixing'),
    description: t('An external mixer takes one output pair per deck.'),
  },
  {
    id: 'mixing.curve',
    page: 'mixing',
    label: t('Crossfader curve'),
    description: t('Smooth for blending, sharp for scratching.'),
  },
  {
    id: 'mixing.limiter',
    page: 'mixing',
    label: t('Master limiter'),
    description: t('No clipping on the master, for 1 ms of latency.'),
  },
  {
    id: 'dj.sync',
    page: 'dj',
    label: t('Sync on a record in relative mode'),
    description: t('What SYNC does when a record drives the deck.'),
  },
  {
    id: 'dj.pitchRange',
    page: 'dj',
    label: t('Tempo fader range'),
    description: t('The travel of the tempo faders, as on your turntables.'),
  },
  {
    id: 'dj.quantize',
    page: 'dj',
    label: t('Quantize'),
    description: t('Cues, loops and jumps land on the beat.'),
  },
  {
    id: 'library.tags',
    page: 'library',
    label: t('Write the tempo and key into the tags of the files'),
    description: t('So that Serato, rekordbox and the others find them.'),
  },
  {
    id: 'interface.language',
    page: 'interface',
    label: t('Language'),
    description: t('The interface starts again in it.'),
  },
  {
    id: 'about.version',
    page: 'about',
    label: t('Version'),
    description: t('Prisme is free software, under the GNU GPL v3.'),
  },
  {
    id: 'about.updates',
    page: 'about',
    label: t('Updates'),
    description: t('New versions are offered in the title bar.'),
  },
  {
    id: 'about.diagnostics',
    page: 'about',
    label: t('Diagnostic report'),
    description: t('Versions, audio setup and the end of the log, to help understand a problem'),
  },
];

/** Lower case without accents, to compare what is typed with what is written. */
export function normalize(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
}
