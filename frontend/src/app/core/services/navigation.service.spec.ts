import { TestBed } from '@angular/core/testing';
import { NavigationService, librarySight } from './navigation.service';

describe('librarySight', () => {
  it('goes through its tracks for its browsing, its sorts, its selection and its preview', () => {
    const ids = [
      'library.browse',
      'library.next',
      'library.previous',
      'library.page.previous',
      'library.list.open',
      'library.sort.bpm',
      'library.selectAll',
      'library.clearSelection',
      'library.preview',
      'library.preview.seek',
    ];
    expect(ids.filter((id) => librarySight(id) !== 'browse')).toEqual([]);
  });

  it('brings the library up for its search, its lists and what changes them', () => {
    const ids = [
      'library.focus',
      'library.lists',
      'library.list.next',
      'library.left',
      'library.go',
      'library.search',
      'library.remove',
      'library.prepare',
    ];
    expect(ids.filter((id) => librarySight(id) !== 'show')).toEqual([]);
  });

  it('leaves it once a track is loaded from it', () => {
    const ids = ['library.load', 'library.load1', 'library.load4', 'library.load2.play'];
    expect(ids.map(librarySight)).toEqual(['done', 'done', 'done', 'done']);
  });

  it('shows it or leaves it in turn with the key showing it', () => {
    expect(librarySight('general.library')).toBe('toggle');
  });

  it('leaves the screen as it is for the decks, the mixer, the effects and a double', () => {
    const ids = [
      'library.double2',
      'library.preview.stop',
      'deck1.play',
      'deck2.hotcue3',
      'mixer.crossfader',
      'deck1.fx.amount',
      'general.record',
      'autodj.toggle',
      'mode.deck.b',
    ];
    expect(ids.map(librarySight)).toEqual(ids.map(() => null));
  });
});

describe('NavigationService', () => {
  it('asks for the library in sight anew each time an action of it is done', () => {
    const navigation = TestBed.inject(NavigationService);
    navigation.done('deck1.play');
    expect(navigation.libraryWanted()).toBeNull();
    navigation.done('library.lists');
    const first = navigation.libraryWanted();
    navigation.done('library.list.next');
    expect(navigation.libraryWanted()).toEqual({ sight: 'show' });
    expect(navigation.libraryWanted()).not.toBe(first);
    navigation.done('library.load1');
    expect(navigation.libraryWanted()).toEqual({ sight: 'done' });
  });

  it('tells anew each action done that is not the library’s, for its setting to be shown', () => {
    const navigation = TestBed.inject(NavigationService);
    navigation.done('library.next');
    expect(navigation.moved()).toBeNull();
    navigation.done('deck1.vocals');
    const first = navigation.moved();
    navigation.done('deck1.vocals');
    expect(navigation.moved()).toEqual({ id: 'deck1.vocals' });
    expect(navigation.moved()).not.toBe(first);
  });
});
