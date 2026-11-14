# Controller profiles

A profile maps a controller model as soon as it is turned on in Settings → MIDI
controllers, before anything is learnt on it. Learning a control on it takes the profile's
place for that control and that action; removing an action removes it from the profile's
controls too. Profiles are compiled into the app (`src/controller_profiles.rs`).

## Format

One JSON file per model:

| Field | |
| --- | --- |
| `name` | The model, as the settings show it. |
| `ports` | The controller is the MIDI port whose name holds one of these, case aside. |
| `credits` | Where the mapping comes from, when it is not Prisme's own. |
| `init` | MIDI messages sent when the controller is opened. |
| `controls` | A control and the action it drives, in the shape of a learnt mapping (`controllers.json`): `control` (`kind` `note`, `controlChange` or `pitchBend`, `channel` 0-15, `number`), `target` (an action id of `frontend/src/app/core/control/controls.ts`), and optionally `encoding` for encoders (`relative`, `relativeOffset`, `relativeSigned`), `fine` (the controller sending the low half of a 14-bit control) and `shift` (in the second layer). |
| `lights` | Lights other than the controls' own, or with values other than 127 and 0: `target`, `control`, `on`, `off`. A mapped control lights itself with its own note or controller. |

`frontend/src/app/core/control/profiles.spec.ts` checks that every target exists and that
only encoders read steps.

## From a Mixxx mapping

`tools/controllers/mixxx_profile.py <mapping.midi.xml> [<profile.json>]` converts a Mixxx
mapping: its plain controls, the controls its script names after what they do
(`playButton`, `eqKnob[1]`…), and the lights of its outputs. What only the script knows (a
second use under shift, lights it drives itself, messages it sends at start) is listed as
left out, to be completed by hand. Its tests: `python -m unittest discover tools/controllers`.

## Pioneer DDJ-SB2

`pioneer-ddj-sb2.json` comes from the Mixxx mapping by Be and Michael Stahl (DG3NEC),
GPL-2.0-or-later. The converter gave 259 of its 432 controls. Added by hand:

- `init`: the request for the positions of the knobs and faders (`9B 09 7F`).
- The jog wheels under shift (controllers 0x1F and 0x26), which still turn the deck.
- Shift + KEY LOCK (note 0x60): the tempo back to the track's.
- The sampler pads (0x30-0x33 on each deck's pad channel): that deck's sounds 1 to 4.
- The light of the headphones key under shift (note 0x68).

Left out, Prisme having no action for them: the effect units, the EQ kills and mute, the
loop rolls, the vinyl key, the master headphones mix, the level meters, the Auto DJ keys and
the browse key's click. Decks 3 and 4 answer in four-deck mode.
