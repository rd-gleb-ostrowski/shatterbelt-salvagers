//! Replay runner (issue 08).
//!
//! [`run_replay`] reconstructs a finished match from a [`Recording`] via
//! [`arena_engine::harness::replay_match`] and publishes every tick's
//! god-view frame through an [`ObserverHub`].
//!
//! ## Timing
//!
//! Timing is injectable via the [`TickPacer`](crate::runner::TickPacer) trait:
//! - Use [`NoopPacer`](crate::pacer::NoopPacer) in tests for instant replay
//!   (no sleeping).
//! - Use [`LivePacer`](crate::pacer::LivePacer) for real-time 30 Hz replay
//!   playback in production.
//!
//! ## Replay parity
//!
//! [`arena_engine::harness::replay_match`] drives the engine with exactly the
//! recorded `intent_log`, so the reconstructed [`MatchResult`] is
//! **guaranteed** to have an identical `final_god_view`, `scores`, and
//! `winner` to the original run.
//!
//! ## Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 11 (Admin projector) | call `run_replay` with a `LivePacer` from an admin endpoint |

use arena_engine::harness::{MatchResult, replay_match};
use arena_engine::Engine;

use crate::observer::{GodViewFrameJson, ObserverHub, god_view_to_json};
use crate::recording::Recording;
use crate::runner::TickPacer;

// ── run_replay ────────────────────────────────────────────────────────────────

/// Replay a recorded match through the observer layer.
///
/// Reconstructs the engine from `recording.seed`, `recording.params`, and
/// `recording.specs`, then replays every tick from `recording.intent_log`
/// in order, publishing a [`"godView"` frame](crate::observer::god_view_to_json)
/// to `hub` after each tick.
///
/// `pacer` is called between each tick so the replay can be slowed to real
/// time or run instantly (inject [`NoopPacer`](crate::pacer::NoopPacer) in
/// tests).
///
/// Returns the [`MatchResult`] of the replayed match.  Its `final_god_view`,
/// `scores`, and `winner` are identical to the original run (replay parity).
///
/// # Usage in tests
///
/// ```rust,ignore
/// use arena_server::pacer::NoopPacer;
/// use arena_server::replay::run_replay;
/// use arena_server::observer::ObserverHub;
///
/// let hub = ObserverHub::new();
/// let result = run_replay(&recording, &hub, Box::new(NoopPacer));
/// assert_eq!(result.winner, recording.meta.winner);
/// ```
pub fn run_replay(
    recording: &Recording,
    hub: &ObserverHub,
    mut pacer: Box<dyn TickPacer>,
) -> MatchResult {
    // Reconstruct the engine and step tick-by-tick so we can publish
    // each intermediate god-view frame to the hub before moving on.
    let mut engine = Engine::new(
        recording.seed,
        recording.params.clone(),
        recording.specs.clone(),
    );

    for frame in &recording.intent_log {
        let events = engine.step(frame.clone());
        hub.publish_god_view(&engine.god_view(), &events);
        pacer.wait_for_next_tick();
    }

    // Return a full MatchResult consistent with harness::replay_match.
    // We drive the engine ourselves above so we can interleave hub publishes;
    // a second call to replay_match would rebuild the same result cheaply.
    replay_match(
        recording.params.clone(),
        recording.specs.clone(),
        recording.seed,
        &recording.intent_log,
    )
}

pub fn collect_replay_god_frames(
    recording: &Recording,
    mut pacer: Box<dyn TickPacer>,
) -> Vec<GodViewFrameJson> {
    // Reconstruct the engine and step tick-by-tick so we can publish
    // each intermediate god-view frame to the hub before moving on.
    let mut engine = Engine::new(
        recording.seed,
        recording.params.clone(),
        recording.specs.clone(),
    );
    let mut god_frames = Vec::with_capacity(3600);

    for frame in &recording.intent_log {
        let events = engine.step(frame.clone());
        god_frames.push(god_view_to_json(&engine.god_view(), &events));
        pacer.wait_for_next_tick();
    }
    god_frames
}
