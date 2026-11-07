//! Cooperative cancellation token for a running job attempt.
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    None,
    /// User asked to pause: finish the current unit, save a checkpoint, stay paused.
    Pause,
    /// User asked to cancel the attempt.
    Cancel,
    /// The app is closing: save a checkpoint and resume automatically next launch.
    Interrupt,
}

#[derive(Debug, Default)]
pub struct JobControl {
    signal: AtomicU8,
}

impl JobControl {
    pub fn new() -> Self {
        Self::default()
    }
    fn set(&self, s: Signal) {
        let v = match s {
            Signal::None => 0,
            Signal::Pause => 1,
            Signal::Cancel => 2,
            Signal::Interrupt => 3,
        };
        // Cancel wins over pause/interrupt; never downgrade a cancel.
        let _ = self.signal.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
            if cur == 2 && v != 2 { None } else { Some(v) }
        });
    }
    pub fn request_pause(&self) {
        self.set(Signal::Pause)
    }
    pub fn request_cancel(&self) {
        self.set(Signal::Cancel)
    }
    pub fn request_interrupt(&self) {
        self.set(Signal::Interrupt)
    }
    pub fn signal(&self) -> Signal {
        match self.signal.load(Ordering::SeqCst) {
            1 => Signal::Pause,
            2 => Signal::Cancel,
            3 => Signal::Interrupt,
            _ => Signal::None,
        }
    }
    pub fn stop_requested(&self) -> bool {
        self.signal() != Signal::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_sticky() {
        let c = JobControl::new();
        assert_eq!(c.signal(), Signal::None);
        c.request_pause();
        assert_eq!(c.signal(), Signal::Pause);
        c.request_cancel();
        c.request_pause();
        assert_eq!(c.signal(), Signal::Cancel);
    }
}
