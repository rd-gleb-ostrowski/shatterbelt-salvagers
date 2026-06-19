//! Live tick pacing implementations for the match-runner loop.
//!
//! The [`TickPacer`](crate::runner::TickPacer) trait is defined in [`crate::runner`];
//! this module provides the two concrete pacers:
//!
//! - [`LivePacer`] — sleeps to maintain ~30 Hz (≈33 ms/tick minus step time).
//! - [`NoopPacer`] — instant no-op; used in tests and headless-fast mode.

use std::time::{Duration, Instant};

use crate::runner::TickPacer;

// ── NoopPacer ─────────────────────────────────────────────────────────────────

/// A [`TickPacer`] that does nothing — ticks run at CPU speed.
///
/// Use in tests and headless-fast (ladder) mode so the runner never sleeps.
pub struct NoopPacer;

impl TickPacer for NoopPacer {
    fn wait_for_next_tick(&mut self) {}
}

// ── LivePacer ─────────────────────────────────────────────────────────────────

/// A [`TickPacer`] that sleeps between ticks to maintain ~30 Hz.
///
/// Each call to [`wait_for_next_tick`](LivePacer::wait_for_next_tick) sleeps
/// until `tick_start + tick_duration`, where `tick_start` is recorded by
/// [`begin_tick`](LivePacer::begin_tick) at the top of the match loop. This
/// accounts for step-execution time so the rate is stable even when individual
/// ticks are slow.
pub struct LivePacer {
    tick_duration: Duration,
    tick_start: Instant,
}

impl LivePacer {
    /// Create a pacer for `ticks_per_second` (typically 30).
    pub fn new(ticks_per_second: u32) -> Self {
        let tick_duration = Duration::from_secs(1) / ticks_per_second;
        Self {
            tick_duration,
            tick_start: Instant::now(),
        }
    }

    /// Record the start of this tick's work.
    ///
    /// Call once at the top of the match loop, before `step_once`. The
    /// subsequent `wait_for_next_tick` will sleep until `tick_start + duration`.
    pub fn begin_tick(&mut self) {
        self.tick_start = Instant::now();
    }
}

impl TickPacer for LivePacer {
    /// Sleep until the next tick deadline.
    ///
    /// If the step took longer than `tick_duration` the sleep is skipped —
    /// the server runs behind but never stalls.
    fn wait_for_next_tick(&mut self) {
        let elapsed = self.tick_start.elapsed();
        if elapsed < self.tick_duration {
            std::thread::sleep(self.tick_duration - elapsed);
        }
    }
}
