"""Turns a Mixxx controller mapping (*.midi.xml) into a Prisme controller profile.

The plain controls of the mapping, and those its script names after what they do
(playButton, eqKnob[1]...), become Prisme actions; their lights come from its outputs.
What only the script knows (a second use under shift, a jog's details, lights it drives
itself, messages sent at start) is left out and listed, to be completed by hand.

usage: python mixxx_profile.py <mapping.midi.xml> [<profile.json>]
"""

from __future__ import annotations

import json
import re
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

PADS = 8

# the actions of a deck driven by Mixxx's own keys
DECK_KEYS = {
    'play': 'play',
    'start_play': 'play',
    'cue_default': 'cue',
    'cue_play': 'cue',
    'sync_enabled': 'sync',
    'beatsync': 'sync',
    'keylock': 'keylock',
    'pfl': 'headphones',
    'volume': 'volume',
    'rate': 'pitch',
    'pregain': 'trim',
    'slip_enabled': 'slip',
    'reloop_toggle': 'reloop',
    'reloop_exit': 'reloop',
    'reloop_andstop': 'reloop',
    'loop_in': 'loop.in',
    'loop_out': 'loop.out',
    'loop_halve': 'loop.halve',
    'loop_double': 'loop.double',
    'beatloop_activate': 'loop',
    'beatloop_4_toggle': 'loop',
    'beatloop_4_activate': 'loop',
    'beatjump_backward': 'jump.back',
    'beatjump_4_backward': 'jump.back',
    'beatjump_forward': 'jump.forward',
    'beatjump_4_forward': 'jump.forward',
    'rate_temp_down': 'bend.down',
    'rate_temp_up': 'bend.up',
    'jog': 'jog',
    'LoadSelectedTrack': 'library.load{deck}',
}
HOTCUE_KEY = re.compile(r'^hotcue_(\d+)_(activate|clear)$')
# the loop rolls of a deck, as long as Prisme's pads and controls have them
ROLL_BEATS = [1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2, 4, 8]
ROLL_KEY = re.compile(r'^beatlooproll_([\d.]+)_activate$')

GLOBAL_KEYS = {
    ('[Master]', 'crossfader'): 'mixer.crossfader',
    ('[Playlist]', 'SelectTrackKnob'): 'library.browse',
    ('[Playlist]', 'SelectNextTrack'): 'library.next',
    ('[Playlist]', 'SelectPrevTrack'): 'library.previous',
    ('[Library]', 'MoveVertical'): 'library.browse',
    ('[Library]', 'MoveDown'): 'library.next',
    ('[Library]', 'MoveUp'): 'library.previous',
    ('[Library]', 'GoToItem'): 'library.load',
    ('[Playlist]', 'LoadSelectedIntoFirstStopped'): 'library.load',
    ('[Master]', 'headMix'): 'mixer.cueMix',
}
EQ_GROUP = re.compile(r'^\[EqualizerRack1_\[Channel(\d)\]_Effect1\]$')
EQ_KEYS = {
    'parameter1': 'eq.low',
    'parameter2': 'eq.mid',
    'parameter3': 'eq.high',
    'button_parameter1': 'eq.low.kill',
    'button_parameter2': 'eq.mid.kill',
    'button_parameter3': 'eq.high.kill',
}
# an effect unit of Mixxx drives the effect of the deck of the same number
EFFECT_GROUP = re.compile(r'^\[EffectRack1_EffectUnit(\d)\]$')
EFFECT_KEYS = {'mix': 'fx.mix', 'super1': 'fx.amount', 'enabled': 'fx'}
EFFECT_SCRIPT = re.compile(r'effectunit\[(\d)\]\.(knob|button\[1\])')
FILTER_GROUP = re.compile(r'^\[QuickEffectRack1_\[Channel(\d)\]\]$')

# what a script's handlers do, read from their names, first match first
SCRIPT_NAMES = [
    (r'crossfader', 'mixer.crossfader'),
    (r'jog.?touch|wheel.?touch', 'jog.touch'),
    (r'jog|wheel|platter', 'jog'),
    (r'headphone|pfl', 'headphones'),
    (r'clearhotcue|hotcue.*(clear|delete)', 'hotcue{n}.delete'),
    (r'hotcue', 'hotcue{n}'),
    (r'roll', 'roll{n}'),
    (r'lowkill|killlow', 'eq.low.kill'),
    (r'midkill|killmid', 'eq.mid.kill'),
    (r'hi(gh)?kill|killhi', 'eq.high.kill'),
    (r'headmix|cuemix', 'mixer.cueMix'),
    (r'keylock', 'keylock'),
    (r'loopin', 'loop.in'),
    (r'loopout', 'loop.out'),
    (r'halve', 'loop.halve'),
    (r'double', 'loop.double'),
    (r'reloop|loopexit', 'reloop'),
    (r'loopmoveback|jump.*back', 'jump.back'),
    (r'loopmoveforward|jump.*forw', 'jump.forward'),
    (r'beatloop|autoloop', 'loop'),
    (r'slip', 'slip'),
    (r'play', 'play'),
    (r'cue', 'cue'),
    (r'sync', 'sync'),
    (r'tempo|pitchfader|ratefader', 'pitch'),
    (r'gain|trim', 'trim'),
    (r'eqknob\[1\]|lowknob', 'eq.low'),
    (r'eqknob\[2\]|midknob', 'eq.mid'),
    (r'eqknob\[3\]|hi(gh)?knob', 'eq.high'),
    (r'quickeffect|filter', 'filter'),
    (r'deckfader|channelfader|volume', 'volume'),
    (r'load', 'library.load{deck}'),
    (r'(rotaryselector|browse|selecttrack).*(click|press|push)', 'library.load'),
    (r'rotaryselector|browse|selecttrack', 'library.browse'),
]
# what Prisme has no action for, or what a script does that its name does not tell
NOT_ACTIONS = re.compile(
    r'mute|sampler|(?<!quick)effect|fx|vinyl|autodj|preview|vu|meter|tap', re.I
)
SHIFT_BUTTON = re.compile(r'(^|\.)shift(button)?(\.input)?$', re.I)
SHIFTED = re.compile(r'shift', re.I)

OUTPUT_KEYS = {
    'play_indicator': 'play',
    'play': 'play',
    'cue_indicator': 'cue',
    'sync_enabled': 'sync',
    'keylock': 'keylock',
    'pfl': 'headphones',
    'loop_enabled': 'loop',
    'slip_enabled': 'slip',
}
HOTCUE_OUTPUT = re.compile(r'^hotcue_(\d+)_(enabled|status)$')


@dataclass(frozen=True)
class Control:
    kind: str
    channel: int
    number: int

    @staticmethod
    def of(status: int, number: int) -> Control | None:
        kind = {0x80: 'note', 0x90: 'note', 0xB0: 'controlChange', 0xE0: 'pitchBend'}.get(status & 0xF0)
        if kind is None:
            return None
        return Control(kind, status & 0x0F, 0 if kind == 'pitchBend' else number)

    def json(self) -> dict:
        return {'kind': self.kind, 'channel': self.channel, 'number': self.number}


@dataclass
class Entry:
    """A <control> or an <output> of the mapping."""

    group: str
    key: str
    control: Control
    options: set[str] = field(default_factory=set)
    on: int = 127
    off: int = 0

    @property
    def half(self) -> str | None:
        """'msb' or 'lsb' for a half of a 14-bit control."""
        if 'fourteen-bit-lsb' in self.options or re.search(r'lsb$', self.key, re.I):
            return 'lsb'
        if 'fourteen-bit-msb' in self.options or re.search(r'msb$', self.key, re.I):
            return 'msb'
        return None


def deck_of(entry: Entry) -> int | None:
    found = re.search(r'deck\[(\d)\]', entry.key) or re.search(r'\[Channel(\d)\]', entry.group)
    return int(found.group(1)) if found else None


def action_of(entry: Entry) -> str | None:
    """The Prisme action the control drives, `{n}` standing for a pad's number; `None`
    when the mapping does not tell."""
    key, group = entry.key, entry.group
    if (group, key) in GLOBAL_KEYS:
        return GLOBAL_KEYS[(group, key)]
    eq = EQ_GROUP.match(group)
    if eq and key in EQ_KEYS:
        return f'deck{eq.group(1)}.{EQ_KEYS[key]}'
    quick = FILTER_GROUP.match(group)
    if quick and key == 'super1':
        return f'deck{quick.group(1)}.filter'
    unit = EFFECT_GROUP.match(group)
    if unit and key in EFFECT_KEYS:
        return f'deck{unit.group(1)}.{EFFECT_KEYS[key]}'
    deck = deck_of(entry)
    roll = ROLL_KEY.match(key)
    if roll and deck and float(roll.group(1)) in ROLL_BEATS:
        return f'deck{deck}.roll{ROLL_BEATS.index(float(roll.group(1))) + 1}'
    hotcue = HOTCUE_KEY.match(key)
    if hotcue and deck and int(hotcue.group(1)) <= PADS:
        suffix = '.delete' if hotcue.group(2) == 'clear' else ''
        return f'deck{deck}.hotcue{hotcue.group(1)}{suffix}'
    if key in DECK_KEYS:
        return named(DECK_KEYS[key], deck)
    if 'script-binding' not in entry.options:
        return None
    if SHIFT_BUTTON.search(key):
        return 'shift'
    if SHIFTED.search(key):
        return None
    lowered = key.lower()
    effect = EFFECT_SCRIPT.search(lowered)
    if effect:
        return f"deck{effect.group(1)}.{'fx.mix' if effect.group(2) == 'knob' else 'fx'}"
    if NOT_ACTIONS.search(key):
        return None
    for pattern, action in SCRIPT_NAMES:
        if re.search(pattern, lowered):
            return named(action, deck)
    return None


def named(action: str, deck: int | None) -> str | None:
    """`action` on `deck`, for the actions of a deck."""
    if '{deck}' in action:
        return action.replace('{deck}', str(deck)) if deck else None
    if action.startswith(('mixer.', 'library.')):
        return action
    return f'deck{deck}.{action}' if deck else None


def is_encoder(action: str) -> bool:
    return action == 'library.browse' or action.endswith('.jog')


def number(text: str | None) -> int | None:
    """A number as Mixxx writes it: 0x90, or 144."""
    if not text or not text.strip():
        return None
    text = text.strip()
    return int(text, 16) if text.lower().startswith('0x') else int(text)


def read(path: Path) -> tuple[ET.Element, list[Entry], list[Entry]]:
    root = ET.parse(path).getroot()

    def entries(tag: str) -> list[Entry]:
        found = []
        for e in root.iter(tag):
            status, midino = number(e.findtext('status')), number(e.findtext('midino'))
            control = Control.of(status, midino) if status is not None and midino is not None else None
            if control is None:
                continue
            options = {o.tag for o in (e.find('options') if e.find('options') is not None else [])}
            on, off = number(e.findtext('on')), number(e.findtext('off'))
            found.append(
                Entry(
                    group=e.findtext('group') or '',
                    key=e.findtext('key') or '',
                    control=control,
                    options=options,
                    on=127 if on is None else on,
                    off=0 if off is None else off,
                )
            )
        return found

    return root, entries('control'), entries('output')


def convert(path: Path) -> tuple[dict, dict]:
    """The profile of the mapping at `path`, and what was and was not converted."""
    root, controls, outputs = read(path)
    name = root.findtext('info/name') or path.stem
    author = root.findtext('info/author') or ''
    port = root.find('controller').get('id') if root.find('controller') is not None else name

    actions = [(entry, action_of(entry)) for entry in controls]
    numbered = number_pads(actions)

    mapped: dict[Control, dict] = {}
    low_halves: dict[tuple[str, int], set[int]] = {}
    left_out = Counter()
    for entry, action in numbered:
        if action is None:
            left_out[entry.key] += 1
            continue
        if entry.half == 'lsb':
            if entry.control.kind == 'controlChange':
                low_halves.setdefault((action, entry.control.channel), set()).add(entry.control.number)
            continue
        if entry.control in mapped:
            continue
        line = {'control': entry.control.json(), 'target': action}
        if is_encoder(action) and entry.control.kind == 'controlChange':
            twos_complement = 'selectknob' in entry.options or action == 'library.browse'
            line['encoding'] = 'relative' if twos_complement else 'relativeOffset'
        mapped[entry.control] = line
    for line in mapped.values():
        lsb = low_half(line, low_halves.get((line['target'], line['control']['channel']), set()))
        if lsb is not None:
            line['fine'] = lsb

    lights = []
    for entry in outputs:
        target = light_of(entry)
        if target is None:
            continue
        own = mapped.get(entry.control)
        if own is not None and own['target'] == target and (entry.on, entry.off) == (127, 0):
            continue
        light = {'target': target, 'control': entry.control.json()}
        if entry.on != 127:
            light['on'] = entry.on
        if entry.off != 0:
            light['off'] = entry.off
        lights.append(light)

    profile = {
        'name': name,
        'ports': [port],
        'credits': f'From the {name} mapping of Mixxx{f" by {author}" if author else ""} (GPL-2.0-or-later).',
        'controls': list(mapped.values()),
    }
    if lights:
        profile['lights'] = lights
    converted = sum(1 for _, action in numbered if action is not None)
    report = {'controls': len(controls), 'converted': converted, 'leftOut': dict(left_out)}
    return profile, report


def low_half(line: dict, numbers: set[int]) -> int | None:
    """The controller sending the low half of a 14-bit control: by convention the number
    32 higher, else the only one its action has."""
    if line['control']['kind'] != 'controlChange':
        return None
    number = line['control']['number']
    if number + 32 in numbers:
        return number + 32
    return next(iter(numbers)) if len(numbers) == 1 else None


def number_pads(actions: list[tuple[Entry, str | None]]) -> list[tuple[Entry, str | None]]:
    """Gives the pads their number: a mapping's hot cue keys by name, a script's in the
    order of their notes on each deck."""
    order: dict[tuple[str, int], list[int]] = {}
    for entry, action in actions:
        if action and '{n}' in action:
            order.setdefault((action, entry.control.channel), []).append(entry.control.number)
    numbered = []
    for entry, action in actions:
        if action and '{n}' in action:
            notes = sorted(set(order[(action, entry.control.channel)]))
            n = notes.index(entry.control.number) + 1
            action = action.replace('{n}', str(n)) if n <= PADS else None
        numbered.append((entry, action))
    return numbered


def light_of(entry: Entry) -> str | None:
    deck = deck_of(entry)
    if deck is None:
        return None
    hotcue = HOTCUE_OUTPUT.match(entry.key)
    if hotcue and int(hotcue.group(1)) <= PADS:
        return f'deck{deck}.hotcue{hotcue.group(1)}'
    action = OUTPUT_KEYS.get(entry.key)
    return f'deck{deck}.{action}' if action else None


def write(profile: dict) -> str:
    """The profile as JSON, a control or a light a line."""
    def lines(items: list[dict]) -> str:
        return ',\n'.join('    ' + json.dumps(item) for item in items)

    head = [f'  {json.dumps(k)}: {json.dumps(v)}' for k, v in profile.items() if k not in ('controls', 'lights')]
    text = '{\n' + ',\n'.join(head)
    text += ',\n  "controls": [\n' + lines(profile['controls']) + '\n  ]'
    if profile.get('lights'):
        text += ',\n  "lights": [\n' + lines(profile['lights']) + '\n  ]'
    return text + '\n}\n'


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__.strip().splitlines()[-1])
        return 2
    profile, report = convert(Path(argv[1]))
    text = write(profile)
    if len(argv) == 3:
        Path(argv[2]).write_text(text, encoding='utf-8')
    else:
        print(text)
    print(
        f"{report['converted']} of {report['controls']} controls converted",
        file=sys.stderr,
    )
    for key, count in sorted(report['leftOut'].items()):
        print(f'  left out: {key} ({count})', file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
