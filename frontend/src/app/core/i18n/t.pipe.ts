import { Pipe, PipeTransform } from '@angular/core';
import { Params, t } from './i18n';

/** `'Text' | t` or `'Hot cue {n}' | t: { n: 3 }`, in the UI's language. */
@Pipe({ name: 't' })
export class TPipe implements PipeTransform {
  transform(text: string, params?: Params): string {
    return t(text, params);
  }
}
