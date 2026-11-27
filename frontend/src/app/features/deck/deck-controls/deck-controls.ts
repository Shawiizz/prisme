import { Component, computed, inject, input } from '@angular/core';
import { deckTempo } from '../../../core/deck-tempo';
import { EngineService } from '../../../core/services/engine.service';
import { formatPitch } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { deckStems } from '../../../core/deck-stems';
import { Knob } from '../../../shared/knob/knob';
import { percent } from '../../../shared/readout';
import { Press } from '../../../shared/press';
import { AssignTarget, deckAction } from '../../../shared/assign/assign';

/**
 * What plays a deck, at the start of its rail: the transport (a record has its own), the
 * stems, sync, and the pitch with key lock.
 */
@Component({
  selector: 'app-deck-controls',
  templateUrl: './deck-controls.html',
  styleUrl: './deck-controls.scss',
  imports: [Knob, TPipe, Press, AssignTarget],
  host: { '[class.plays]': '!vinyl()' },
})
export class DeckControls {
  readonly deck = input.required<number>();
  /** The action `name` of this deck, for the assigning mode. */
  protected action(name: string): string {
    return deckAction(this.deck(), name);
  }

  protected readonly engine = inject(EngineService);
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
  protected readonly percent = percent;

  /** CUE lit at the cue point, blinking stopped elsewhere, as a CDJ's. */
  protected readonly cue = computed(() => this.status()?.lights.cue ?? 'off');
  private readonly syncKey = computed(() => this.status()?.sync.key ?? 'noGrid');
  protected readonly canSync = computed(
    () => this.syncKey() === 'lock' || this.syncKey() === 'once',
  );
  protected readonly syncTitle = computed(() => {
    switch (this.syncKey()) {
      case 'absolute':
        return t('Sync needs relative mode or internal play');
      case 'noGrid':
        return t('No beat grid');
      case 'once':
        return t('Match the tempo and beats once');
      case 'lock':
        return t('Tap: lock to the leader · hold: match its tempo and beats once');
    }
  });
  protected readonly pitchText = computed(() => formatPitch(this.tempo.rate()));
  /** On a record not locked by sync: the pitch to set on the turntable to match the other
   * deck. */
  protected readonly pitchTarget = this.tempo.toMatch;

  cueDown() {
    this.engine.press(this.deck(), 'cue');
  }

  cueUp() {
    this.engine.press(this.deck(), 'cue', false);
  }

  resetPitch() {
    if (!this.tempo.held()) this.engine.press(this.deck(), 'tempo.reset');
  }
}
