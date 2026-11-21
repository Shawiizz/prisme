import { Component, DestroyRef, computed, effect, inject, signal, untracked } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { deckStems } from '../../core/deck-stems';
import { deckTempo } from '../../core/deck-tempo';
import { EFFECT_NAMES, PARAM2 } from '../../core/effects';
import { BANDS } from '../../core/models/engine';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { NavigationService } from '../../core/services/navigation.service';
import { t } from '../../core/i18n/i18n';
import { formatBeats } from '../../shared/beats';
import { KeyChip } from '../../shared/key-chip/key-chip';
import { percent, readout } from '../../shared/readout';
import { deckLetter } from '../../shared/time';
import { MixerSetting, PhoneSight, Setting, settingOf } from './phone-sight';
import type { KnobState, MusicalKey } from '../../core/models/backend';

/** How long a setting moved stays shown after its last move (ms). */
export const SHOWN_MS = 1500;

/** A setting as it is shown: its deck, what it belongs to, its name (lit while its switch is
 * on), where it stands from 0 to 1 (from the middle, `centred`) and what it reads. */
interface Shown {
  letter: string | null;
  group: string;
  name: string;
  lit: boolean;
  at: number | null;
  centred: boolean;
  /** Its level kept, not heard: muted, killed. */
  off: boolean;
  reads: string;
  key: MusicalKey | null;
}

const BAND_NAMES = { low: 'LOW', mid: 'MID', high: 'HI' } as const;
/** The mixer's settings beside the channels as its sheet names them: the row, then the knob. */
const MIXER_NAMES: Record<
  Exclude<MixerSetting, 'crossfader' | 'mic' | 'aux' | 'samplers'>,
  { group: () => string; name: string }
> = {
  main: { group: () => t('Master'), name: 'LEVEL' },
  balance: { group: () => t('Master'), name: 'BAL' },
  booth: { group: () => t('Master'), name: 'BOOTH' },
  cueMix: { group: () => t('Headphones'), name: 'CUE/MST' },
  headphones: { group: () => t('Headphones'), name: 'PHONES' },
};
const LIVE_NAMES = { mic: () => t('Microphone'), aux: () => t('Aux line') };

function shown(
  letter: string | null,
  group: string,
  name: string,
  more: Partial<Shown> = {},
): Shown {
  const still = { lit: false, at: null, centred: false, off: false, reads: '', key: null };
  return { letter, group, name, ...still, ...more };
}

/** A knob as the backend tells it. */
function knob(
  letter: string | null,
  group: string,
  name: string,
  state: KnobState,
  centred = false,
): Shown {
  return shown(letter, group, name, { at: state.at, centred, reads: readout(state.reads) });
}

/** A level from 0 to 1, read as a share. */
function level(letter: string | null, group: string, name: string, at: number): Shown {
  return shown(letter, group, name, { at, reads: percent(at) });
}

/**
 * The setting a controller or a key moved last, shown over the bar of the phone while it moves
 * and a moment after, as a phone shows its volume: unless the screen shows it already (see
 * `PhoneSight`). Its value and what it reads are the backend's, as the screen's knobs show them.
 * It takes no touch and moves nothing.
 */
@Component({
  selector: 'app-phone-moved',
  template: `
    @if (shown(); as s) {
      <div class="moved" role="status" animate.enter="pr-fade-in" animate.leave="pr-fade-out">
        @if (s.letter) {
          <span class="pr-tile">{{ s.letter }}</span>
        }
        @if (s.group) {
          <span class="group">{{ s.group }}</span>
        }
        @if (s.name) {
          <b class="name" [class.lit]="s.lit">{{ s.name }}</b>
        }
        @if (s.at !== null) {
          <span class="bar" [class.off]="s.off">
            <i
              [style.left.%]="(s.centred ? Math.min(s.at, 0.5) : 0) * 100"
              [style.right.%]="(1 - (s.centred ? Math.max(s.at, 0.5) : s.at)) * 100"
            ></i>
            @if (s.centred) {
              <b class="middle"></b>
            }
          </span>
        } @else {
          <span class="space"></span>
        }
        @if (s.key) {
          <app-key-chip fill [key]="s.key" />
        }
        @if (s.reads) {
          <span class="reads pr-num">{{ s.reads }}</span>
        }
      </div>
    }
  `,
  styles: `
    /* over the bar, which it hides alone */
    :host {
      position: absolute;
      inset: 2px 8px;
      z-index: var(--pr-z-tip);
      pointer-events: none;
    }
    .moved {
      display: flex;
      align-items: center;
      gap: 8px;
      height: 100%;
      padding: 0 10px 0 4px;
      border: 1px solid var(--pr-line-2);
      border-radius: var(--pr-radius);
      background: var(--pr-base);
      box-shadow: 0 8px 24px rgb(0 0 0 / 0.6);
      white-space: nowrap;
    }
    .pr-tile {
      width: 24px;
      height: 24px;
      font-size: 14px;
    }
    .group {
      font-size: var(--pr-size-small);
      color: var(--pr-text-2);
    }
    .name {
      padding: 2px 5px;
      border-radius: var(--pr-radius-sm);
      font: 700 13px / 1 var(--pr-font);
      letter-spacing: 0.06em;
      color: var(--pr-text);
      &.lit {
        background: var(--pr-light);
        color: var(--pr-on-light);
      }
    }
    .bar,
    .space {
      flex: 1;
      min-width: 24px;
    }
    .bar {
      position: relative;
      height: 6px;
      border-radius: 3px;
      background: var(--pr-line-2);
      i {
        position: absolute;
        top: 0;
        bottom: 0;
        border-radius: 3px;
        background: var(--pr-light);
      }
      &.off i {
        background: var(--pr-text-3);
      }
    }
    .middle {
      position: absolute;
      left: 50%;
      top: -3px;
      bottom: -3px;
      width: 2px;
      margin-left: -1px;
      background: var(--pr-text-2);
    }
    .reads {
      min-width: 6ch;
      text-align: right;
      font-size: 15px;
      font-weight: 600;
      color: var(--pr-text);
    }
  `,
  imports: [KeyChip],
})
export class PhoneMoved {
  private readonly navigation = inject(NavigationService);
  private readonly sight = inject(PhoneSight);
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);

  /** The setting moved last, while it is shown. */
  private readonly moved = signal<Setting | null>(null);
  private readonly deck = computed(() => this.moved()?.deck ?? 0);
  private readonly stems = deckStems(this.deck);
  private readonly tempo = deckTempo(this.deck);
  private readonly readouts = deckReadouts(this.deck);

  protected readonly shown = computed(() => {
    const setting = this.moved();
    return setting && !this.sight.showing(setting) ? this.describe(setting) : null;
  });
  protected readonly Math = Math;

  constructor() {
    let timer: ReturnType<typeof setTimeout> | undefined;
    effect(() => {
      const told = this.navigation.moved();
      const setting = told && settingOf(told.id);
      if (!setting) return;
      untracked(() => this.moved.set(setting));
      clearTimeout(timer);
      timer = setTimeout(() => this.moved.set(null), SHOWN_MS);
    });
    inject(DestroyRef).onDestroy(() => clearTimeout(timer));
  }

  private describe(setting: Setting): Shown | null {
    if (setting.deck === null) return this.mixerSetting(setting.of);
    const deck = setting.deck;
    const settings = this.engine.decks()[deck];
    if (!settings) return null;
    const letter = deckLetter(deck);
    const strip = settings.knobs;
    switch (setting.of) {
      case 'eq': {
        const band = BANDS.indexOf(setting.band);
        const killed = settings.strip.kills[band];
        const eq = knob(letter, 'EQ', BAND_NAMES[setting.band], strip.eq[band], true);
        return { ...eq, lit: killed, off: killed };
      }
      case 'trim':
        return knob(letter, '', 'TRIM', strip.trim, true);
      case 'filter':
        return knob(letter, '', 'FILTER', strip.filter, true);
      case 'volume':
        return shown(letter, '', 'VOL', { at: settings.volume });
      case 'tempo':
        return shown(letter, '', 'PITCH', {
          at: this.tempo.position(),
          centred: true,
          reads: this.tempo.pitch(),
        });
      case 'key': {
        const shift = this.engine.tracks()[deck]?.keyShift ?? 0;
        const reads = shift > 0 ? `+${shift}` : shift < 0 ? `−${-shift}` : '0';
        return shown(letter, '', 'KEY', { key: this.readouts.key(), reads });
      }
      case 'keyLock':
        return shown(letter, '', 'KEY LOCK', { lit: settings.keyLock });
      case 'cue':
        return shown(letter, '', 'CUE', { lit: settings.cue });
      case 'mute':
        return shown(letter, '', 'MUTE', { lit: settings.mute });
      case 'fxChain':
        return shown(letter, '', 'FX', { lit: settings.fx.enabled });
      case 'samplerGain':
        return level(letter, 'SMPL', 'VOL', settings.samplerGain);
      case 'stem': {
        const part = this.stems.knobs().find((k) => k.part === setting.part);
        if (!part) return null;
        return { ...level(letter, 'Stems', part.label, part.level), lit: !part.on, off: !part.on };
      }
      case 'fx': {
        const slot = settings.fx.slots[setting.slot ?? 0];
        const effect = EFFECT_NAMES[slot.kind]();
        const group = setting.slot === null ? 'FX' : `FX ${setting.slot + 1}`;
        const of = `${group} · ${effect}`;
        switch (setting.knob) {
          case 'mix':
            return level(letter, of, 'MIX', slot.mix);
          case 'amount':
            return level(letter, of, 'AMOUNT', slot.amount);
          case 'param2':
            return level(letter, of, PARAM2[slot.kind].label, slot.param2);
          case 'beats': {
            const beats = formatBeats(slot.beats);
            const reads =
              Math.abs(slot.beats) > 1
                ? t('{beats} beats', { beats })
                : t('{beats} beat', { beats });
            return shown(letter, of, '', { reads });
          }
          case 'kind':
          case 'on':
            return shown(letter, group, effect, { lit: slot.enabled });
        }
      }
    }
  }

  private mixerSetting(of: MixerSetting): Shown {
    const knobs = this.mixer.knobs();
    switch (of) {
      case 'crossfader':
        return shown(null, t('Crossfader'), '', { at: knobs.crossfader.at, centred: true });
      case 'samplers': {
        const first = this.engine.decks()[this.engine.deckIndexes()[0] ?? 0];
        return level(null, 'SMPL', 'VOL', first?.samplerGain ?? 1);
      }
      case 'mic':
      case 'aux':
        return {
          ...knob(null, LIVE_NAMES[of](), 'LEVEL', knobs[of]),
          off: !this.mixer.live(of)().on,
        };
      default: {
        const { group, name } = MIXER_NAMES[of];
        return knob(null, group(), name, knobs[of], of === 'balance');
      }
    }
  }
}
