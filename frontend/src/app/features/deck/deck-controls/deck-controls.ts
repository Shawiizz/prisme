import { Component, computed, inject, input } from '@angular/core';
import { deckTempo } from '../../../core/deck-tempo';
import { EngineService } from '../../../core/services/engine.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { pitchToMatch } from '../../../core/sync';
import { formatPitch } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { StemPart, partIn, partLevel, partPlaying } from '../../../core/stem-parts';
import { Knob } from '../../../shared/knob/knob';
import { readouts } from '../../../shared/mixer-scales';
import { StemsService } from '../../../core/services/stems.service';

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
  imports: [Knob, TPipe],
  host: { '[class.plays]': '!vinyl()' },
})
export class DeckControls {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly stems = inject(StemsService);
  private readonly preferences = inject(PreferencesService);
  private readonly tempo = deckTempo(this.deck);
  protected readonly status = this.tempo.status;
  protected readonly settings = this.tempo.settings;
  protected readonly vinyl = this.tempo.vinyl;
  protected readonly syncState = this.tempo.syncState;
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly separation = computed(() => this.track()?.separation ?? 'none');
  /** Vocals and instrumental, or the four stems of a fully separated track: a knob each,
   * its name muting it at its level. */
  protected readonly stemKnobs = computed(() => {
    const mix = this.track()?.stemMix;
    const waiting = this.engine.status()?.decks[this.deck()]?.stemsWaiting ?? 0;
    const knob = (part: StemPart, label: string, name: string) => ({
      part,
      label,
      title: t('{part}: turn for the level, click the name to mute', { part: name }),
      level: mix ? partLevel(mix, part) : 1,
      on: !mix || partPlaying(mix, part),
      waiting: partIn(waiting, part),
    });
    return this.separation() === 'full'
      ? [
          knob('vocals', 'VOX', t('Vocals')),
          knob('drums', 'DRUM', t('Drums')),
          knob('bass', 'BASS', t('Bass')),
          knob('other', 'MELO', t('Melody (the other instruments)')),
        ]
      : [knob('vocals', 'VOX', t('Vocals')), knob('instrumental', 'INST', t('Instrumental'))];
  });
  /** How far this track's separation is, while it is being separated. */
  protected readonly separating = computed(() => {
    const progress = this.stems.progress();
    const path = this.track()?.path;
    return path && progress.current === path ? progress.fraction : null;
  });
  protected readonly canSeparate = computed(
    () => !!this.track() && this.stems.canAdd('none') && this.separating() === null,
  );
  protected readonly percent = readouts.percent;

  separate() {
    const path = this.track()?.path;
    if (path) this.stems.separate([path]);
  }
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

  togglePlay() {
    this.engine.togglePlay(this.deck());
  }

  cueDown() {
    this.engine.cueDown(this.deck());
  }

  cueUp() {
    this.engine.cueUp(this.deck());
  }

  syncDown(event: PointerEvent) {
    if (event.button !== 0) return;
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

  /** From the keyboard (a click with no pointer behind it): a tap. */
  syncKey(event: MouseEvent) {
    if (event.detail === 0) this.engine.toggleSync(this.deck());
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
