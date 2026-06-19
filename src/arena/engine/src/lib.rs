//! # arena-engine
//!
//! The deterministic, headless heart of a *Shatterbelt Salvagers* Match.
//!
//! Constructed from `(seed, params, ships)`, advanced via `step(intents) -> events`,
//! and queried via `observation` / `god_view`.  Zero dependencies on networking,
//! WASM, HTTP, the Ladder, or auth.
//!
//! ## Vocabulary
//!
//! Type and method names follow `src/arena/CONTEXT.md` — Ship, Hull, Shield,
//! Aether, Relic, Anchor, Drift, Sigil, Match, Tick, Observation, Intent, etc.

mod engine;
pub mod harness;
mod intent;
mod observation;
mod params;
mod types;

pub use engine::{scale_drift, Engine, IntentFrame};
pub use intent::Intent;
pub use observation::{GodShipView, GodView, Observation, OtherShipView, SelfView};
pub use params::Params;
pub use types::{
    AnchorView, ArenaDims, AsteroidView, Event, MineView, ProjectileView, RelicView, Resource,
    ShipClass, ShipId, ShipSpec, Sigil, SingularityView, Vec2,
};
