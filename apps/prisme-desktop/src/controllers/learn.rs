//! A control learnt by moving it.

use super::{Controllers, Learning, Unmapped, FINE_HALF_WITHIN};
use crate::state::lock;
use prisme_control::hid::Learnt;
use prisme_control::{Control, Encoding, Event, Kind, Mapping, Modes, ProfileControl, Shifts, Target, SHIFT};
use std::time::Instant;

impl Controllers {
    pub fn learn(&self, target: String) {
        let learnt = match target.parse::<Target>().map(|t| t.kind()) {
            Ok(Kind::Encoder) => Learnt::Encoder,
            Ok(Kind::Fader) => Learnt::Fader,
            _ => Learnt::Button,
        };
        *lock(&self.learning) = Some(Learning { target, learnt });
    }

    pub fn cancel_learning(&self) {
        *lock(&self.learning) = None;
    }

    /// Learns the control of `event` of `device` for the action waiting for one, if one is: but
    /// for a key let go, the end of a press made before, and shift.
    pub(super) fn learn_from(&self, device: &str, event: Event) -> Option<Mapping> {
        if lock(&self.learning).is_none() {
            return None;
        }
        let reader = lock(&self.reader);
        let shift = lock(&self.mappings)
            .find(device, Event { raw: 127, ..event }, &Shifts::default(), &Modes::new())
            .is_some_and(|m| m.target == SHIFT);
        let released = event.control.is_key() && event.raw == 0;
        let Learning { target, learnt } = lock(&self.learning).take_if(|_| !released && !shift)?;
        let binding = ProfileControl {
            control: event.control,
            target: target.into(),
            encoding: if learnt == Learnt::Encoder { Encoding::guess(event) } else { Encoding::Absolute },
            shift: reader.shifted(device).into(),
            ..Default::default()
        };
        drop(reader);
        let mapping = Mapping::new(device, binding);
        self.save(|s| {
            let unmapped = Unmapped::On { device: device.to_owned(), target: mapping.target.to_string() };
            s.unmapped.retain(|u| *u != unmapped);
            s.mappings.learn(mapping.clone());
        });
        *lock(&self.just_learnt) = Some((mapping.clone(), Instant::now()));
        Some(mapping)
    }

    /// A fader just learnt whose low half follows on the controller numbered 32 higher is
    /// a 14-bit one: it is learnt as such.
    pub(super) fn complete_fine(&self, device: &str, event: Event) -> Option<Mapping> {
        let mut just_learnt = lock(&self.just_learnt);
        let (mapping, at) =
            just_learnt.take_if(|(m, at)| m.device == device && at.elapsed() < FINE_HALF_WITHIN)?;
        let fine = match mapping.control {
            Control::ControlChange(coarse) if coarse.number < 32 => Some(coarse.number + 32),
            _ => None,
        };
        let follows = fine.is_some_and(|fine| mapping.control.with_number(fine) == Some(event.control))
            && mapping.encoding == Encoding::Absolute;
        let Some(fine) = fine.filter(|_| follows) else {
            *just_learnt = Some((mapping, at));
            return None;
        };
        self.save(|s| s.mappings.make_fine(&mapping, fine));
        let mut fine_mapping = mapping;
        fine_mapping.fine = Some(fine);
        Some(fine_mapping)
    }
}
