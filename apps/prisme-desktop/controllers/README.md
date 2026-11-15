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
| `ports` | The controller is the MIDI port whose name holds one of these, case aside; of several models, the longest name wins. |
| `credits` | Where the mapping comes from, when it is not Prisme's own. |
| `jogStepsPerTurn` | Steps in a turn of its jog wheels (720 when left out). |
| `init` | MIDI messages sent when the controller is opened (SysEx included). |
| `close` | MIDI messages sent when it is closed, after its lights went off. |
| `keepAlive` | `everyMs` and `message`: sent again and again while it is open, for the models that want it. |
| `modes` | Groups of controls that drive other actions in each mode, when the controls send the same in each (the pad pages of a side): `group`, `modes` (the first is where it begins), `select` (a key choosing each mode: `control`, `mode`, optionally `shift` and `light`). A mode key lights `on` while its mode is the one. |
| `controls` | A control and the action it drives, in the shape of a learnt mapping (`controllers.json`): `control` (`kind` `note`, `controlChange` or `pitchBend`, `channel` 0-15, `number`), `target`, and optionally `encoding` for encoders (`relative`, `relativeOffset`, `relativeSigned`), `fine` (the controller sending the low half of a 14-bit control), `shift` (in the second layer), `when` (`group`, `mode`: only in that mode), `invert` (a fader whose lowest value is at its top) and `light` (its values by state). |
| `lights` | Lights other than the controls' own: `target`, `control`, optionally `when`, and its values by state beside them. |
| `meters` | Levels shown on the controller: `source` (`deck1`…`deck4`, `main`), `control`, optionally `max` (127) and `curve` (the level raised to it, 1 straight). |

### Targets

An action id of `frontend/src/app/core/control/controls.ts` (`deck1.play`, `deck2.hotcue3`,
`mixer.crossfader`…), a jump or a loop of so many beats (`deck1.jump.-8`, `deck1.jump.16`,
`deck1.loop.0.25`), or a mode key (`mode.<group>.<mode>`, made from `modes`).

### Lights

A light shows the state of its action: `on` and `off` for most, `off`, `loaded` and `playing`
for the sampler's pads. Its values by state are numbers, or `{ "blink": [a, b], "ms": n }` for
two in turn. A state it does not name shows as `on`, but `off`; without values, 127 and 0:

```json
"light": { "on": 127, "off": 1, "loaded": 56, "playing": { "blink": [9, 0], "ms": 250 } }
```

`frontend/src/app/core/control/profiles.spec.ts` checks that every target exists, that only
encoders read steps, that mode keys choose modes their group has and that meters show
levels the app has.

## From a Mixxx mapping

`tools/controllers/mixxx_profile.py <mapping.midi.xml> [<profile.json>]` converts a Mixxx
mapping: its plain controls, the controls its script names after what they do
(`playButton`, `eqKnob[1]`…), and the lights of its outputs. What only the script knows (a
second use under shift, lights it drives itself, messages it sends at start) is listed as
left out, to be completed by hand. Its tests: `python -m unittest discover tools/controllers`.

## Pioneer DDJ-SB2

`pioneer-ddj-sb2.json` comes from the Mixxx mapping by Be and Michael Stahl (DG3NEC),
GPL-2.0-or-later. The converter gives 318 of its 432 controls: the transport, the mixer
(EQ, kills held, filter, trim, faders, crossfader, headphones mix), the jog wheels, the pads
(hot cues, auto loops, loop rolls held, manual loops), the effect units (unit 1 on deck A, 2
on deck B: on/off and level), the browse knob and its click (load on a free deck). Added by
hand:

- `init`: the request for the positions of the knobs and faders (`9B 09 7F`).
- The jog wheels under shift (controllers 0x1F and 0x26), which still turn the deck.
- Shift + KEY LOCK (note 0x60): the tempo back to the track's.
- The sampler pads (0x30-0x33 on each deck's pad channel): that deck's sounds 1 to 4.
- Shift + the browse knob's click (note 0x42): the track to the prepare list.
- The light of the headphones key under shift (note 0x68).

Left out, Prisme having no action for them: the mute, the vinyl key, the effects' second
and third keys, the level meters, the Auto DJ keys and the other uses under shift (the
headphones of the deck behind, the playlists). Decks 3 and 4 answer in four-deck mode.
