//! # arena-server
//!
//! The live match-loop layer that wraps the [`arena_engine`] and drives matches
//! to completion.
//!
//! ## Architecture
//!
//! The central type is [`runner::MatchRunner`]:
//! - It owns the [`arena_engine::Engine`] (the pure deterministic stepper).
//! - It calls one [`runner::BotDriver`] per ship every tick to gather fresh
//!   intents. A driver returning `None` lets the engine carry the ship's previous
//!   intent forward (per-field persistence, `PROTOCOL.md §2`).
//! - It delegates wall-clock pacing to a [`runner::TickPacer`]; tests inject a
//!   no-op pacer so they run at CPU speed without sleeping.
//!
//! ## Seams for future issues
//!
//! | Seam | Interface | Future issue |
//! |------|-----------|-------------|
//! | Bot transport | [`runner::BotDriver`] trait | 03 (WS bots), 06 (resolver) |
//! | Tick pacing | [`runner::TickPacer`] trait | already used by [`pacer::LivePacer`] |
//! | Engine state | `runner.engine()` | 07 (observer god_view), 08 (intent_log) |
//! | Match outcome | [`runner::MatchOutcome`] | 02 (registration), 14 (ladder) |

pub mod bot;
pub mod pacer;
pub mod runner;
