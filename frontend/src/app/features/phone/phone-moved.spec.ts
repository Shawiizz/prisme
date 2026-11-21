import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ControlState } from '../../core/models/engine';
import { EngineService } from '../../core/services/engine.service';
import { NavigationService } from '../../core/services/navigation.service';
import { PhoneMoved, SHOWN_MS } from './phone-moved';
import { PhoneSight } from './phone-sight';
import type { DeckSettings } from '../../core/models/backend';

describe('PhoneMoved', () => {
  let fixture: ComponentFixture<PhoneMoved>;
  let navigation: NavigationService;

  beforeEach(() => {
    vi.useFakeTimers();
    TestBed.configureTestingModule({ providers: [PhoneSight] });
    const engine = TestBed.inject(EngineService);
    const knob = (at: number) => ({ at, home: 0.5, reads: { db: (at - 0.5) * 24 } });
    const knobs = { trim: knob(0.5), eq: [knob(0.25), knob(0.5), knob(0.5)], filter: knob(0.5) };
    const strip = { kills: [false, false, false] };
    engine.decks.set([{ volume: 0.8, knobs, strip } as unknown as DeckSettings]);
    engine.controls.set({ mixer: { knobs: {} } } as unknown as ControlState);
    navigation = TestBed.inject(NavigationService);
    fixture = TestBed.createComponent(PhoneMoved);
    fixture.detectChanges();
  });

  afterEach(() => vi.useRealTimers());

  /** What a controller or a key moved, as the backend tells it. */
  const moved = (id: string) => {
    navigation.done(id);
    fixture.detectChanges();
  };
  const shown = () => {
    fixture.detectChanges();
    const element = fixture.nativeElement as HTMLElement;
    const parts = element.querySelector('.moved')?.children;
    if (!parts) return null;
    const texts = [...parts].map((part) => part.textContent!.trim());
    return texts.filter(Boolean).join(' ');
  };
  const wait = (ms: number) => vi.advanceTimersByTime(ms);

  it('shows the setting moved with what it reads, the latest over the one before', () => {
    moved('deck1.trim');
    expect(shown()).toBe('A TRIM 0.0 dB');
    moved('deck1.eq.low');
    expect(shown()).toBe('A EQ LOW −6.0 dB');
    moved('deck1.play');
    expect(shown()).toBe('A EQ LOW −6.0 dB');
  });

  it('fades once the setting has not moved for a moment', () => {
    moved('deck1.volume');
    wait(SHOWN_MS - 100);
    expect(shown()).toBe('A VOL');
    moved('deck1.volume');
    wait(SHOWN_MS - 100);
    expect(shown()).toBe('A VOL');
    wait(100);
    expect(fixture.componentInstance['shown']()).toBeNull();
  });

  it('shows nothing of a setting the screen shows', () => {
    const channel = signal({ kind: 'channel' as const, deck: 0 });
    const gone = TestBed.inject(PhoneSight).show(channel);
    moved('deck1.volume');
    expect(shown()).toBeNull();
    moved('deck1.trim');
    expect(shown()).toBe('A TRIM 0.0 dB');
    gone();
    moved('deck1.volume');
    expect(shown()).toBe('A VOL');
  });
});
