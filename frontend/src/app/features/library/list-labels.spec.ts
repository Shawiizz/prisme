import { emptyText } from './list-labels';
import type { LibraryView } from '../../core/models/backend';

describe('emptyText', () => {
  const view = (changes: Partial<LibraryView>) =>
    ({ scope: 'list', source: { kind: 'crate', id: 1 }, unread: null, ...changes }) as LibraryView;

  it('says why the music folder could not be read, before what the list is', () => {
    const unread = { code: 'other', params: {}, message: 'denied' };
    expect(emptyText(view({ unread }), '', null)).toContain('could not be read');
  });

  it('says what an empty list of its kind is, and what a search of the library found', () => {
    expect(emptyText(view({}), '', null)).toBe('This crate is empty: drag tracks onto it');
    const prepare = view({ source: { kind: 'prepare' } });
    expect(emptyText(prepare, '', 'P')).toContain('press P');
    expect(emptyText(view({ scope: 'library' }), 'zz', null)).toBe('No analysed track matches');
  });
});
