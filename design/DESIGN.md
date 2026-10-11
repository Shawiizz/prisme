# Prisme design system

**White light for the interface, the spectrum for the music.** A prism splits white light into
its colours; Prisme splits music into its frequencies and its harmony. The interface itself is
colourless glass, and colour appears only where it carries information.

The DJ looks at the screen between two gestures on the records, in the dark, often a metre
away. Everything here serves that glance.

## Files

| File | Role |
|---|---|
| `frontend/src/styles/tokens.css` | Every colour, size and duration. **The source of truth.** |
| `frontend/src/styles/components.css` | Ready-made classes `.pr-*`: buttons, choices, key chips, readouts, pads, fields, dialogs, tables. |
| `frontend/src/styles/fonts.css` | Instrument Sans and Martian Mono, shipped in `styles/fonts` (OFL): the app works offline. |
| `design/reference/main-screen.html` | The main screen at 1280 × 800. **The exact visual model.** |
| `design/reference/design-system.html` | Every token and component on one board. |
| `design/logo.svg` | The prism: white light in, three bands out. |

The reference pages load the app's own stylesheets: open them in a browser from the repository,
they always show the current system.

## Using it

The build loads the three stylesheets before `styles.scss` (`angular.json`), so every component
can use the classes and the tokens.

- Components use the `.pr-*` classes for what they cover, and the `--pr-*` tokens for anything
  else. **No colour or size is written in a component**, not even in a canvas painter: read the
  token (`getComputedStyle`).
- The older tokens of `styles.scss` (`--bg`, `--deck-1`…) stay while the screens are redone one
  by one, then disappear. New code never uses them.

## Colours

Three families that never mix, plus three states.

| Role | Token | Value |
|---|---|---|
| Background, behind the waveforms | `--pr-void` | `#0A0B0D` |
| Panels, bands, sidebar | `--pr-base` | `#111316` |
| Set pads, fields | `--pr-raise` | `#181B1F` |
| Hover, selected row | `--pr-raise-2` | `#20242A` |
| Lines between rows / separations / outlines | `--pr-line-soft` / `--pr-line` / `--pr-line-2` | `#16191D` / `#23272D` / `#333840` |
| Text / secondary / labels | `--pr-text` / `--pr-text-2` / `--pr-text-3` | `#E7E8EA` / `#A1A6AE` / `#6B717A` |
| **Light**: on, main action, playhead | `--pr-light` | `#F3F1EB` |
| Glyphs on the light | `--pr-on-light` | `#0B0C0E` |
| OK: record signal, audio running | `--pr-ok` | `oklch(0.80 0.15 155)` |
| Warning: weak signal, last 30 s | `--pr-warn` | `oklch(0.84 0.15 80)` |
| Alert: lost signal, dropout, recording, last 10 s | `--pr-alert` | `oklch(0.66 0.21 27)` |

**Light is the only accent.** A toggle that is on, the selected choice, the main action of a
zone and the playhead are in the light, with dark glyphs on it. Nothing else is bright.

### The spectrum

Used for the music only.

- **Frequencies** in the waveform: low `--pr-f-low` (red), mid `--pr-f-mid` (green), high
  `--pr-f-high` (violet). The order of a real prism: the longest wavelength of light for the
  lowest frequencies of sound.
- **Keys**: one hue per Camelot number, in the order of the wheel (`--pr-key-hue-1` to `-12`),
  so compatible keys have neighbouring colours. `.pr-key` makes minor keys deep and major keys
  pale. In the library, keys compatible with the reference deck are filled, the others
  outlined.
- **Tags**: the eight colours of hot cues and tracks, in the usual DJ order (`--pr-tag-red` to
  `--pr-tag-pink`). Colours imported from Serato or rekordbox are shown as they are.

Decks have **no colour**: A and B are told apart by their letter and their place.

## Type

| Use | Face | Size / weight |
|---|---|---|
| Screen and dialog titles | Instrument Sans, narrow (`--pr-narrow`) | 30 (dialogs 22) / 600 |
| Track titles | Instrument Sans | 15 / 600 |
| Interface | Instrument Sans | 13 / 400 |
| Labels (REMAINING, BPM, KEY) | Instrument Sans, capitals, 0.09 em | 10.5 / 600 |
| Remaining time | Martian Mono (`--pr-mono-width`) | 27 / 500 |
| BPM, key | Martian Mono | 20 / 500 |
| Table numbers | Martian Mono | 12 / 400 |

**Every number that changes is in Martian Mono** (`.pr-num`): with fixed widths, a running time
never shakes its line. Two weights only, 400 and 600; 500 is for the mono numbers. Never under
10.5 px.

## Buttons

Three heights, one per use. Two buttons side by side always share one.

| Size | Height | Use |
|---|---|---|
| `.pr-btn--lg` | 40 | What is touched while mixing: pads, loops, beat jumps, loading |
| (default) | 32 | Everything else |
| `.pr-btn--sm` | 24 | Dialogs and toolbars only |

| Variant | Use |
|---|---|
| `.pr-btn--light` | The main action of a zone: one per zone |
| (default, outlined) | Every other action |
| `.pr-btn--ghost` | Title bar, cancel, minor actions |
| `.pr-btn--danger` | Deleting |
| `.pr-btn--rec` | Recording; red with `.is-on` while it records |
| `.pr-btn--icon` | A square symbol button: always an `aria-label` |

A toggle is a button with `.is-on` when on. Radius 3 px, no shadow, no glow.

## Controls

| Class | Use |
|---|---|
| `.pr-seg` (`--lg`) | One choice among a few: INT / REL / ABS, A / B. Buttons `role="radio"` with `aria-checked`. |
| `.pr-switch` | A setting on or off: `<button role="switch" aria-checked>` with a `<span>`. |
| `.pr-check` | A box in a list of options. |
| `.pr-input`, `.pr-select`, `.pr-field` | Fields: 32 high, label above, help below. Native `<select>`. |
| `.pr-range` | Sliders in the settings. |
| `.pr-pad` (`--set`, `.is-hot`) | Hot cues and samples: empty dashed, set with its tag (`--tag`), in the light while held. |
| `.pr-dialog` in `.pr-backdrop` | Dialogs: head, scrolling body, foot with the actions. The only shadow of the app. |
| `.pr-table` | Rows on a grid (`--pr-cols`): `.is-selected`, `.is-played` (dimmed, never hidden). |
| `.pr-tooltip` | Tooltips: they name the keyboard shortcut of the control. |

## Readouts and indicators

- **Per deck, three readouts read from afar**: remaining time (27), BPM and key (20). Under each,
  a small line: elapsed and length, original BPM and pitch, key name.
- **Remaining time** (`.pr-readout`): `--warn` for the last 30 seconds, `--alert` and blinking
  at 1 Hz for the last 10. The only blinking of the app.
- **Record signal** (`.pr-sig`): full green when fine, hollow amber when weak, hollow red when
  lost. At the end of the record the deck goes to INT by itself, in the light, until it is handed
  back.
- **Audio health** (`.pr-chip`): driver, latency, dropouts; `--warn` from the first dropout.
- **Level meter** (`.pr-meter`): 24 segments, grey to −6 dB, amber to 0, red beyond.
- **Pitch → A** in the library: the pitch to set on the turntable to match a track to the
  reference deck. Dimmed beyond ±8 %, the travel of a turntable pitch fader.

## Waveform

- **Three layers**, low behind and high in front, each its own envelope: the kick, the bass and
  the hats read separately. Heights: low 94 %, mid 66 %, high 40 % of the lane.
- **What has played** (left of the playhead) at 42 % opacity.
- **The playhead** is the only white line of the screen, 2 px, dispersed on its edges: 1 px of
  `--pr-f-low` on the left, 1 px of `--pr-f-high` on the right.
- **Beat grid**: short ticks at the top and bottom only, longer and brighter on each bar.
- **Hot cues**: a line and a numbered flag in the cue's tag colour, as on its pad; the main
  cue in the light, flagged C. **Loop**: a band of `--pr-light-veil` between two lines in the
  light; a stored loop as two `--pr-text-3` lines.
- **Overview**: the whole track in grey, played part dimmed, cues as ticks, position in the
  light.
- **Phase strip** between the two decks: the beats of A at the top and of B at the bottom,
  around the playheads, with the offset in milliseconds. Matched, they meet.

## Layout

- Title bar 36: logo, audio health, then Record, Turntables, MIDI, Shortcuts, Settings.
- Two deck rows: the deck panel (380) on the left, the waveform on the right; the phase strip
  between them.
- A performance band of pads, loops and beat jumps, one half per deck.
- The library fills the rest: sidebar (210) and the track table, with search and the reference
  deck.
- **External mixer first**: without the software mixer the screen goes to the waveforms and the
  library. When enabled, the internal mixer is inserted in the middle of the performance band.
- Spacing on a base of 4: 4, 8, 12, 16, 24, 32, 48.

## Ergonomic rules

1. **A on the left or at the top, B on the right or at the bottom**, everywhere, dialogs of the
   turntables and of MIDI included.
2. **External mixer first**: the screen belongs to the waveforms and the library.
3. **Remaining time at least 27 px**, BPM and key at least 20: the numbers looked for between
   two gestures.
4. **Every live number in mono**: time, BPM, pitch, latency. Nothing shakes while playing.
5. **No interface animation during playback.** Openings in 120 ms at most (`--pr-fast`). Only the
   last 10 seconds of a track blink.
6. **An alert changes colour and shape** (hollow signal, amber then red time, dropout chip):
   seen in peripheral vision and without telling colours apart.
7. **The keyboard does everything.** Every control has a shortcut, named in its tooltip. The
   search answers Ctrl F and understands `124-128` and `5A`.
8. **What has been played fades**: tracks played tonight are dimmed in the library, never
   hidden.

## Never

Glows and neon. Gradients. Shadows, except under dialogs. Pure white, pure black. Colour as
decoration. A colour per deck. Emoji. Text under 10.5 px. Weight 700.

## Words

- English is the key of every text, French is in `core/i18n/fr.ts`. In French the interface
  says *vous* and uses the imperative (« Appliquez », « Glissez-y des morceaux »).
- French numbers use the decimal comma (126,00 · +0,8 %); the remaining time takes a true
  minus sign (−2:15,7).
- Name things as a DJ does: deck, cue, crate (« bac »), pitch, key lock. No engine words in the
  interface (callback, buffer underrun): a dropout is a dropout.

## Reference screens

| Model | Matches |
|---|---|
| `reference/main-screen.html` | `app.html`, `shell/titlebar`, `features/deck`, `features/library` |
| `reference/design-system.html` | Every class of `components.css` |

The pages are static: the waveforms are sketched from synthetic music (`reference/sketch.js`),
and the tracks are real ones of the author's collection.
