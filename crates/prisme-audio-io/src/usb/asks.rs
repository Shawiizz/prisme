//! Whether Prisme may open the cards: a phone's system asks the user first, once for each time
//! a card is plugged in, and answers later. Prisme asks once and waits for the answer, however
//! long it takes, rather than ask again over a question still on screen (the system would show
//! one more); a refusal holds until the card is plugged in again or the user chooses a card.

use super::{UsbCard, UsbHost};
use crate::AudioError;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A card Prisme asked the user to let it open: the answer awaited, or a refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The card's name, as the user is shown it.
    pub card: String,
    pub refused: bool,
}

/// The user answered the ask for card `id` (the system's name of it), or let Prisme open it
/// another way (the system's offer of an application for a card plugged in).
pub fn answered(id: &str, allowed: bool) {
    held(&ASKS).answered(id, allowed);
    ANSWERS.came();
}

/// Card `id` was unplugged: what it was asked is forgotten.
pub fn unplugged(id: &str) {
    held(&ASKS).forget(id);
    ANSWERS.came();
}

/// The user chose the cards again: those refused are asked again, as those whose question may
/// have gone unseen.
pub fn ask_again() {
    held(&ASKS).0.clear();
}

/// The ask to show: one awaiting its answer, else a refusal.
pub fn asked() -> Option<Asked> {
    held(&ASKS).shown()
}

/// How many answers came so far, to wait for the next ([`wait_for_answer`]).
pub fn answers() -> u64 {
    ANSWERS.count()
}

/// Waits `timeout` at most for an answer after the `seen` first ones; how many came by then.
pub fn wait_for_answer(seen: u64, timeout: Duration) -> u64 {
    ANSWERS.wait(seen, timeout)
}

/// Whether card `card` may be opened, among the cards `plugged` in: asked to the user the first
/// time, the answer awaited after.
pub(crate) fn may_open(host: &dyn UsbHost, card: &UsbCard, plugged: &[UsbCard]) -> Result<(), AudioError> {
    let step = held(&ASKS).step(card, host.allowed(&card.id), plugged);
    match step {
        Step::Open => Ok(()),
        Step::Ask => {
            host.ask(&card.id);
            Err(AudioError::UsbAsked(card.name.clone()))
        }
        Step::Wait => Err(AudioError::UsbAsked(card.name.clone())),
        Step::Refused => Err(AudioError::UsbRefused(card.name.clone())),
    }
}

static ASKS: Mutex<Asks> = Mutex::new(Asks(Vec::new()));
static ANSWERS: Answers = Answers::new();

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A card asked for, by the system's name of it.
#[derive(Debug)]
struct Ask {
    id: String,
    card: String,
    refused: bool,
}

/// The cards asked for and not let open yet.
#[derive(Debug, Default)]
struct Asks(Vec<Ask>);

/// What opening a card takes.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Open,
    Ask,
    Wait,
    Refused,
}

impl Asks {
    /// What opening `card` takes, the system letting Prisme open it or not (`allowed`): the
    /// asks of the cards no longer `plugged` in are forgotten.
    fn step(&mut self, card: &UsbCard, allowed: bool, plugged: &[UsbCard]) -> Step {
        self.0.retain(|ask| plugged.iter().any(|c| c.id == ask.id));
        if allowed {
            self.forget(&card.id);
            return Step::Open;
        }
        match self.0.iter().find(|ask| ask.id == card.id) {
            Some(ask) if ask.refused => Step::Refused,
            Some(_) => Step::Wait,
            None => {
                self.0.push(Ask { id: card.id.clone(), card: card.name.clone(), refused: false });
                Step::Ask
            }
        }
    }

    /// A yes forgets the card's ask, whatever it was; a no refuses the one awaited only (any
    /// other question of the system answers no as it closes: one of a card unplugged).
    fn answered(&mut self, id: &str, allowed: bool) {
        if allowed {
            self.forget(id);
        } else if let Some(ask) = self.0.iter_mut().find(|ask| ask.id == id) {
            ask.refused = true;
        }
    }

    fn forget(&mut self, id: &str) {
        self.0.retain(|ask| ask.id != id);
    }

    fn shown(&self) -> Option<Asked> {
        let ask = self.0.iter().find(|ask| !ask.refused).or_else(|| self.0.first())?;
        Some(Asked { card: ask.card.clone(), refused: ask.refused })
    }
}

/// The answers counted as they come, waited for.
struct Answers {
    count: Mutex<u64>,
    came: Condvar,
}

impl Answers {
    const fn new() -> Self {
        Self { count: Mutex::new(0), came: Condvar::new() }
    }

    fn came(&self) {
        *held(&self.count) += 1;
        self.came.notify_all();
    }

    fn count(&self) -> u64 {
        *held(&self.count)
    }

    fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let deadline = Instant::now() + timeout;
        let mut count = held(&self.count);
        while *count == seen {
            let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else {
                break;
            };
            count = self.came.wait_timeout(count, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        *count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice::from_ref;
    use std::sync::Arc;

    fn card(id: &str) -> UsbCard {
        UsbCard { id: id.into(), name: format!("UMC404HD {id}") }
    }

    #[test]
    fn a_card_is_asked_for_once_and_its_answer_awaited() {
        let mut asks = Asks::default();
        let umc = card("3");
        let plugged = [umc.clone()];
        assert_eq!(asks.step(&umc, false, &plugged), Step::Ask);
        assert_eq!(asks.step(&umc, false, &plugged), Step::Wait, "no second question over the first");
        assert_eq!(asks.shown(), Some(Asked { card: "UMC404HD 3".into(), refused: false }));
        asks.answered("3", true);
        assert_eq!(asks.shown(), None);
        assert_eq!(asks.step(&umc, true, &plugged), Step::Open);
    }

    #[test]
    fn a_card_let_open_another_way_opens_without_its_answer() {
        let mut asks = Asks::default();
        let umc = card("3");
        asks.step(&umc, false, from_ref(&umc));
        assert_eq!(asks.step(&umc, true, from_ref(&umc)), Step::Open);
        assert_eq!(asks.shown(), None);
    }

    #[test]
    fn a_refusal_holds_until_the_card_is_plugged_in_again_or_asked_again() {
        let mut asks = Asks::default();
        let umc = card("3");
        asks.step(&umc, false, from_ref(&umc));
        asks.answered("3", false);
        assert_eq!(asks.step(&umc, false, from_ref(&umc)), Step::Refused);
        assert_eq!(asks.shown(), Some(Asked { card: "UMC404HD 3".into(), refused: true }));

        // plugged in again, the system names it anew
        let again = card("4");
        assert_eq!(asks.step(&again, false, from_ref(&again)), Step::Ask);
        assert_eq!(asks.0.len(), 1, "the card unplugged is forgotten");

        asks.answered("4", false);
        asks.0.clear();
        assert_eq!(asks.step(&again, false, from_ref(&again)), Step::Ask);
    }

    #[test]
    fn only_the_question_awaited_is_refused() {
        let mut asks = Asks::default();
        asks.answered("3", false);
        assert_eq!(asks.shown(), None, "the question of a card never asked for");
        let umc = card("3");
        asks.step(&umc, false, from_ref(&umc));
        asks.forget("3");
        asks.answered("3", false);
        assert_eq!(asks.shown(), None, "the question of a card unplugged");
    }

    #[test]
    fn the_ask_awaited_is_shown_before_a_refusal() {
        let mut asks = Asks::default();
        let (first, second) = (card("3"), card("5"));
        let plugged = [first.clone(), second.clone()];
        asks.step(&first, false, &plugged);
        asks.answered("3", false);
        asks.step(&second, false, &plugged);
        assert_eq!(asks.shown(), Some(Asked { card: "UMC404HD 5".into(), refused: false }));
    }

    #[test]
    fn a_wait_ends_with_the_next_answer() {
        let answers = Arc::new(Answers::new());
        let seen = answers.count();
        assert_eq!(answers.wait(seen, Duration::from_millis(10)), seen, "none came");
        let answering = answers.clone();
        let answer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            answering.came();
        });
        let started = Instant::now();
        assert_eq!(answers.wait(seen, Duration::from_secs(10)), seen + 1);
        assert!(started.elapsed() < Duration::from_secs(5));
        answer.join().unwrap();
        assert_eq!(answers.wait(seen, Duration::from_secs(10)), seen + 1, "one came since");
    }

    struct Phone {
        allowed: bool,
        asked: Mutex<Vec<String>>,
    }

    impl UsbHost for Phone {
        fn cards(&self) -> Vec<UsbCard> {
            Vec::new()
        }
        fn allowed(&self, _: &str) -> bool {
            self.allowed
        }
        fn ask(&self, id: &str) {
            held(&self.asked).push(id.into());
        }
        fn open(&self, _: &str) -> Option<crate::OpenedCard> {
            None
        }
        fn close(&self, _: &str) {}
    }

    #[test]
    fn the_system_asks_the_user_once_until_answered() {
        let phone = Phone { allowed: false, asked: Mutex::default() };
        // a card of its own: the asks are the whole crate's
        let umc = UsbCard { id: "/dev/bus/usb/009/001".into(), name: "UMC404HD 192k".into() };
        let plugged = [umc.clone()];
        let asked =
            || matches!(may_open(&phone, &umc, &plugged), Err(AudioError::UsbAsked(c)) if c == umc.name);
        assert!(asked());
        assert!(asked());
        assert_eq!(held(&phone.asked).as_slice(), from_ref(&umc.id));
        answered(&umc.id, false);
        assert!(matches!(may_open(&phone, &umc, &plugged), Err(AudioError::UsbRefused(_))));
        assert_eq!(held(&phone.asked).len(), 1);
        unplugged(&umc.id);
        assert!(asked());
        assert_eq!(held(&phone.asked).len(), 2);
        let allowed = Phone { allowed: true, asked: Mutex::default() };
        assert!(may_open(&allowed, &umc, &plugged).is_ok());
        assert!(held(&allowed.asked).is_empty());
    }
}
