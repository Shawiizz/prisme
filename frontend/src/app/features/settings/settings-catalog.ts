import { ANDROID, PHONE } from '../../core/platform';
import { Text, t } from '../../core/i18n/i18n';
import { SettingsCategory } from '../../core/services/navigation.service';

/** A page of the settings, in the sidebar. */
export interface SettingsPage {
  id: SettingsCategory;
  label: Text;
  description: Text;
  /** More words the search finds the page by. */
  keywords: string;
}

/** A setting: one row of a page, found by the search and the command palette. */
export interface SettingEntry {
  id: string;
  page: SettingsCategory;
  label: Text;
  description: Text;
}

const PAGES: readonly SettingsPage[] = [
  {
    id: 'audio',
    label: () => t('Audio'),
    description: () => t('The sound card and how fast it answers.'),
    keywords: 'asio wasapi core audio alsa jack latency',
  },
  {
    id: 'turntables',
    label: () => t('Turntables'),
    description: () => t('The timecode signal of each record, and how it drives its deck.'),
    keywords: 'dvs vinyl timecode serato scope',
  },
  {
    id: 'mixing',
    label: () => t('Mixing'),
    description: () => t('Your mixer or the one in Prisme, and how it sounds.'),
    keywords: 'crossfader limiter mixer booth microphone mic aux line talkover balance',
  },
  {
    id: 'dj',
    label: () => 'DJ',
    description: () => t('Sync, tempo fader, quantize and auto gain.'),
    keywords: 'sync pitch quantize',
  },
  {
    id: 'library',
    label: () => t('Library'),
    description: () => t('Your music files and what Prisme writes into them.'),
    keywords: 'tags bpm key',
  },
  {
    id: 'controllers',
    label: () => t('MIDI controllers'),
    description: () => t('Map the controls of a controller by moving them.'),
    keywords: 'midi learn mapping clock',
  },
  {
    id: 'shortcuts',
    label: () => t('Keyboard shortcuts'),
    description: () => t('Every key of the keyboard, as printed on yours.'),
    keywords: 'keys keyboard',
  },
  {
    id: 'interface',
    label: () => t('Interface'),
    description: () => t('Language of Prisme.'),
    keywords: 'language french english',
  },
  {
    id: 'about',
    label: () => t('About'),
    description: () => t('Version, updates and help with a problem.'),
    keywords: 'version update diagnostic',
  },
];

/** The pages a phone has no use for: it has no keyboard. */
const COMPUTER_ONLY: readonly SettingsCategory[] = ['shortcuts'];

export const SETTINGS_PAGES = PHONE ? PAGES.filter((p) => !COMPUTER_ONLY.includes(p.id)) : PAGES;

const ALL_SETTINGS: readonly SettingEntry[] = [
  {
    id: 'audio.driver',
    page: 'audio',
    label: () => t('Driver type'),
    description: () => t('ASIO and JACK answer fastest; the others work with every device.'),
  },
  {
    id: 'audio.output',
    page: 'audio',
    label: () => t('Output'),
    description: () => t('Where the decks come out.'),
  },
  {
    id: 'audio.buffer',
    page: 'audio',
    label: () => t('Buffer'),
    description: () => t('Smaller is more responsive; raise it if the sound crackles.'),
  },
  {
    id: 'audio.latency',
    page: 'audio',
    label: () => t('Latency'),
    description: () =>
      t(
        'Join an output to an input with a cable and measure: the time from the turntables to the speakers through Prisme, with the devices in use.',
      ),
  },
  {
    id: 'turntables.input',
    page: 'turntables',
    label: () => t('Input (DVS interface)'),
    description: () => t('Deck A reads inputs 1-2, deck B inputs 3-4.'),
  },
  {
    id: 'turntables.vinyl',
    page: 'turntables',
    label: () => t('Control vinyl'),
    description: () => t('The timecode pressed on your records.'),
  },
  {
    id: 'turntables.simulate',
    page: 'turntables',
    label: () => t('Simulated turntable'),
    description: () =>
      t('A turntable played by Prisme on deck A, to try vinyl control without one.'),
  },
  {
    id: 'mixing.mode',
    page: 'mixing',
    label: () => t('Mixing'),
    description: () => t('An external mixer takes one output pair per deck.'),
  },
  {
    id: 'mixing.headphones',
    page: 'mixing',
    label: () => t('Headphones'),
    description: () =>
      t(
        'Where the software mixer plays the cued decks. Automatic takes outputs 3-4 of a DJ interface, never the surround channels of a computer.',
      ),
  },
  {
    id: 'mixing.booth',
    page: 'mixing',
    label: () => t('Booth'),
    description: () =>
      t('The monitors of the booth: the master on a pair of its own, at its own level.'),
  },
  {
    id: 'mixing.mic',
    page: 'mixing',
    label: () => t('Microphone'),
    description: () =>
      t(
        'On an input the turntables leave free, mixed into the master; its key is beside the crossfader. Heard in the headphones only when cued there.',
      ),
  },
  {
    id: 'mixing.aux',
    page: 'mixing',
    label: () => t('Aux line'),
    description: () =>
      t('A phone, a CD player or a second mixer on a free pair of inputs, mixed into the master.'),
  },
  {
    id: 'mixing.talkover',
    page: 'mixing',
    label: () => t('Talkover'),
    description: () => t('While the microphone is on with TALK, the music ducks by this much.'),
  },
  {
    id: 'mixing.curve',
    page: 'mixing',
    label: () => t('Crossfader curve'),
    description: () => t('Smooth for blending, sharp for scratching.'),
  },
  {
    id: 'mixing.limiter',
    page: 'mixing',
    label: () => t('Master limiter'),
    description: () => t('No clipping on the master, for 1 ms of latency.'),
  },
  {
    id: 'dj.sync',
    page: 'dj',
    label: () => t('Sync on a record in relative mode'),
    description: () => t('What SYNC does when a record drives the deck.'),
  },
  {
    id: 'dj.pitchRange',
    page: 'dj',
    label: () => t('Tempo fader range'),
    description: () =>
      t(
        'The travel of the tempo faders, as on your turntables: of them all, each deck can then have its own in its menu.',
      ),
  },
  {
    id: 'dj.quantize',
    page: 'dj',
    label: () => t('Quantize'),
    description: () =>
      t('Cues, loops and jumps land on the beat; stems are cut and brought back on the bar.'),
  },
  {
    id: 'dj.decks',
    page: 'dj',
    label: () => t('Decks'),
    description: () =>
      t(
        'With four decks on an interface for two, C shares the outputs and the turntable of A, D those of B: the deck put on REL or ABS takes the turntable, the other plays on by itself.',
      ),
  },
  {
    id: 'dj.autoGain',
    page: 'dj',
    label: () => t('Auto gain'),
    description: () =>
      t(
        'Every track plays at the same loudness, measured as it loads: no trim to touch between a quiet one and a loud one.',
      ),
  },
  {
    id: 'dj.knobMagnet',
    page: 'dj',
    label: () => t('Magnetic knobs'),
    description: () =>
      t(
        'The EQ and filter knobs hold at their middle when turned through it, to find it again without looking.',
      ),
  },
  {
    id: 'dj.lockControls',
    page: 'dj',
    label: () => t('Controls on the lock screen'),
    description: () =>
      t(
        'Play and pause each deck from the player of the lock screen. Off for a gig: nothing a pocket can press.',
      ),
  },
  {
    id: 'dj.cueOnLoad',
    page: 'dj',
    label: () => t('Headphones on the deck a track is loaded on'),
    description: () =>
      t('A track loaded on a deck puts that deck alone in the headphones, to cue it at once.'),
  },
  {
    id: 'dj.protectPlaying',
    page: 'dj',
    label: () => t('Never replace a playing deck'),
    description: () =>
      t('A track loaded onto a deck that plays is refused with a word, rather than asked about.'),
  },
  {
    id: 'library.tags',
    page: 'library',
    label: () => t('Write the tempo and key into the tags of the files'),
    description: () => t('So that Serato, rekordbox and the others find them.'),
  },
  {
    id: 'library.tagsReplace',
    page: 'library',
    label: () => t('Replace those another program wrote'),
    description: () =>
      t(
        'A tempo or a key Mixed In Key or another program put in a file is kept unless this is on.',
      ),
  },
  {
    id: 'library.pauseWhilePlaying',
    page: 'library',
    label: () => t('Pause the analysis of the library while a deck plays'),
    description: () =>
      t(
        'The processor and the disk stay with the sound during a set. A track copied meanwhile is analysed once the decks stop, or as it is loaded.',
      ),
  },
  {
    id: 'library.stemsFast',
    page: 'library',
    label: () => t('Separation'),
    description: () =>
      t(
        'Quality: the large models, the cleanest stems. Fast: one small model for every stem, several times faster, for a phone or a computer without a graphics card worth it.',
      ),
  },
  {
    id: 'library.stemsModel',
    page: 'library',
    label: () => t('Vocals separation'),
    description: () =>
      t(
        'Downloaded once. A track takes a few seconds with a recent graphics card, a few minutes on the processor alone.',
      ),
  },
  {
    id: 'library.stemsFour',
    page: 'library',
    label: () => t('Separation into stems'),
    description: () =>
      t(
        'Vocals, drums, bass and melody from one small model, downloaded once, to mute each on its own.',
      ),
  },
  {
    id: 'library.stemsDrumsBass',
    page: 'library',
    label: () => t('Drums and bass separation'),
    description: () =>
      t(
        'Splits the instrumental into drums, bass and melody, to mute each on its own. Needs the vocals separation, and takes two to three times as long.',
      ),
  },
  {
    id: 'library.stemsGpu',
    page: 'library',
    label: () => t('Hardware acceleration'),
    description: () =>
      t(
        'The graphics chip separates the tracks, many times faster than the processor alone. Off, a track takes minutes: none is separated as it loads unless asked.',
      ),
  },
  {
    id: 'library.stemsOnLoad',
    page: 'library',
    label: () => t('Separate the tracks as they are loaded or prepared'),
    description: () =>
      t(
        'A track loaded on a deck right away, from where it plays: its stems come within seconds. A track put in Prepare in the background, ready before the set. Off at first without a graphics card for it: the processor alone takes minutes a track.',
      ),
  },
  {
    id: 'library.stemsAll',
    page: 'library',
    label: () => t('Separate the whole library'),
    description: () =>
      t(
        'The stems are stored next to each track, in a .prisme folder: copy your music to another computer and its separations come along.',
      ),
  },
  {
    id: 'interface.language',
    page: 'interface',
    label: () => t('Language'),
    description: () => t('The whole interface changes at once.'),
  },
  {
    id: 'interface.screenBelow',
    page: 'interface',
    label: () => t('Decks on the screen below'),
    description: () =>
      t(
        'On a laptop with two screens, or with a screen set under the main one: the library above, the decks below, under the fingers.',
      ),
  },
  {
    id: 'about.version',
    page: 'about',
    label: () => t('Version'),
    description: () => t('Prisme is free software, under the GNU GPL v3.'),
  },
  {
    id: 'about.updates',
    page: 'about',
    label: () => t('Updates'),
    description: () => t('New versions are offered in the title bar.'),
  },
  {
    id: 'about.diagnostics',
    page: 'about',
    label: () => t('Diagnostic report'),
    description: () =>
      t('Versions, audio setup and the end of the log, to help understand a problem'),
  },
];

/** The settings a phone has no use for: it has one screen. */
const COMPUTER_ONLY_SETTINGS: readonly string[] = ['interface.screenBelow'];
/** The settings only Android has the means of yet. */
const ANDROID_ONLY_SETTINGS: readonly string[] = ['dj.lockControls'];

export const SETTINGS = ALL_SETTINGS.filter(
  (s) =>
    !(PHONE && COMPUTER_ONLY_SETTINGS.includes(s.id)) &&
    (ANDROID || !ANDROID_ONLY_SETTINGS.includes(s.id)),
);

export const SETTING_BY_ID: ReadonlyMap<string, SettingEntry> = new Map(
  SETTINGS.map((s) => [s.id, s]),
);

/** Lower case without accents, to compare what is typed with what is written. */
export function normalize(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
}

/** The words typed (normalized) are all in `text`. */
export function matches(words: string[], text: string): boolean {
  const haystack = normalize(text);
  return words.every((w) => haystack.includes(w));
}
