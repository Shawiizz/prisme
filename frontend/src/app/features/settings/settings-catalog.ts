import { PHONE } from '../../core/platform';
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

const PAGES: readonly SettingsPage[] = [
  {
    id: 'audio',
    get label() {
      return t('Audio');
    },
    get description() {
      return t('The sound card and how fast it answers.');
    },
    keywords: 'asio wasapi core audio alsa jack latency',
  },
  {
    id: 'turntables',
    get label() {
      return t('Turntables');
    },
    get description() {
      return t('The timecode signal of each record, and how it drives its deck.');
    },
    keywords: 'dvs vinyl timecode serato scope',
  },
  {
    id: 'mixing',
    get label() {
      return t('Mixing');
    },
    get description() {
      return t('Your mixer or the one in Prisme, and how it sounds.');
    },
    keywords: 'crossfader limiter mixer',
  },
  {
    id: 'dj',
    label: 'DJ',
    get description() {
      return t('Sync, tempo fader, quantize and auto gain.');
    },
    keywords: 'sync pitch quantize',
  },
  {
    id: 'library',
    get label() {
      return t('Library');
    },
    get description() {
      return t('Your music files and what Prisme writes into them.');
    },
    keywords: 'tags bpm key',
  },
  {
    id: 'controllers',
    get label() {
      return t('MIDI controllers');
    },
    get description() {
      return t('Map the controls of a controller by moving them.');
    },
    keywords: 'midi learn mapping clock',
  },
  {
    id: 'shortcuts',
    get label() {
      return t('Keyboard shortcuts');
    },
    get description() {
      return t('Every key of the keyboard, as printed on yours.');
    },
    keywords: 'keys keyboard',
  },
  {
    id: 'interface',
    get label() {
      return t('Interface');
    },
    get description() {
      return t('Language of Prisme.');
    },
    keywords: 'language french english',
  },
  {
    id: 'about',
    get label() {
      return t('About');
    },
    get description() {
      return t('Version, updates and help with a problem.');
    },
    keywords: 'version update diagnostic',
  },
];

/** The pages a phone has no use for: it has no keyboard. */
const COMPUTER_ONLY: readonly SettingsCategory[] = ['shortcuts'];

export const SETTINGS_PAGES = PHONE ? PAGES.filter((p) => !COMPUTER_ONLY.includes(p.id)) : PAGES;

export const SETTINGS: readonly SettingEntry[] = [
  {
    id: 'audio.driver',
    page: 'audio',
    get label() {
      return t('Driver type');
    },
    get description() {
      return t('ASIO and JACK answer fastest; the others work with every device.');
    },
  },
  {
    id: 'audio.output',
    page: 'audio',
    get label() {
      return t('Output');
    },
    get description() {
      return t('Where the decks come out.');
    },
  },
  {
    id: 'audio.buffer',
    page: 'audio',
    get label() {
      return t('Buffer');
    },
    get description() {
      return t('Smaller is more responsive; raise it if the sound crackles.');
    },
  },
  {
    id: 'turntables.input',
    page: 'turntables',
    get label() {
      return t('Input (DVS interface)');
    },
    get description() {
      return t('Deck A reads inputs 1-2, deck B inputs 3-4.');
    },
  },
  {
    id: 'turntables.vinyl',
    page: 'turntables',
    get label() {
      return t('Control vinyl');
    },
    get description() {
      return t('The timecode pressed on your records.');
    },
  },
  {
    id: 'turntables.simulate',
    page: 'turntables',
    get label() {
      return t('Simulated turntable');
    },
    get description() {
      return t('A turntable played by Prisme on deck A, to try vinyl control without one.');
    },
  },
  {
    id: 'mixing.mode',
    page: 'mixing',
    get label() {
      return t('Mixing');
    },
    get description() {
      return t('An external mixer takes one output pair per deck.');
    },
  },
  {
    id: 'mixing.headphones',
    page: 'mixing',
    get label() {
      return t('Headphones');
    },
    get description() {
      return t(
        'Where the software mixer plays the cued decks. Automatic takes outputs 3-4 of a DJ interface, never the surround channels of a computer.',
      );
    },
  },
  {
    id: 'mixing.curve',
    page: 'mixing',
    get label() {
      return t('Crossfader curve');
    },
    get description() {
      return t('Smooth for blending, sharp for scratching.');
    },
  },
  {
    id: 'mixing.limiter',
    page: 'mixing',
    get label() {
      return t('Master limiter');
    },
    get description() {
      return t('No clipping on the master, for 1 ms of latency.');
    },
  },
  {
    id: 'dj.sync',
    page: 'dj',
    get label() {
      return t('Sync on a record in relative mode');
    },
    get description() {
      return t('What SYNC does when a record drives the deck.');
    },
  },
  {
    id: 'dj.pitchRange',
    page: 'dj',
    get label() {
      return t('Tempo fader range');
    },
    get description() {
      return t('The travel of the tempo faders, as on your turntables.');
    },
  },
  {
    id: 'dj.quantize',
    page: 'dj',
    get label() {
      return t('Quantize');
    },
    get description() {
      return t(
        'Cues, loops and jumps land on the beat; stems are cut and brought back on the bar.',
      );
    },
  },
  {
    id: 'dj.decks',
    page: 'dj',
    get label() {
      return t('Decks');
    },
    get description() {
      return t(
        'With four decks on an interface for two, C shares the outputs and the turntable of A, D those of B: the deck put on REL or ABS takes the turntable, the other plays on by itself.',
      );
    },
  },
  {
    id: 'dj.autoGain',
    page: 'dj',
    get label() {
      return t('Auto gain');
    },
    get description() {
      return t(
        'Every track plays at the same loudness, measured as it loads: no trim to touch between a quiet one and a loud one.',
      );
    },
  },
  {
    id: 'dj.knobMagnet',
    page: 'dj',
    get label() {
      return t('Magnetic knobs');
    },
    get description() {
      return t(
        'The EQ and filter knobs hold at their middle when turned through it, to find it again without looking.',
      );
    },
  },
  {
    id: 'library.tags',
    page: 'library',
    get label() {
      return t('Write the tempo and key into the tags of the files');
    },
    get description() {
      return t('So that Serato, rekordbox and the others find them.');
    },
  },
  {
    id: 'library.tagsReplace',
    page: 'library',
    get label() {
      return t('Replace those another program wrote');
    },
    get description() {
      return t(
        'A tempo or a key Mixed In Key or another program put in a file is kept unless this is on.',
      );
    },
  },
  {
    id: 'library.pauseWhilePlaying',
    page: 'library',
    get label() {
      return t('Pause the analysis of the library while a deck plays');
    },
    get description() {
      return t(
        'The processor and the disk stay with the sound during a set. A track copied meanwhile is analysed once the decks stop, or as it is loaded.',
      );
    },
  },
  {
    id: 'library.stemsModel',
    page: 'library',
    get label() {
      return t('Vocals separation');
    },
    get description() {
      return t(
        'Downloaded once. A track takes a few seconds with a recent graphics card, a few minutes on the processor alone.',
      );
    },
  },
  {
    id: 'library.stemsDrumsBass',
    page: 'library',
    get label() {
      return t('Drums and bass separation');
    },
    get description() {
      return t(
        'Splits the instrumental into drums, bass and melody, to mute each on its own. Needs the vocals separation, and takes two to three times as long.',
      );
    },
  },
  {
    id: 'library.stemsOnLoad',
    page: 'library',
    get label() {
      return t('Separate the tracks as they are loaded or prepared');
    },
    get description() {
      return t(
        'A track loaded on a deck right away, from where it plays: its stems come within seconds. A track put in Prepare in the background, ready before the set. Off at first without a graphics card for it: the processor alone takes minutes a track.',
      );
    },
  },
  {
    id: 'library.stemsAll',
    page: 'library',
    get label() {
      return t('Separate the whole library');
    },
    get description() {
      return t(
        'The stems are stored next to each track, in a .prisme folder: copy your music to another computer and its separations come along.',
      );
    },
  },
  {
    id: 'interface.language',
    page: 'interface',
    get label() {
      return t('Language');
    },
    get description() {
      return t('The whole interface changes at once.');
    },
  },
  {
    id: 'about.version',
    page: 'about',
    get label() {
      return t('Version');
    },
    get description() {
      return t('Prisme is free software, under the GNU GPL v3.');
    },
  },
  {
    id: 'about.updates',
    page: 'about',
    get label() {
      return t('Updates');
    },
    get description() {
      return t('New versions are offered in the title bar.');
    },
  },
  {
    id: 'about.diagnostics',
    page: 'about',
    get label() {
      return t('Diagnostic report');
    },
    get description() {
      return t('Versions, audio setup and the end of the log, to help understand a problem');
    },
  },
];

/** Lower case without accents, to compare what is typed with what is written. */
export function normalize(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
}

/** The words typed (normalized) are all in `text`. */
export function matches(words: string[], text: string): boolean {
  const haystack = normalize(text);
  return words.every((w) => haystack.includes(w));
}
