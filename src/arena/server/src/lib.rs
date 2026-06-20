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
//! | Registration & tokens | [`auth::TokenRegistry`] + [`routes::build_router`] | 02 (this issue) |
//! | Bot transport | [`runner::BotDriver`] trait | 03 (WS bots), 06 (resolver) |
//! | Connection resolution | [`resolver::ConnectionResolver`] + [`resolver::WsConnectionRegistry`] | 06 (this issue), 09 (headless), 11 (admin) |
//! | Headless-fast runner | [`headless::HeadlessRunner`] + [`headless::HeadlessResult`] | 09 (this issue), 10 (ladder), 11 (admin) |
//! | Tick pacing | [`runner::TickPacer`] trait | already used by [`pacer::LivePacer`] |
//! | Engine state | `runner.engine()` | 07 (observer god_view), 08 (intent_log) |
//! | Observer stream | [`observer::ObserverHub`] | 07 (this issue), 08 (recording tap), 11 (admin projector) |
//! | Match outcome | [`runner::MatchOutcome`] | 14 (ladder) |
//! | WASM artifact storage | [`store::WasmBotStore`] | 04 (this issue), 05 (wasmtime host), 06 (resolver), 11 (admin) |

pub mod auth;
pub mod bot;
pub mod headless;
pub mod ladder;
pub mod observer;
pub mod pacer;
pub mod recording;
pub mod replay;
pub mod resolver;
pub mod routes;
pub mod runner;
pub mod store;
pub mod wasm_host;
pub mod ws;
