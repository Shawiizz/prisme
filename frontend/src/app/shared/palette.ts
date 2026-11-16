/** The tokens of the design system (tokens.css) a canvas paints with. */
export interface Palette {
  low: string;
  mid: string;
  high: string;
  light: string;
  onLight: string;
  veil: string;
  void: string;
  base: string;
  line: string;
  line2: string;
  text: string;
  text2: string;
  text3: string;
  /** The first beat of a bar on the beat grid. */
  bar: string;
  mono: string;
}

let read: Palette | null = null;

/** The palette, read from the page once, when first painted with. */
export function palette(): Palette {
  if (read) return read;
  const style = getComputedStyle(document.documentElement);
  const token = (name: string) => style.getPropertyValue(name).trim();
  read = {
    low: token('--pr-f-low'),
    mid: token('--pr-f-mid'),
    high: token('--pr-f-high'),
    light: token('--pr-light'),
    onLight: token('--pr-on-light'),
    veil: token('--pr-light-veil'),
    void: token('--pr-void'),
    base: token('--pr-base'),
    line: token('--pr-line'),
    line2: token('--pr-line-2'),
    text: token('--pr-text'),
    text2: token('--pr-text-2'),
    text3: token('--pr-text-3'),
    bar: token('--pr-alert'),
    mono: token('--pr-font-mono'),
  };
  return read;
}
