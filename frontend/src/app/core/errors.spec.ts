import { errorText } from './errors';

describe('errorText', () => {
  it('words a failure of the backend from its code', () => {
    const noTrack = { code: 'noTrack', params: { deck: 1 }, message: 'no track on deck 2' };
    expect(errorText(noTrack)).toBe('No track on deck B');
    const genre = { code: 'genreNotWritten', params: { count: 3, file: 'a.mp3' }, message: '' };
    expect(errorText(genre)).toBe('Genre not written into 3 files');
  });

  it('names the USB card the DJ is asked for, or refused', () => {
    const asked = { code: 'usbAsked', params: { card: 'UMC404HD 192k' }, message: '' };
    expect(errorText(asked)).toBe('Allow Prisme to use the card UMC404HD 192k');
    const refused = { code: 'usbRefused', params: { card: 'UMC404HD 192k' }, message: '' };
    expect(errorText(refused)).toBe(
      'The card UMC404HD 192k was not allowed: choose it again in the settings',
    );
  });

  it('falls back on the message of a code it does not know', () => {
    const failure = { code: 'library', params: {}, message: 'database is locked' };
    expect(errorText(failure)).toBe('database is locked');
  });

  it('shows anything else as it says it', () => {
    expect(errorText('the updater is offline')).toBe('the updater is offline');
  });
});
