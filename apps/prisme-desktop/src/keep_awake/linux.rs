//! Linux: an inhibitor held with the login manager (`systemd-inhibit --list` shows it).

use std::process::{Child, Command, Stdio};

/// An inhibitor of idling and sleeping, held by a `systemd-inhibit` process for as long as
/// the command it runs reads its input: until dropped, or until Prisme is gone.
pub struct KeepAwake(Child);

impl KeepAwake {
    /// `None` without systemd's login manager: the computer may then sleep as usual.
    pub fn new() -> Option<Self> {
        Command::new("systemd-inhibit")
            .args(["--what=idle:sleep", "--who=Prisme", "--why=Prisme is playing", "--mode=block"])
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .map(Self)
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        // its input closed, the command ends, and the inhibitor with it
        drop(self.0.stdin.take());
        let _ = self.0.wait();
    }
}

/// Linux does not slow down an application whose window is behind others: nothing to opt
/// out of.
pub fn run_at_full_speed() -> bool {
    true
}
