//! The motor of a turntable, as internal play has it: switched off, the record slows to a
//! stop (a brake); switched on from a stop, it gets up to speed (a soft start); spun back by
//! a hand, it runs backwards fast while held, and the motor takes it up again once let go.

/// How fast the record's speed changes (in times the deck's speed, a second): stopped in a
/// second by the brake, up to speed in half a second by a soft start, sent backwards at
/// [`SPIN_SPEED`] in about 0.15 s by the hand, taken up again in a third of a second.
const BRAKE: f64 = 1.0;
const START: f64 = 2.0;
const SPIN: f64 = 25.0;
const CATCH: f64 = 12.0;
/// The speed of a record spun back, in times the deck's speed.
const SPIN_SPEED: f64 = -3.0;

/// The record's speed as a factor of the deck's, gliding towards a target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Motor {
    factor: f64,
    target: f64,
    /// Change of the factor a second.
    rate: f64,
}

impl Default for Motor {
    fn default() -> Self {
        Self { factor: 1.0, target: 1.0, rate: 0.0 }
    }
}

impl Motor {
    /// The record turns at the deck's speed, as it does unless a gesture moves it.
    pub(crate) fn is_steady(&self) -> bool {
        self.factor == 1.0 && self.target == 1.0
    }

    /// The motor switched off is slowing the record down, or has stopped it.
    pub(crate) fn is_braking(&self) -> bool {
        self.target == 0.0
    }

    /// Braked to a stop: the deck no longer plays.
    pub(crate) fn has_stopped(&self) -> bool {
        self.factor == 0.0 && self.target == 0.0
    }

    pub(crate) fn brake(&mut self) {
        self.glide(0.0, BRAKE);
    }

    /// Up to speed from where the record is: from a stop, or braking.
    pub(crate) fn start(&mut self, from_stop: bool) {
        if from_stop {
            self.factor = 0.0;
        }
        self.glide(1.0, START);
    }

    /// The record spun back while `held` (from a stop or as it turns), taken up again by the
    /// motor once let go.
    pub(crate) fn spin_back(&mut self, held: bool, from_stop: bool) {
        if held {
            if from_stop {
                self.factor = 0.0;
            }
            self.glide(SPIN_SPEED, SPIN);
        } else if self.target == SPIN_SPEED {
            self.glide(1.0, CATCH);
        }
    }

    fn glide(&mut self, target: f64, rate: f64) {
        self.target = target;
        self.rate = rate;
    }

    /// The factor for the next frame, `seconds` long.
    #[inline]
    pub(crate) fn tick(&mut self, seconds: f64) -> f64 {
        let step = self.rate * seconds;
        self.factor = if self.factor < self.target {
            (self.factor + step).min(self.target)
        } else {
            (self.factor - step).max(self.target)
        };
        self.factor
    }

    pub(crate) fn factor(&self) -> f64 {
        self.factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_S: f64 = 1.0 / 48_000.0;

    fn run(motor: &mut Motor, seconds: f64) -> f64 {
        let frames = (seconds / FRAME_S).round() as usize;
        (0..frames).map(|_| motor.tick(FRAME_S)).last().unwrap_or(motor.factor())
    }

    #[test]
    fn a_brake_stops_the_record_in_a_second_at_an_even_pace() {
        let mut motor = Motor::default();
        motor.brake();
        assert!((run(&mut motor, 0.5) - 0.5).abs() < 1e-6);
        assert!(!motor.has_stopped());
        run(&mut motor, 0.5);
        assert!(motor.has_stopped());
    }

    #[test]
    fn a_soft_start_gets_up_to_speed_in_half_a_second() {
        let mut motor = Motor::default();
        motor.start(true);
        assert!((run(&mut motor, 0.25) - 0.5).abs() < 1e-6);
        run(&mut motor, 0.25);
        assert!(motor.is_steady());
    }

    #[test]
    fn a_record_spun_back_runs_backwards_while_held_then_gets_up_to_speed() {
        let mut motor = Motor::default();
        motor.spin_back(true, false);
        assert_eq!(run(&mut motor, 1.0), SPIN_SPEED);
        motor.spin_back(false, false);
        run(&mut motor, 0.2);
        assert!(motor.factor() > -1.0 && motor.factor() < 1.0, "{}", motor.factor());
        run(&mut motor, 0.2);
        assert!(motor.is_steady());
    }

    #[test]
    fn letting_go_of_nothing_changes_nothing() {
        let mut motor = Motor::default();
        motor.brake();
        motor.spin_back(false, false);
        assert!(motor.is_braking());
    }
}
