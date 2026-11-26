import { Injectable, Injector, effect, inject, untracked } from '@angular/core';
import { EngineService } from './engine.service';
import { NavigationService } from './navigation.service';
import { NoticeService } from '../../shared/notice/notice';
import { t } from '../i18n/i18n';

/**
 * Tells the DJ, where it shows, what happened to the sound card out of sight: the card chosen
 * held by another program, and taken back once it lets go; a controller with no mixer
 * of its own whose card Prisme now mixes for.
 */
@Injectable({ providedIn: 'root' })
export class SoundCardNoticesService {
  private readonly engine = inject(EngineService);
  private readonly notices = inject(NoticeService);
  private readonly navigation = inject(NavigationService);
  private readonly injector = inject(Injector);

  start() {
    let held: string | null = null;
    effect(
      () => {
        const { busy, info, settings } = this.engine.told().audio;
        const playing = info?.device ?? '';
        untracked(() => {
          if (busy && busy !== held) this.tellHeld(busy, playing, settings.outputDevice !== null);
          else if (!busy && held && settings.outputDevice !== null) this.tellFree(held);
        });
        held = busy;
      },
      { injector: this.injector },
    );
    // a controller plugged in, or its card chosen, while Prisme runs: told once; at the start
    // the faders simply work
    let mixedFor: string | null | undefined;
    effect(
      () => {
        const { allInOne, allInOneMixes, mode } = this.engine.told().routing;
        const now = allInOne && allInOneMixes && mode === 'external' ? allInOne : null;
        untracked(() => {
          if (now && mixedFor !== undefined && now !== mixedFor) this.tellMixedFor(now);
        });
        mixedFor = now;
      },
      { injector: this.injector },
    );
  }

  private tellHeld(card: string, playing: string, chosen: boolean) {
    this.notices.show({
      text: t('{model} is used by another program', { model: card }),
      detail: chosen
        ? t(
            'Another DJ program may hold it: close it, Prisme takes the card back by itself. Meanwhile the sound plays on {device}.',
            { device: playing },
          )
        : t('An audio mixer such as VoiceMeeter may hold it: the sound plays on {device}.', {
            device: playing,
          }),
      warning: true,
      stays: true,
    });
  }

  private tellFree(card: string) {
    this.notices.show({
      text: t('{card} is free', { card }),
      detail: t('The sound plays on it again.'),
      prominent: true,
    });
  }

  private tellMixedFor(controller: string) {
    this.notices.show({
      text: t('Prisme mixes for {controller}', { controller }),
      detail: t(
        'It has no mixer of its own: its faders, crossfader and trims go through Prisme, the headphones too.',
      ),
      prominent: true,
      actions: [{ label: t('Mixing settings'), run: () => this.navigation.openSettings('mixing') }],
    });
  }
}
