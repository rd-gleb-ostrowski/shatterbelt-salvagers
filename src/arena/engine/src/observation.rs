use std::collections::HashMap;

use crate::types::*;

// ─── Self view (PROTOCOL §6 "self" field) ────────────────────────────────────

/// What the observing ship knows about itself (PROTOCOL.md §6 `self` block).
/// Full state including Aether and held Sigil.
#[derive(Debug, Clone, PartialEq)]
pub struct SelfView {
    pub id: ShipId,
    pub class: ShipClass,
    pub alive: bool,
    /// `true` while under spawn-protection or Bulwark immunity — shots do nothing.
    pub invuln: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub heading: f32,
    pub ang_vel: f32,
    pub hull: Resource,
    pub shield: Resource,
    pub aether: Resource,
    /// Held Sigil name, or `None`.
    pub sigil: Option<Sigil>,
    /// Ticks until the rune-cannon may fire again.
    pub cannon_cooldown: u32,
    pub relics_carried: u32,
    /// Remaining ticks of Afterburner boost, or 0 when inactive.
    pub afterburner_ticks_left: u32,
}

// ─── Other-ship view (PROTOCOL §6 "ships" array) ─────────────────────────────

/// What the observing ship sees of an *other* ship (PROTOCOL.md §6 `ships` array).
/// Aether and held Sigil are intentionally absent — bluff room for opponents.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherShipView {
    pub id: ShipId,
    pub class: ShipClass,
    pub alive: bool,
    pub invuln: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub heading: f32,
    pub hull: Resource,
    pub shield: Resource,
    pub relics_carried: u32,
    // NOTE: no `aether` or `sigil` — hidden per PROTOCOL §6
}

// ─── Full observation (PROTOCOL §6 top-level object) ─────────────────────────

/// The per-tick snapshot sent to a bot (PROTOCOL.md §6).
/// Produced by `Engine::observation(&ship_id)`.
#[derive(Debug, Clone)]
pub struct Observation {
    pub tick: u32,
    pub max_ticks: u32,
    pub seed: u64,
    pub arena: ArenaDims,
    /// The observing ship's own full state.
    pub self_view: SelfView,
    /// Every ship's Anchor (always fully visible, including enemies').
    pub anchors: Vec<AnchorView>,
    /// All *other* ships (self excluded); no aether/sigil.
    pub ships: Vec<OtherShipView>,
    pub relics: Vec<RelicView>,
    pub asteroids: Vec<AsteroidView>,
    pub projectiles: Vec<ProjectileView>,
    pub singularities: Vec<SingularityView>,
    pub mines: Vec<MineView>,
    /// Scores indexed by ShipId.
    pub scores: HashMap<ShipId, f32>,
    /// Events that happened to this ship since the previous tick (see PROTOCOL §7).
    pub events: Vec<Event>,
}

// ─── God-mode ship view ───────────────────────────────────────────────────────

/// Full per-ship state in the god-mode Viewer stream — everything, including
/// the fields that are hidden in per-bot Observations.
#[derive(Debug, Clone, PartialEq)]
pub struct GodShipView {
    pub id: ShipId,
    pub class: ShipClass,
    pub alive: bool,
    pub invuln: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub heading: f32,
    pub ang_vel: f32,
    pub hull: Resource,
    pub shield: Resource,
    pub aether: Resource,
    pub sigil: Option<Sigil>,
    pub cannon_cooldown: u32,
    pub relics_carried: u32,
    /// Remaining ticks of Afterburner boost, or 0 when inactive.
    pub afterburner_ticks_left: u32,
}

// ─── God-mode view (Viewer / recording) ──────────────────────────────────────

/// The full-world "god-mode" view produced by `Engine::god_view()`.
/// Sent to the Viewer for the projector and for replay recording.
/// Bots never receive this — they get `Observation` instead.
///
/// `PartialEq` is derived so the harness replay test can assert that two
/// independent runs from the same seed + intent-log produce identical views.
#[derive(Debug, Clone, PartialEq)]
pub struct GodView {
    pub tick: u32,
    pub max_ticks: u32,
    pub seed: u64,
    pub arena: ArenaDims,
    /// All ships with full state.
    pub ships: Vec<GodShipView>,
    pub anchors: Vec<AnchorView>,
    pub relics: Vec<RelicView>,
    pub asteroids: Vec<AsteroidView>,
    pub projectiles: Vec<ProjectileView>,
    pub singularities: Vec<SingularityView>,
    pub mines: Vec<MineView>,
    pub scores: HashMap<ShipId, f32>,
}
