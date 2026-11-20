# Controller profiles

A profile maps a controller model, MIDI or HID, as soon as it is turned on in Settings →
Controllers, before anything is learnt on it. Learning a control on it takes the profile's
place for that control and that action; removing an action removes it from the profile's
controls too.

## Where profiles come from

Three layers, each replacing the one before it for the same file name
(`src/controller_profiles.rs`):

1. The profiles shipped with the version of Prisme (this folder, compiled in).
2. The profiles published since, fetched from this folder on GitHub once a day (or from
   Settings → Controllers → Look for updates) into the data folder's
   `controller-updates`. `index.json` lists them: of each file, the digest of its text, its
   revision (raised at each change) and the version of the format it was written in. A
   Prisme takes a published profile only if it is a later revision than the one it ships, in
   a format it reads (`PROFILE_FORMAT` in `crates/prisme-control`).
3. The DJ's own, in the data folder's `controllers` (Settings → Controllers → Open the
   folder): a file of the same name as a shipped one replaces it, another adds a model.
   **Export as a profile**, beside a controller in use, writes there what it drives now, what
   was learnt included: a start for a profile of one's own, or one to send.

## Schema

`profile.schema.json` is the format as a JSON Schema, generated from the types Prisme reads
(`crates/prisme-control`) with the actions of the app (`targets.json`, from
`crates/prisme-control/src/listed.rs`). A profile naming it as its `"$schema"` is
checked and completed by editors such as VS Code: misspelt fields, unknown actions, MIDI
values out of range, HID fields of a width Prisme does not read.

The tests keep the published files in step: `PRISME_WRITE_TARGETS=1 cargo test -p
prisme-control listed` writes `targets.json`, then `PRISME_WRITE_PROFILES=1 cargo test -p prisme-desktop
controller_profiles` writes `index.json` and `profile.schema.json`. A profile changed or added
gets a later revision in the index then.

## Format

One JSON file per model:

| Field | |
| --- | --- |
| `name` | The model, as the settings show it. |
| `ports` | The controller is the MIDI port whose name holds one of these, case aside; of several models, the longest name wins. None for a HID controller. |
| `hid` | A HID controller: how it is found and what is written to it (see HID controllers). Its controls and lights are then fields of its reports. |
| `credits` | Where the mapping comes from, when it is not Prisme's own. |
| `verified` | `true` once tried on the controller itself; the settings tell a profile that is not. |
| `variant` | `true` for another profile of a model's ports (a firmware or an edition of its own): never found by itself, offered in Settings → Controllers for a controller its ports fit as closely as its model's, the choice kept for that controller. |
| `jogStepsPerTurn` | Steps in a turn of its jog wheels (720 when left out). |
| `init` | MIDI messages sent when the controller is opened (SysEx included); `{ "afterMs": n, "message": [...] }` for one sent that much later (once a light show the model plays by itself is over). To a HID controller, whole output reports, its id the first byte. |
| `close` | MIDI messages (whole reports for HID) sent when it is closed, after its lights went off. |
| `keepAlive` | `everyMs` and `message`: sent again and again while it is open, for the models that want it. |
| `modes` | Groups of controls that drive other actions in each mode, when the controls send the same in each (the pad pages of a side): `group`, `modes` (the first is where it begins), `select` (a key choosing each mode: `control`, `mode`, optionally `shift`, `light`, `held` (the mode only while the key is held: let go, the group is back in the mode before; with `shift`, a supershift) and `once` (the mode for one action of the group, then back to its first). A mode key lights `on` while its mode is the one. A key flipping between two modes is two controls aimed at `mode.<group>.<other>`, each with `when` on one of them: a release is read in the modes of its press, so it does not flip back. |
| `controls` | A control and the action it drives, in the shape of a learnt mapping (`controllers.json`), see below. |
| `lights` | Lights other than the controls' own: `target`, `control`, optionally `when`, and its values by state beside them. |
| `meters` | Levels shown on the controller: `source` (`deck1`…`deck4`, `main`, `main.left`, `main.right`), on one `control` or on `leds` (see Meters), optionally `curve` (the level raised to it, 1 straight) and `when`. |
| `outputs` | Values shown on the controller (a jog wheel's ring, a knob's ring of lights, a tempo strip's LEDs): `source` (a fader's action, `deck1.pitch` where the tempo is included, `deck1.position` of its track, `deck1.turn` of its record at 33⅓), `control`, optionally `min`, `max` (below `min` for lights running the other way) and `when`. |
| `displays` | Numbers shown on the controller's screens, by a message of its own (SysEx, mostly): `source` (`deck1.bpm`; `deck1.elapsed`, `deck1.remaining`, `deck1.duration` in seconds; `deck1.tempo`, the tempo change, and `deck1.tempoRange` in percent), `message` with `"value"` where the number goes, optionally `scale` and `offset` (the number sent is the value times `scale` plus `offset`, rounded), `nibbles` (the number in that many bytes of four bits, the most significant first, two's complement below zero; without, one byte from 0 to 127), `sign` (`[8, 7]`: the first nibble at or above zero, below it) and `when`. A number goes again only when its message changes. |

### Controls

| Field | |
| --- | --- |
| `control` | `kind` (`note`, `controlChange`, `pitchBend`), `channel` 0-15, `number`; or `kind` `hid` and a field of a report (see HID controllers). |
| `target` | The action it drives (see Targets). |
| `encoding` | For encoders: `relative` (two's complement), `relativeOffset` (around 64), `relativeOffsetNoZero` (around 64 with no zero: 65 one step right, 64 one left), `relativeSigned` (a sign bit), `relativeZero` (0 one step right, 127 one left), `absoluteWrap` (a position wrapping around, a jog's angle: its changes are the steps). An encoder may drive a fader's action: its steps move the setting. A HID field's values are read the same over its own bits (a counter of 4 bits wraps at 16). |
| `scale` | An encoder's steps multiplied by this; below 1 they add up to whole steps (`0.1`: a track browsed every ten steps). |
| `step` | How far a step of an encoder moves a fader's action, a share of its travel (1/64 when left out). |
| `invert` | A fader whose lowest value is at its top; an encoder turning the other way. |
| `fine` | The controller sending the low half of a 14-bit control. |
| `shift` | In the second layer: `true` while any shift is held, or a named shift (`"left"`) while the key aimed at `shift.left` is. A control mapped in several layers lights as the one in use. |
| `when` | `group`, `mode`: only in that mode. |
| `onValue` | Drives its action as a key pressed when the control sends this value: a switch's position, one entry per position. |
| `also` | Other actions it drives along. |
| `down` | An encoder pressing keys: a step to the right presses `target`, a step to the left this action. |
| `hold` | `ms`, `target`: another action (or mode) for the key held that long; let go sooner, its own (a mode key chooses its mode as it is let go). |
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
the cue key, and `pressed` for any key while it is held, if it names it (let go, `off` if
its action has no state of its own). Its values by state are numbers, `{ "blink": [a, b],
"ms": n }` for two in turn, or a whole message (`[176, 74, 12]`) for the models addressing
their lights otherwise than by a value on the control's note. A state it does not name shows
as `on`, but `off`; without values, 127 and 0:

```json
"light": { "on": 127, "off": 1, "loaded": 56, "playing": { "blink": [9, 0], "ms": 250 } }
```

### Meters

The level is linear, the peak (1 at full scale). Mixxx's `vu_meter` is on a log scale: its
value t is the level (10^t − 1)/9, which `curve` 0.45 (times a script's own) follows closely.

A meter on one `control` shows the level by its value: from `min` (0, the value at silence)
to `max` (127), or as one of `steps` (the values at silence, then at each equal share of the
level, in place of `min` and `max`), and `clip` for a second after the level reached full
scale.

A meter on `leds` shows it on LEDs switched one by one, each lit once the level (past
`curve`) reaches its `from`, 0 to 1. An LED's values `on` and `off` are a light's (127 and 0
when left out), on its `control`; or whole messages, for the models addressing their LEDs
otherwise, its `control` then any that no other light of the model uses. It shows its `clip`
value, if it names one, for a second after the level reached full scale. Only the LEDs that
change are sent:

```json
{ "source": "deck1", "leds": [
  { "control": { "kind": "controlChange", "channel": 0, "number": 83 }, "from": 0.065, "on": 1 },
  { "control": { "kind": "note", "channel": 0, "number": 10 }, "from": 0.331, "on": [176, 80, 10], "off": [176, 81, 10] },
  { "control": { "kind": "controlChange", "channel": 0, "number": 85 }, "from": 1, "on": 1, "clip": 1 }
] }
```

A meter's controls show the level at silence once the controller is closed. A peak LED of
its own is rather a light of `deck1.clip` or `main.clip`.

The tests of `apps/prisme-desktop/src/controller_profiles.rs` check that every target exists,
that only encoders read steps, that mode keys choose modes their group has and that meters show
levels the app has.

## HID controllers

Many controllers speak HID rather than MIDI (Native Instruments' Traktor Kontrol, older
Hercules, Pioneer CDJs in HID mode). Prisme reads them through hidapi on Windows, macOS and
Linux (`src/controller_hid.rs`); on Linux, the DJ needs read and write access to the device's
`/dev/hidraw*` (a udev rule, as for Mixxx). Android and iOS have no HID controllers yet.

A HID device is named `<product> (HID <vendor>:<product id>)`, `2- ` before it for the second
of a model, and listed with the MIDI ports in Settings → Controllers. One of a model Prisme
knows is opened when plugged in, as a MIDI controller is (or waited for while another program
holds it); the others are listed apart, not offered, as most HID devices are no controllers
(keyboards, mice, touch screens and the computer's own are never listed). A controller seen
both as a MIDI port and as a HID device (a Hercules with its driver) is used one way only: by
HID if Prisme knows it so and not by MIDI, else by MIDI, its maker's driver then speaking to
the device itself.

### The device

```json
"hid": {
  "vendorId": 6092, "productId": 4880, "interface": 4, "usagePage": 65281, "usage": 1,
  "outputs": [{ "report": 128, "length": 53, "bytes": { "42": 127 } }, { "report": 129, "length": 63 }],
  "leds": [{ "control": { "kind": "hid", "report": 1, "byte": 13 }, "led": { "kind": "hid", "report": 129, "byte": 32, "bits": 8 } }]
}
```

| Field | |
| --- | --- |
| `vendorId`, `productId` | The device's USB ids: they tell its model, whatever the system calls it. |
| `interface`, `usagePage`, `usage` | Which of its interfaces or collections holds its controls, when it has several (its sound card's, say). A device the system tells no interface of (macOS, at times) is told by its usage page and usage alone. |
| `inputs` | The input reports asked of it once it is opened, by id (`[1, 2]`): its faders' and knobs' positions known before they move, as a script's `getInputReport` does. |
| `outputs` | Its output reports: `report` (its id), `length` (its bytes, the id included) and `bytes` it always holds by their place (a header, a light always on). A report is written whole, again only once one of its bytes changed, at most every 10 ms, never from the audio thread. |
| `rewrite` | `true`: its output reports written again after each report it sends, changed or not, for a model that puts lights of its own back on a key pressed (the Traktor Kontrol F1). |
| `leds` | The light of each key that has one: `control`, the key's field, and `led`, a field of an output report. A HID key lights there, not on itself. |
| `brightness` | A light's value at full (127): the colours of three-colour lights are scaled to it. |

### Fields

A control, and a light, is a field of a report:

```json
{ "kind": "hid", "report": 1, "byte": 13, "bit": 2 }
{ "kind": "hid", "report": 2, "byte": 9, "bits": 12 }
```

| Field | |
| --- | --- |
| `report` | The id of its report; 0 (left out) for a device that does not number its reports. |
| `byte` | Its first byte, counted from the report's first as the system gives it: an input report from its id when the device numbers its reports (else from its first byte of data), an output report always from its id (0 for a device that does not number them). Mixxx's scripts count the same: their offsets are taken as they are. |
| `bit` | Its lowest bit in that byte, 0 to 7 (0 left out). |
| `bits` | How many bits it has: 1 (left out) for a key or a light, up to 16 for a value; 24 for a light of three colours (its red, green and blue bytes). A key is a field of one bit: pressed at 1. |
| `bigEndian` | Its bytes from the most significant, when it has several (little-endian otherwise). |

A field of 12 bits is a fader from 0 to 4095. A jog wheel's counter is a field read with
`absoluteWrap` (its changes are the steps; `jogStepsPerTurn` the steps of a turn), as are the
encoders that count up and down in a nibble. A field of more than 16 bits is read by its low 16:
a counter wraps the same.

Each report read is compared with the one of its id before it: a field whose value changed is
an event of its control, as a MIDI message is, and goes through all the rest unchanged (layers,
modes, holds, chords, steps, lights by state). The first report of an id gives the position of
every fader, but not the keys at rest.

### Lights

A mapped key lights at its `led`; the `lights`, `meters` and `outputs` name fields of the
output reports directly. A light of one bit is lit by any value above 0; a byte takes the value
(0-127 for most, 9 a dim light on a Native Instruments controller); a light of three colours
takes three values (`"on": [127, 4, 4]`), or one value for all three, and a hot cue's colour
when its action has one and its control no `palette`:

```json
{ "control": { "kind": "hid", "report": 1, "byte": 13, "bit": 7 }, "target": "deck1.hotcue1", "light": { "on": [127, 4, 4], "off": [8, 1, 1] } }
```

### Learning

On a HID controller, the field that changes is the control learnt: a field of its profile if it
holds the change; else a key pressed is its bit, an encoder the nibble or byte that counts, a
fader the byte or two that changed. A fader of a model Prisme does not know is best given its
width in a profile.

### Without the controller

A debug build plugs in a simulated HID device, which Prisme takes as it takes a real one, by
its ids: commands feed it reports and read back what Prisme wrote to it, over CDP
(`window.__TAURI_INTERNALS__.invoke(...)`):

| Command | |
| --- | --- |
| `simulate_hid_plug` | `{ device: { product, vendorId, productId, interface?, usagePage?, usage?, numbered?, lengths: { "<report id>": <bytes> } } }`: plugged in; gives back `{ id, name }`. |
| `simulate_hid_report` | `{ id, report: [...] }`: a report, as the device would send it. |
| `simulate_hid_set` | `{ id, values: [{ control, value }] }`: the last report of each field's id, changed by the values. |
| `simulated_hid_written` | `{ id }`: the output reports written to it since last asked. |
| `simulate_hid_unplug` | `{ id }`. |

The input reports a model asks for when it is opened (`inputs`) are the last of their ids it was
fed, zeros before.

## From a Mixxx mapping

`tools/controllers/mixxx_profile.py <mapping.midi.xml> [<profile.json>]` converts a Mixxx
mapping: its plain controls, the controls its script names after what they do
(`playButton`, `eqKnob[1]`…), and the lights of its outputs. What only the script knows (a
second use under shift, lights it drives itself, messages it sends at start) is listed as
left out, to be completed by hand. Its tests: `python -m unittest discover tools/controllers`.

## Pioneer DDJ-SB2

`pioneer-ddj-sb2.json` comes from the Mixxx mapping by Be and Michael Stahl (DG3NEC),
GPL-2.0-or-later, read by hand with its script; not yet tried on the controller. Decks 3 and
4 answer on their own channels in four-deck mode, as the controller sends them.

- Transport: shift + PLAY reverses, shift + CUE goes to the start, stopped. SYNC tapped
  matches the other deck once (or lets a locked sync go), held a quarter of a second locks
  it; its light shows the lock. Shift + SYNC: quantize.
- The shift keys are named by side (`shift.left`, `shift.right`): each shifts its own deck's
  knobs only.
- Jog wheels: VINYL chooses whether a touch scratches or only nudges; the ring under shift
  searches through the track, the platter under shift still scratches or nudges. Shift +
  VINYL: slip while held.
- Shift + KEY LOCK: the tempo back to the track's. Shift + the headphones key: the deck
  behind's (1 and 3, 2 and 4).
- Effects: unit 1 is deck A's chain, unit 2 deck B's. A key focuses its effect, or lets it
  go, and lights while it is focused; held a quarter of a second, it switches the effect. The
  knob is the three effects' levels together while none is focused, the focused effect's
  amount otherwise; under shift, the gain, high and mid knobs of the side are the focused
  effect's amount, second knob and period. The focus is a mode of the profile
  (`fx1Knob`, `fx2Knob`), the app's focus moved along.
- Browse knob: the tracks, under shift the lists; its click previews the track in the
  headphones (or stops the preview), shifted it opens the list pointed at.
- Pads, as the controller pages them: hot cues 1-4 and 5-8 (cue loop page), deleted under
  shift; auto loop (a loop of the deck's size, halve, double, reloop; shifted: a 4-beat roll,
  jumps back and ahead, back to the loop stopped); manual loop (in, out, reloop, halve;
  shifted: the loop moved a beat back or ahead, double); rolls 1/16 to 8; kills and mute while
  held; sampler (play, stop under shift, load, eject under shift).
- Auto DJ keys: skip on the left, on/off on the right. Level meters per deck, red at clip.

Left out: the fader start under shift, the gradual return of the tempo, the focused effect's
fourth and fifth parameters (low EQ and filter knobs under shift), the chain presets, the
skin's panels, the meters twinkling under Auto DJ.

## Native Instruments Traktor Kontrol S4 MK2

`native-instruments-traktor-kontrol-s4-mk2.json` (HID) comes from the Mixxx mapping by Owen
Williams and Fayaaz Ahmed, GPL-2.0-or-later, read by hand with its script (its generator:
`prisme-controllers-staging/hid/`); not yet tried on the controller.

- Decks: the deck switch of each side flips it between A and C (left) or B and D (right), its
  light dim on A or B, full on C or D; the deck letters light. The mixer's channels are decks 1
  to 4.
- Transport: shift + PLAY the key lock, shift + CUE back to the start (stopped). SYNC tapped
  matches once (or lets a lock go), held 300 ms locks; shift + SYNC toggles the lock.
- The shift keys are named by side (`shift.left`, `shift.right`): each shifts its own deck's
  keys; the mixer's, the browser's and the effect keys take either.
- Jog wheels: 1024 steps a turn (the counter's byte), the top touched; under shift, a search.
- Hot cues 1-4, deleted under shift, red on A and B and blue on C and D as the script has them,
  in their own colour when set. The remix pads: samples 1-4 of the deck (load, play, stop), emptied
  under shift.
- Loops: in and out, lit while a loop plays; the loop encoder doubles or halves the loop, under
  shift moves the tempo by 0.1 %; its push a loop of the size, under shift the key back. The move
  encoder jumps by the size, under shift doubles or halves it; its push reloops (and stops,
  under shift). SLIP slips; under shift the tempo fader's previous range, RESET its next.
- Effects: each side's unit is its deck's chain: its three keys turn the effects on and off (the
  next effect under shift), its three knobs their amounts, its mix knob their levels together.
  A channel's two FX assign keys turn its deck's chain on and off.
- Mixer: volume, EQ, filter, the gain encoder (0 dB on its push; under shift it moves the beat
  grid, under the browser's PLAY key its tempo), headphones, crossfader, cue mix. Six-LED meters
  per channel, the peak LEDs, the main clip LEDs, the loaded lights.
- Browser: the knob through the tracks, under shift the lists; its push loads or opens; PREVIEW,
  QUANTIZE, SNAP (the library), the record key and its lights.

Left out: the loop size shown on the seven-segment displays, the auto-slip mode (shift + PLAY
key of the browser), rounding the tempo, the effect units' mode keys, the remix pads as loop rolls
(a setting of the script), the internal clock's leader light.
