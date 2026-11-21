import { Component, inject, input, linkedSignal, output, signal } from '@angular/core';
import { LibraryService } from '../../../core/services/library.service';
import { errorText } from '../../../core/errors';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Dismiss, overlay } from '../../../shared/overlay';
import type { Crate, Rule, SmartRules, TextField } from '../../../core/models/backend';

type Choice = TextField | 'bpm' | 'year' | 'key';

const CHOICES: { id: Choice; label: Text }[] = [
  {
    id: 'genre',
    label: () => t('Genre'),
  },
  {
    id: 'artist',
    label: () => t('Artist'),
  },
  {
    id: 'title',
    label: () => t('Title'),
  },
  {
    id: 'album',
    label: () => t('Album'),
  },
  {
    id: 'comment',
    label: () => t('Comment'),
  },
  {
    id: 'fileName',
    label: () => t('File name'),
  },
  { id: 'bpm', label: () => 'BPM' },
  {
    id: 'year',
    label: () => t('Year'),
  },
  {
    id: 'key',
    label: () => t('Key'),
  },
];

function newRule(choice: Choice): Rule {
  switch (choice) {
    case 'bpm':
    case 'year':
      return { kind: choice, min: null, max: null };
    case 'key':
      return { kind: 'key', camelot: [] };
    default:
      return { kind: 'text', field: choice, contains: '' };
  }
}

/** Creates a smart crate, or changes the name and rules of one. */
@Component({
  selector: 'app-smart-crate-dialog',
  templateUrl: './smart-crate-dialog.html',
  styleUrl: './smart-crate-dialog.scss',
  imports: [TPipe, Dismiss],
})
export class SmartCrateDialog {
  /** The crate to change; `null` to create one. */
  readonly crate = input<Crate | null>(null);
  readonly closed = output();

  private readonly library = inject(LibraryService);
  protected readonly choices = CHOICES;
  protected readonly name = linkedSignal(() => this.crate()?.name ?? '');
  protected readonly matchAll = linkedSignal(() => this.crate()?.rules?.matchAll ?? true);
  protected readonly rules = linkedSignal<Rule[]>(
    () => this.crate()?.rules?.rules ?? [newRule('genre')],
  );
  /** What the backend could not take of the rules. */
  protected readonly error = signal<string | null>(null);

  constructor() {
    overlay(
      () => true,
      () => this.closed.emit(),
    );
  }

  choiceOf(rule: Rule): Choice {
    return rule.kind === 'text' ? rule.field : rule.kind;
  }

  setChoice(index: number, choice: Choice) {
    this.update(index, () => newRule(choice));
  }

  setContains(index: number, text: string) {
    this.update(index, (r) => (r.kind === 'text' ? { ...r, contains: text } : r));
  }

  setBound(index: number, bound: 'min' | 'max', text: string) {
    const value = text.trim() === '' ? null : Number(text.replace(',', '.'));
    const parsed = value !== null && Number.isFinite(value) ? value : null;
    this.update(index, (r) => {
      if (r.kind !== 'bpm' && r.kind !== 'year') return r;
      return bound === 'min' ? { ...r, min: parsed } : { ...r, max: parsed };
    });
  }

  setKeys(index: number, text: string) {
    const camelot = text.split(/[\s,]+/).filter(Boolean);
    this.update(index, (r) => (r.kind === 'key' ? { ...r, camelot } : r));
  }

  add() {
    this.rules.update((rules) => [...rules, newRule('genre')]);
  }

  remove(index: number) {
    this.rules.update((rules) => rules.filter((_, i) => i !== index));
  }

  /** Kept as typed: the backend says what it cannot take, the dialog stays open on it. */
  async save() {
    const rules: SmartRules = { matchAll: this.matchAll(), rules: this.rules() };
    try {
      await this.library.saveSmartCrate(this.crate()?.id ?? null, this.name(), rules);
      this.closed.emit();
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  private update(index: number, change: (rule: Rule) => Rule) {
    this.rules.update((rules) => rules.map((r, i) => (i === index ? change(r) : r)));
  }
}
