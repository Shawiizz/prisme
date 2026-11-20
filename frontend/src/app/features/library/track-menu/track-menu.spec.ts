import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState } from '../../../core/models/engine';
import { LibraryMenu } from '../../../core/models/library';
import { EngineService } from '../../../core/services/engine.service';
import { TrackMenu } from './track-menu';

const MENU: LibraryMenu = {
  count: 1,
  first: null,
  removable: null,
  elsewhere: 0,
  folder: null,
  holds: null,
  genre: 'House',
  separable: 1,
};

describe('TrackMenu', () => {
  afterEach(() => clearMocks());

  /** The menu of the selection, the backend answering once `answer` is called. */
  function opened(start: 'main' | 'genre') {
    let answer: (menu: LibraryMenu) => void = () => {};
    mockIPC((command) =>
      command === 'library_menu' ? new Promise<LibraryMenu>((r) => (answer = r)) : [],
    );
    const told = { stems: { reach: 'full' }, lists: { crates: [], sessions: [] } };
    TestBed.inject(EngineService).controls.set(told as unknown as ControlState);
    const fixture = TestBed.createComponent(TrackMenu);
    fixture.componentRef.setInput('paths', null);
    fixture.componentRef.setInput('start', start);
    fixture.detectChanges();
    const tick = () => new Promise((r) => setTimeout(r));
    return {
      fixture,
      answer: async (menu: LibraryMenu) => {
        await tick();
        answer(menu);
        await tick();
        fixture.detectChanges();
      },
    };
  }

  it('shows what it offers once the backend said it', async () => {
    const { fixture, answer } = opened('main');
    const menu = () => fixture.nativeElement.querySelector('.menu') as HTMLElement;
    expect(menu().classList).toContain('waiting');
    await answer(MENU);
    expect(menu().classList).not.toContain('waiting');
  });

  it('keeps the genre the DJ typed over the one the tracks share', async () => {
    const { fixture, answer } = opened('genre');
    const input = fixture.nativeElement.querySelector('input') as HTMLInputElement;
    input.value = 'Techno';
    input.dispatchEvent(new Event('input'));
    await answer(MENU);
    expect(input.value).toBe('Techno');
  });
});
