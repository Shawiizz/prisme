import { TestBed } from '@angular/core/testing';
import { ConfirmService, Question } from './confirm';

describe('ConfirmService', () => {
  const question = (action?: string): Question => ({
    title: 'Deck A is playing',
    detail: 'The track playing stops, and the new one takes its place.',
    confirm: 'Load',
    cancel: 'Cancel',
    action,
  });

  it('takes the action asked again while its question is open for yes', async () => {
    const confirm = TestBed.inject(ConfirmService);
    const asked = confirm.ask(question('library.load1'));
    expect(await confirm.ask(question('library.load1'))).toBe(false);
    expect(await asked).toBe(true);
    expect(confirm.asking()).toBeNull();
  });

  it('answers no a question another one takes the place of', async () => {
    const confirm = TestBed.inject(ConfirmService);
    const asked = confirm.ask(question('library.load1'));
    const other = confirm.ask(question('library.load2'));
    expect(await asked).toBe(false);
    expect(confirm.asking()?.action).toBe('library.load2');
    const unnamed = confirm.ask(question());
    expect(await other).toBe(false);
    void confirm.ask(question());
    expect(await unnamed).toBe(false);
  });
});
