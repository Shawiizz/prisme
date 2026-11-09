//! Soft takeover: a fader or knob whose position no longer matches its setting (moved
//! from the screen, or another layer) takes it over only once it reaches it, instead of
//! making it jump.

/// Positions closer than this are the same (on a 0-1 scale).
const NEAR: f32 = 0.02;

#[derive(Debug, Default, Clone)]
pub struct Pickup {
    /// The control's previous position, to see it cross the setting.
    previous: Option<f32>,
    /// The setting the control last gave, to notice it changed elsewhere.
    given: Option<f32>,
}

impl Pickup {
    /// Whether the control at `position` drives a setting now at `current` (both 0-1).
    pub fn takes(&mut self, position: f32, current: f32) -> bool {
        let held = self.given.is_some_and(|g| (g - current).abs() <= NEAR / 4.0);
        let crossed = self.previous.is_some_and(|p| (p - current) * (position - current) <= 0.0);
        let takes = held || crossed || (position - current).abs() <= NEAR;
        self.previous = Some(position);
        self.given = takes.then_some(position);
        takes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_away_from_the_setting_waits_until_it_reaches_it() {
        let mut pickup = Pickup::default();
        assert!(!pickup.takes(0.2, 0.5));
        assert!(!pickup.takes(0.4, 0.5));
        // crossed between two messages
        assert!(pickup.takes(0.6, 0.5));
        assert!(pickup.takes(0.7, 0.6));
    }

    #[test]
    fn a_control_on_the_setting_takes_it_at_once() {
        let mut pickup = Pickup::default();
        assert!(pickup.takes(0.51, 0.5));
    }

    #[test]
    fn a_setting_changed_elsewhere_must_be_picked_up_again() {
        let mut pickup = Pickup::default();
        assert!(pickup.takes(0.5, 0.5));
        assert!(pickup.takes(0.6, 0.5));
        // the screen moved it to 0.9 meanwhile
        assert!(!pickup.takes(0.62, 0.9));
        assert!(pickup.takes(0.95, 0.9));
    }
}
