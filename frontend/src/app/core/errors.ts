import { deckLetter } from '../shared/time';
import { Params, t } from './i18n/i18n';

/** A failure of the backend (`error.rs`): a code the UI words in its language, with what it
 * needs to word it, and the English message for the codes it does not know. */
export interface BackendError {
  code: string;
  params: Params;
  message: string;
}

const onDeck = (p: Params) => ({ ...p, deck: deckLetter(Number(p['deck'])) });

/** The words of each code. */
const TEXTS: Record<string, (p: Params) => string> = {
  audioNotRunning: () => t('The audio is not running'),
  noStemsModel: () => t('The separation model is not installed'),
  invalidDeck: (p) => t('There is no deck {deck}', onDeck(p)),
  noTrack: (p) => t('No track on deck {deck}', onDeck(p)),
  emptyCrateName: () => t('A crate needs a name'),
  smartCrate: () => t('A smart crate picks its own tracks'),
  noBeatGrid: (p) => t('Deck {deck} has no beat grid', onDeck(p)),
  invalidBpm: (p) => t('{bpm} BPM is out of range', p),
  invalidKey: (p) => t('Unknown key {key}', p),
  sampleTooLong: (p) =>
    t('A sampler sound lasts {seconds} s at most: load longer files on a deck', p),
  controllerUnplugged: (p) => t('Controller {controller} is not plugged in', p),
  unknownFormat: (p) => t('Unknown timecode {format}', p),
  engineBusy: () => t('The engine is busy, try again'),
  inputsNotAllowed: () =>
    t('Allow Prisme to listen to the inputs of the sound card, then apply again'),
  notTheUpdate: () => t('The file downloaded is not the new version of Prisme'),
  alreadyRecording: () => t('Already recording'),
  silentMaster: () =>
    t('Your own mixer mixes: the master of Prisme plays nothing. Record the inputs of your mixer'),
  noPreviewOutput: () =>
    t('No headphones of their own to preview in: choose their outputs in the settings'),
  genreNotWritten: (p) =>
    p['count'] === 1
      ? t('Genre not written into {file}', p)
      : t('Genre not written into {count} files', p),
  noOutputDevice: () => t('No audio output found'),
  deviceNotFound: (p) =>
    p['device']
      ? t('Audio device not found: {device}', p)
      : t('No sound card found: plug it in, and let Prisme use it if asked'),
  backendUnavailable: (p) => t('{backend} is not available on this computer', p),
  sampleRateMismatch: (p) =>
    t(
      'The input runs at {input} Hz but the output at {output} Hz: set both devices to the same rate',
      p,
    ),
  noClick: (p) =>
    t('No click came back from output {output} to input {input}: is the cable there?', p),
  cannotOpen: (p) => t('Cannot open {path}', p),
  unreadableFile: () => t('Unsupported or damaged file'),
  folderNotFound: () => t('This folder cannot be found'),
  folderAlreadyIn: (p) => t('Already among your folders, in {folder}', p),
  folderHoldsAnother: (p) => t('It holds {folder}, already one of your folders', p),
  foldersFixed: () => t('The folders of the library cannot be changed on this device'),
  mainFolderStays: () => t('The main folder stays: change it in the settings'),
};

/** What failed, to show: a failure of the backend in the UI's language, anything else as it
 * says it. Worded as it happens: an error shown keeps its words if the language changes. */
export function errorText(error: unknown): string {
  if (!isBackendError(error)) return String(error);
  return TEXTS[error.code]?.(error.params) ?? error.message;
}

export function isBackendError(error: unknown): error is BackendError {
  return typeof error === 'object' && error !== null && 'code' in error && 'message' in error;
}
