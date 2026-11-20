import { Component, DestroyRef, computed, inject, input } from '@angular/core';
import { deckTempo } from '../../../core/deck-tempo';
import { EngineService } from '../../../core/services/engine.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { pitchToMatch } from '../../../core/sync';
import { formatPitch } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { deckStems } from '../../../core/deck-stems';
import { Knob } from '../../../shared/knob/knob';
import { readouts } from '../../../shared/mixer-scales';
import { Press } from '../../../shared/press';

/** Holding SYNC this long (ms) matches the leader once instead of locking to it. */
const SYNC_HOLD_MS = 450;

/**
 * What plays a deck, at the start of its rail: the transport (a record has its own), the
 * stems, sync, and the pitch with key lock.
 */
@Component({
  selector: 'app-deck-controls',
  templateUrl: './deck-controls.html',
  styleUrl: './deck-controls.scss',
  imports: [Knob, TPipe, Press],
  host: { '[class.plays]': '!vinyl()' },
})
export class DeckControls {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);
  private readonly tempo = deckTempo(this.deck);
  protected readonly status = this.tempo.status;
  protected readonly settings = this.tempo.settings;
  protected readonly vinyl = this.tempo.vinyl;
  protected readonly syncState = this.tempo.syncState;
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  private readonly stemsOf = deckStems(this.deck);
  protected readonly separation = this.stemsOf.separation;
  protected readonly stemKnobs = this.stemsOf.knobs;
  protected readonly separating = this.stemsOf.separating;
  protected readonly canSeparate = this.stemsOf.canSeparate;
  protected readonly resumable = this.stemsOf.resumable;
  protected readonly separate = this.stemsOf.separate;
  protected readonly percent = readouts.percent;

  private readonly absolute = computed(
    () => this.vinyl() && this.settings().vinylMode === 'absolute',
  );

  protected readonly canSync = computed(() => {
    const s = this.status();
    return !!s?.loaded && s.bpm !== null && !this.absolute();
  });
  protected readonly syncTitle = computed(() => {
    if (this.absolute()) return t('Sync needs relative mode or internal play');
    if (this.status()?.bpm == null) return t('No beat grid');
    const once = this.vinyl() && this.preferences.dj().vinylSync === 'once';
    return once
      ? t('Match the tempo and beats once')
      : t('Tap: lock to the leader · hold: match its tempo and beats once');
  });
  private syncHold: ReturnType<typeof setTimeout> | null = null;

  protected readonly pitchText = computed(() => formatPitch(this.tempo.rate()));
  /** On a record not locked by sync: the pitch to set on the turntable to match the other
   * deck. */
  protected readonly pitchTarget = computed(() => {
    const target = this.tempo.target();
    return target === null ? null : formatPitch(target);
  });

  constructor() {
    inject(DestroyRef).onDestroy(() => this.syncCancel());
  }

  cueDown() {
    this.engine.press(this.deck(), 'cue');
  }

  cueUp() {
    this.engine.press(this.deck(), 'cue', false);
  }

  syncDown() {
    this.syncCancel();
    this.syncHold = setTimeout(() => {
      this.syncHold = null;
      this.matchOnce();
    }, SYNC_HOLD_MS);
  }

  /** Released before the hold: a tap. */
  syncUp() {
    if (this.syncHold === null) return;
    this.syncCancel();
    this.engine.toggleSync(this.deck());
  }

  syncCancel() {
    if (this.syncHold !== null) clearTimeout(this.syncHold);
    this.syncHold = null;
  }

  /** The leader's tempo and beats, once: the deck stays free. Internal play takes the
   * tempo on its fader, so that the fader says what it plays. */
  private matchOnce() {
    const own = this.status();
    const reference = this.tempo.reference();
    const change = own && reference ? pitchToMatch(own, reference) : null;
    if (!this.vinyl() && change !== null) {
      this.engine.setPitch(this.deck(), this.tempo.rate() * (1 + change));
    }
    this.engine.syncOnce(this.deck());
  }

  resetPitch() {
    if (!this.tempo.held()) this.engine.setPitch(this.deck(), 1);
  }
}
