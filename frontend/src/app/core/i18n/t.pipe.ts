import { Pipe, PipeTransform } from '@angular/core';
import { Params, t } from './i18n';

/** `'Text' | t` or `'Hot cue {n}' | t: { n: 3 }`, in the UI's language: impure, so that
 * the text follows a change of language. */
@Pipe({ name: 't', pure: false })
export class TPipe implements PipeTransform {
  transform(text: string, params?: Params): string {
    return t(text, params);
  }
}
