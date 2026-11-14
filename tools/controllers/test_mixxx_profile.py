"""python -m unittest discover tools/controllers"""

import json
import tempfile
import unittest
from pathlib import Path

from mixxx_profile import convert, write

MIXXX = Path(r'C:\PROGRAMMATION\mixxx\res\controllers')

MAPPING = """<?xml version="1.0" encoding="UTF-8"?>
<MixxxControllerPreset mixxxVersion="2.0+" schemaVersion="1">
  <info><name>Brand DJ-1</name><author>Someone</author></info>
  <controller id="DJ-1">
    <controls>
      <control><group>[Channel1]</group><key>play</key><status>0x90</status><midino>0x0B</midino><options><normal/></options></control>
      <control><group>[Channel2]</group><key>DJ1.deck[2].playButton.input</key><status>0x91</status><midino>0x0B</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.shiftButton</key><status>0x90</status><midino>0x3F</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.jogRingTickShift</key><status>0xB0</status><midino>0x26</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.jogRingTick</key><status>0xB0</status><midino>0x21</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.tempoFaderMSB</key><status>0xB0</status><midino>0x00</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.tempoFaderLSB</key><status>0xB0</status><midino>0x20</midino><options><script-binding/></options></control>
      <control><group>[Master]</group><key>crossfader</key><status>0xB6</status><midino>0x1F</midino><options><fourteen-bit-msb/></options></control>
      <control><group>[Master]</group><key>crossfader</key><status>0xB6</status><midino>0x3F</midino><options><fourteen-bit-lsb/></options></control>
      <control><group>[EqualizerRack1_[Channel1]_Effect1]</group><key>parameter1</key><status>0xB0</status><midino>0x0F</midino><options><normal/></options></control>
      <control><group>[Channel1]</group><key>DJ1.hotCueButtons</key><status>0x97</status><midino>0x40</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>DJ1.hotCueButtons</key><status>0x97</status><midino>0x00</midino><options><script-binding/></options></control>
      <control><group>[Channel1]</group><key>hotcue_3_clear</key><status>0x97</status><midino>0x0A</midino><options><normal/></options></control>
      <control><group>[Channel1]</group><key>DJ1.lowKillButton</key><status>0x97</status><midino>0x60</midino><options><script-binding/></options></control>
      <control><group>[Playlist]</group><key>SelectTrackKnob</key><status>0xB6</status><midino>0x40</midino><options><selectknob/></options></control>
    </controls>
    <outputs>
      <output><group>[Channel1]</group><key>play_indicator</key><status>0x90</status><midino>0x0B</midino><on>0x7F</on><off>0x00</off></output>
      <output><group>[Channel1]</group><key>play_indicator</key><status>0x90</status><midino>0x47</midino><on>0x7F</on><off>0x00</off></output>
      <output><group>[Channel1]</group><key>keylock</key><status>0x90</status><midino>0x1A</midino><on>0x40</on><off>0x01</off></output>
    </outputs>
  </controller>
</MixxxControllerPreset>
"""


def converted(xml: str = MAPPING) -> tuple[dict, dict]:
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / 'mapping.midi.xml'
        path.write_text(xml, encoding='utf-8')
        return convert(path)


def target(profile: dict, kind: str, channel: int, number: int) -> dict | None:
    control = {'kind': kind, 'channel': channel, 'number': number}
    return next((c for c in profile['controls'] if c['control'] == control), None)


class Conversion(unittest.TestCase):
    def setUp(self):
        self.profile, self.report = converted()

    def test_the_model_is_named_and_credited(self):
        self.assertEqual(self.profile['name'], 'Brand DJ-1')
        self.assertEqual(self.profile['ports'], ['DJ-1'])
        self.assertIn('Someone', self.profile['credits'])

    def test_plain_keys_and_script_names_become_actions(self):
        self.assertEqual(target(self.profile, 'note', 0, 0x0B)['target'], 'deck1.play')
        self.assertEqual(target(self.profile, 'note', 1, 0x0B)['target'], 'deck2.play')
        self.assertEqual(target(self.profile, 'note', 0, 0x3F)['target'], 'shift')
        self.assertEqual(target(self.profile, 'controlChange', 0, 0x0F)['target'], 'deck1.eq.low')

    def test_a_second_use_under_shift_is_left_to_the_hand(self):
        self.assertIsNone(target(self.profile, 'controlChange', 0, 0x26))
        self.assertIn('DJ1.jogRingTickShift', self.report['leftOut'])
        self.assertIn('DJ1.lowKillButton', self.report['leftOut'])

    def test_encoders_read_their_steps(self):
        jog = target(self.profile, 'controlChange', 0, 0x21)
        self.assertEqual((jog['target'], jog['encoding']), ('deck1.jog', 'relativeOffset'))
        browse = target(self.profile, 'controlChange', 6, 0x40)
        self.assertEqual((browse['target'], browse['encoding']), ('library.browse', 'relative'))

    def test_14_bit_controls_take_their_low_half(self):
        self.assertEqual(target(self.profile, 'controlChange', 0, 0x00)['fine'], 0x20)
        self.assertEqual(target(self.profile, 'controlChange', 6, 0x1F)['fine'], 0x3F)
        self.assertIsNone(target(self.profile, 'controlChange', 0, 0x20))

    def test_pads_are_numbered(self):
        self.assertEqual(target(self.profile, 'note', 7, 0x00)['target'], 'deck1.hotcue1')
        self.assertEqual(target(self.profile, 'note', 7, 0x40)['target'], 'deck1.hotcue2')
        self.assertEqual(target(self.profile, 'note', 7, 0x0A)['target'], 'deck1.hotcue3.delete')

    def test_only_the_lights_a_control_does_not_give_are_listed(self):
        lights = {(l['control']['number'], l['target']): l for l in self.profile['lights']}
        self.assertNotIn((0x0B, 'deck1.play'), lights)
        self.assertIn((0x47, 'deck1.play'), lights)
        self.assertEqual((lights[(0x1A, 'deck1.keylock')]['on'], lights[(0x1A, 'deck1.keylock')]['off']), (0x40, 1))

    def test_the_report_counts_what_was_converted(self):
        self.assertEqual(self.report['controls'], 15)
        self.assertEqual(self.report['converted'], 13)

    def test_the_profile_is_written_as_json_a_control_a_line(self):
        text = write(self.profile)
        self.assertEqual(json.loads(text), self.profile)
        self.assertEqual(text.count('"target"'), len(self.profile['controls']) + len(self.profile['lights']))


@unittest.skipUnless(MIXXX.exists(), 'the Mixxx mappings are not on this computer')
class MixxxMappings(unittest.TestCase):
    def test_the_ddj_sb2(self):
        profile, report = convert(MIXXX / 'Pioneer-DDJ-SB2.midi.xml')
        self.assertEqual(target(profile, 'note', 0, 0x0B)['target'], 'deck1.play')
        self.assertEqual(target(profile, 'controlChange', 0, 0x13)['fine'], 0x33)
        self.assertEqual(target(profile, 'note', 8, 0x43)['target'], 'deck2.hotcue8')
        self.assertGreater(report['converted'] / report['controls'], 0.5)

    def test_every_mapping_converts_without_failing(self):
        for path in sorted(MIXXX.glob('*.midi.xml')):
            with self.subTest(path.name):
                profile, report = convert(path)
                json.loads(write(profile))
                self.assertLessEqual(report['converted'], report['controls'])


if __name__ == '__main__':
    unittest.main()
