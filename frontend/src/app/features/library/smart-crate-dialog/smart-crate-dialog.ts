import { Component, inject, input, linkedSignal, output } from '@angular/core';
import { Crate, Rule, SmartRules, TextField } from '../../../core/models/library';
import { LibraryService } from '../../../core/services/library.service';

type Choice = TextField | 'bpm' | 'year' | 'key';

const CHOICES: { id: Choice; label: string }[] = [
  { id: 'genre', label: 'Genre' },
  { id: 'artist', label: 'Artist' },
  { id: 'title', label: 'Title' },
  { id: 'album', label: 'Album' },
  { id: 'comment', label: 'Comment' },
  { id: 'fileName', label: 'File name' },
  { id: 'bpm', label: 'BPM' },
  { id: 'year', label: 'Year' },
  { id: 'key', label: 'Key' },
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
    const camelot = text
      .split(/[\s,]+/)
      .filter(Boolean)
      .map((code) => code.toUpperCase());
    this.update(index, (r) => (r.kind === 'key' ? { ...r, camelot } : r));
  }

  add() {
    this.rules.update((rules) => [...rules, newRule('genre')]);
  }

  remove(index: number) {
    this.rules.update((rules) => rules.filter((_, i) => i !== index));
  }

  async save() {
    const name = this.name().trim();
    if (!name || !this.rules().length) return;
    const rules: SmartRules = { matchAll: this.matchAll(), rules: this.rules() };
    const crate = this.crate();
    if (crate) await this.library.updateSmartCrate(crate.id, name, rules);
    else await this.library.createSmartCrate(name, rules);
    this.closed.emit();
  }

  private update(index: number, change: (rule: Rule) => Rule) {
    this.rules.update((rules) => rules.map((r, i) => (i === index ? change(r) : r)));
  }
}
