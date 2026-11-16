# Controller profiles

A profile maps a controller model as soon as it is turned on in Settings → MIDI
controllers, before anything is learnt on it. Learning a control on it takes the profile's
place for that control and that action; removing an action removes it from the profile's
controls too.

## Where profiles come from

Three layers, each replacing the one before it for the same file name
(`src/controller_profiles.rs`):

1. The profiles shipped with the version of Prisme (this folder, compiled in).
2. The profiles published since, fetched from this folder on GitHub once a day (or from
   Settings → MIDI controllers → Look for updates) into the data folder's
   `controller-updates`. `index.json` lists them: of each file, the digest of its text, its
   revision (raised at each change) and the version of the format it was written in. A
   Prisme takes a published profile only if it is a later revision than the one it ships, in
   a format it reads (`PROFILE_FORMAT` in `crates/prisme-control`).
3. The DJ's own, in the data folder's `controllers` (Settings → MIDI controllers → Open the
   folder): a file of the same name as a shipped one replaces it, another adds a model.
   **Export as a profile**, beside a controller in use, writes there what it drives now, what
   was learnt included: a start for a profile of one's own, or one to send.

## Schema

`profile.schema.json` is the format as a JSON Schema, generated from the types Prisme reads
(`crates/prisme-control`) with the actions of the app (`targets.json`, from
`frontend/src/app/core/control/controls.ts`). A profile naming it as its `"$schema"` is
checked and completed by editors such as VS Code: misspelt fields, unknown actions, MIDI
values out of range.

The tests keep the published files in step: `PRISME_WRITE_TARGETS=1 npm test` in `frontend`
writes `targets.json`, then `PRISME_WRITE_PROFILES=1 cargo test -p prisme-desktop
controller_profiles` writes `index.json` and `profile.schema.json`. A profile changed or added
gets a later revision in the index then.

## Format

One JSON file per model:

| Field | |
| --- | --- |
| `name` | The model, as the settings show it. |
| `ports` | The controller is the MIDI port whose name holds one of these, case aside; of several models, the longest name wins. |
| `credits` | Where the mapping comes from, when it is not Prisme's own. |
| `verified` | `true` once tried on the controller itself; the settings tell a profile that is not. |
| `jogStepsPerTurn` | Steps in a turn of its jog wheels (720 when left out). |
| `init` | MIDI messages sent when the controller is opened (SysEx included); `{ "afterMs": n, "message": [...] }` for one sent that much later (once a light show the model plays by itself is over). |
| `close` | MIDI messages sent when it is closed, after its lights went off. |
| `keepAlive` | `everyMs` and `message`: sent again and again while it is open, for the models that want it. |
| `modes` | Groups of controls that drive other actions in each mode, when the controls send the same in each (the pad pages of a side): `group`, `modes` (the first is where it begins), `select` (a key choosing each mode: `control`, `mode`, optionally `shift`, `light`, `held` (the mode only while the key is held: let go, the group is back in the mode before; with `shift`, a supershift) and `once` (the mode for one action of the group, then back to its first). A mode key lights `on` while its mode is the one. A key flipping between two modes is two controls aimed at `mode.<group>.<other>`, each with `when` on one of them: a release is read in the modes of its press, so it does not flip back. |
| `controls` | A control and the action it drives, in the shape of a learnt mapping (`controllers.json`), see below. |
| `lights` | Lights other than the controls' own: `target`, `control`, optionally `when`, and its values by state beside them. |
| `meters` | Levels shown on the controller: `source` (`deck1`…`deck4`, `main`, `main.left`, `main.right`), `control`, optionally `min` (0, the value at silence), `max` (127), `curve` (the level raised to it, 1 straight), `steps` (the values at silence, then at each equal share of the level, in place of `min` and `max`), `clip` (the value shown for a second after the level reached full scale) and `when`. |
| `outputs` | Values shown on the controller (a jog wheel's ring, a knob's ring of lights): `source` (a fader's action, `deck1.position` of its track, `deck1.turn` of its record at 33⅓), `control`, optionally `min`, `max` and `when`. |
| `displays` | Numbers shown on the controller's screens, by a message of its own (SysEx, mostly): `source` (`deck1.bpm`; `deck1.elapsed`, `deck1.remaining`, `deck1.duration` in seconds; `deck1.tempo`, the tempo change, and `deck1.tempoRange` in percent), `message` with `"value"` where the number goes, optionally `scale` and `offset` (the number sent is the value times `scale` plus `offset`, rounded), `nibbles` (the number in that many bytes of four bits, the most significant first, two's complement below zero; without, one byte from 0 to 127), `sign` (`[8, 7]`: the first nibble at or above zero, below it) and `when`. A number goes again only when its message changes. |

### Controls

| Field | |
| --- | --- |
| `control` | `kind` (`note`, `controlChange`, `pitchBend`), `channel` 0-15, `number`. |
| `target` | The action it drives (see Targets). |
| `encoding` | For encoders: `relative` (two's complement), `relativeOffset` (around 64), `relativeSigned` (a sign bit), `relativeZero` (0 one step right, 127 one left), `absoluteWrap` (a position wrapping around, a jog's angle: its changes are the steps). An encoder may drive a fader's action: its steps move the setting. |
| `scale` | An encoder's steps multiplied by this; below 1 they add up to whole steps (`0.1`: a track browsed every ten steps). |
| `step` | How far a step of an encoder moves a fader's action, a share of its travel (1/64 when left out). |
| `invert` | A fader whose lowest value is at its top; an encoder turning the other way. |
| `fine` | The controller sending the low half of a 14-bit control. |
| `shift` | In the second layer: `true` while any shift is held, or a named shift (`"left"`) while the key aimed at `shift.left` is. |
| `when` | `group`, `mode`: only in that mode. |
| `onValue` | Drives its action as a key pressed when the control sends this value: a switch's position, one entry per position. |
| `also` | Other actions it drives along. |
| `down` | An encoder pressing keys: a step to the right presses `target`, a step to the left this action. |
| `hold` | `ms`, `target`: another action (or mode) for the key held that long; let go sooner, its own. |
| `double` | `ms`, `target`: another action for the key pressed twice within `ms`. |
| `chord` | `with` (a control), `target`: another action for the key pressed while `with` is held. |
| `set` | A key putting a fader's action at a position (`0.5`), or at each of two in turn (`[0, 1]`, lit at the second). |
| `latch` | A key driving an action while held, latched: a press holds it, the next lets it go. |
| `light` | Its values by state (see Lights). |
| `palette` | Its values by the colour of its action, a hot cue's (`{ "#ff0000": 5, "#0000ff": 45 }`): the nearest colour's. |

### Targets

An action id of `frontend/src/app/core/control/controls.ts` (`deck1.play`, `deck2.hotcue3`,
`deck1.fx.next`, `deck1.key.up`, `deck1.jog.search`, `general.quantize`, `mixer.crossfader`…),
a jump or a loop of so many beats (`deck1.jump.-8`, `deck1.jump.16`, `deck1.loop.0.25`), the
key moved so many semitones (`deck1.key.+2`), a sample played as hard as its pad is hit
(`deck1.sample3.velocity`), or a mode key (`mode.<group>.<mode>`, made from `modes`).

### Lights

A light shows the state of its action: `on` and `off` for most, `off`, `loaded` and `playing`
for the sampler's pads, `on` (on the cue point), `paused` (stopped elsewhere) and `off` for
the cue key, and `pressed` for any key while it is held, if it names it. Its values by state
are numbers, `{ "blink": [a, b], "ms": n }` for two in turn, or a whole message (`[176, 74,
12]`) for the models addressing their lights otherwise than by a value on the control's
note. A state it does not name shows as `on`, but `off`; without values, 127 and 0:

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
