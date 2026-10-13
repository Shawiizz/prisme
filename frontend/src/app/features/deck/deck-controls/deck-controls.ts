import { Component, computed, inject, input } from '@angular/core';
import { MixerService } from '../../../core/services/mixer.service';
import { ChannelStrip } from '../../mixer/channel-strip/channel-strip';
import { EngineService } from '../../../core/services/engine.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { pitchToMatch } from '../../../core/sync';
import { formatPitch } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';

type SyncState = 'off' | 'matched' | 'locked';

/** Holding SYNC this long (ms) matches the leader once instead of locking to it. */
const SYNC_HOLD_MS = 450;

/** Transport, sync and tempo fader of a deck, with its channel when the software mixer runs. */
@Component({
  selector: 'app-deck-controls',
  templateUrl: './deck-controls.html',
  styleUrl: './deck-controls.scss',
  imports: [ChannelStrip, Fader, TPipe],
})
export class DeckControls {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  private readonly preferences = inject(PreferencesService);
  protected readonly range = computed(() => this.preferences.dj().pitchRange);
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly vinyl = computed(() => this.settings().vinylEnabled);
  private readonly absolute = computed(
    () => this.vinyl() && this.settings().vinylMode === 'absolute',
  );

  /** The deck this one is compared with: the leader, else another deck with a tempo. */
  private readonly reference = computed(() => {
    const others = (this.engine.status()?.decks ?? []).filter((_, i) => i !== this.deck());
    return others.find((d) => d.sync.leader) ?? others.find((d) => d.bpm !== null) ?? null;
  });

  protected readonly syncState = computed<SyncState>(() => {
    const sync = this.status()?.sync;
    if (sync?.locked) return 'locked';
    return sync && sync.rateOffset !== 1 ? 'matched' : 'off';
  });
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
  protected readonly tempoFaderLocked = computed(
    () => this.vinyl() || this.syncState() === 'locked',
  );

  /** Playback rate as the engine reports it, the fader position when it cannot. */
  private readonly rate = computed(
    () => this.engine.tempoRate(this.deck()) ?? this.settings().pitch,
  );
  protected readonly pitchText = computed(() => formatPitch(this.rate()));
  /** A fader the deck does not obey shows the rate it plays at. */
  protected readonly faderValue = computed(
    () => (this.tempoFaderLocked() ? this.rate() : this.settings().pitch) - 1,
  );

  /** On a record not locked by sync: the pitch change that would match the other deck. */
  protected readonly pitchHint = computed(() => {
    const own = this.status();
    const reference = this.reference();
    if (!this.vinyl() || this.syncState() === 'locked' || !own || !reference) return null;
    const change = pitchToMatch(own, reference);
    return change === null ? null : formatPitch(1 + change);
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

  bend(direction: -1 | 0 | 1) {
    this.engine.pitchBend(this.deck(), direction);
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
    const reference = this.reference();
    const change = own && reference ? pitchToMatch(own, reference) : null;
    if (!this.vinyl() && change !== null) {
      this.engine.setPitch(this.deck(), this.rate() * (1 + change));
    }
    this.engine.syncOnce(this.deck());
  }

  setPitch(offset: number) {
    this.engine.setPitch(this.deck(), 1 + offset);
  }

  resetPitch() {
    this.engine.setPitch(this.deck(), 1);
  }
}
