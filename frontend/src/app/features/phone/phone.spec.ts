import { Component, input } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState } from '../../core/models/engine';
import { EngineService } from '../../core/services/engine.service';
import { NavigationService } from '../../core/services/navigation.service';
import { SETTINGS_PAGES } from '../settings/settings-catalog';

@Component({ selector: 'app-phone-decks', template: '' })
class Decks {
  readonly touch = input(false);
}

@Component({ selector: 'app-settings-dialog', template: '' })
class Page {}

@Component({ selector: 'app-phone-library', template: '' })
class Library {}

describe('Phone', () => {
  let fixture: ComponentFixture<unknown>;
  let navigation: NavigationService;
  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  beforeEach(async () => {
    // the canvases' sizes are watched from the module's load, which the test runner cannot do
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const { Phone } = await import('./phone');
    const { PhoneDecks } = await import('./phone-decks');
    const { PhoneLibrary } = await import('./phone-library');
    const { PhoneSettings } = await import('./phone-settings');
    const { SettingsDialog } = await import('../settings/settings-dialog');
    mockIPC(() => null);
    TestBed.overrideComponent(Phone, {
      remove: { imports: [PhoneDecks, PhoneLibrary] },
      add: { imports: [Decks, Library] },
    });
    TestBed.overrideComponent(PhoneSettings, {
      remove: { imports: [SettingsDialog] },
      add: { imports: [Page] },
    });
    const told = {
      audio: { settings: null, info: null },
      routing: { recordOptions: [] },
      controllers: { canLearn: false },
      preferences: { ui: { phoneTouch: true } },
    };
    TestBed.inject(EngineService).controls.set(told as unknown as ControlState);
    navigation = TestBed.inject(NavigationService);
    fixture = TestBed.createComponent(Phone);
    fixture.detectChanges();
  });

  const element = () => fixture.nativeElement as HTMLElement;
  const [DECKS_TAB, LIBRARY_TAB, SETTINGS_TAB] = [0, 1, 2];
  /** The screen shown: decks, library or settings. */
  const shown = () => {
    const tabs = [...element().querySelectorAll('nav.tabs button')];
    return tabs.findIndex((tab) => tab.getAttribute('aria-current') === 'page');
  };
  /** What a controller, a key or the palette did, as the backend tells it. */
  const done = (id: string) => {
    navigation.done(id);
    fixture.detectChanges();
  };
  const touch = (tab: number) => {
    element().querySelectorAll<HTMLButtonElement>('nav.tabs button')[tab].click();
    fixture.detectChanges();
  };
  const pageShown = () => element().querySelector('app-settings-dialog') !== null;

  it('opens the settings where they were left, and their list when touched again', () => {
    touch(SETTINGS_TAB);
    navigation.openSettings('library');
    fixture.detectChanges();
    expect(pageShown()).toBe(true);

    touch(0);
    touch(SETTINGS_TAB);
    expect(pageShown()).toBe(true);
    expect(navigation.settings()?.category).toBe('library');

    touch(SETTINGS_TAB);
    expect(pageShown()).toBe(false);
    expect(navigation.settings()).toBeNull();
  });

  it('leaves a search for the list of the pages when touched again', () => {
    touch(SETTINGS_TAB);
    const search = element().querySelector<HTMLInputElement>('app-phone-settings input')!;
    search.value = 'nothing by this name';
    search.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    expect(element().querySelector('app-phone-settings .none')).not.toBeNull();

    touch(SETTINGS_TAB);
    expect(search.value).toBe('');
    const pages = element().querySelectorAll('app-phone-settings .pages button');
    expect(pages.length).toBe(SETTINGS_PAGES.length);
  });

  it('brings the library up for what is done in it, and goes back once a track is loaded', () => {
    done('deck1.play');
    done('mixer.crossfader');
    expect(shown()).toBe(DECKS_TAB);
    done('library.browse');
    expect(shown()).toBe(LIBRARY_TAB);
    done('library.double2');
    done('library.preview.stop');
    expect(shown()).toBe(LIBRARY_TAB);
    done('library.load1');
    expect(shown()).toBe(DECKS_TAB);
    done('library.load2');
    expect(shown()).toBe(DECKS_TAB);
  });

  it('stays on a library the DJ chose when a track is loaded from it', () => {
    touch(LIBRARY_TAB);
    done('library.next');
    done('library.load1');
    expect(shown()).toBe(LIBRARY_TAB);
  });

  it('goes back to the screen it brought the library over, unless the DJ chose another', () => {
    touch(SETTINGS_TAB);
    done('library.list.next');
    expect(shown()).toBe(LIBRARY_TAB);
    done('library.load1.play');
    expect(shown()).toBe(SETTINGS_TAB);

    done('library.browse');
    touch(DECKS_TAB);
    touch(LIBRARY_TAB);
    done('library.load1');
    expect(shown()).toBe(LIBRARY_TAB);
  });

  it('shows the library and leaves it in turn with the key showing it', () => {
    done('general.library');
    expect(shown()).toBe(LIBRARY_TAB);
    done('general.library');
    expect(shown()).toBe(DECKS_TAB);
  });

  it('closes the palette over the library it brings up', () => {
    navigation.palette.set(true);
    done('library.browse');
    expect(navigation.palette()).toBe(false);
    expect(shown()).toBe(LIBRARY_TAB);
  });
});
