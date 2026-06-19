/// Primitive value types shared across the engine.
///
/// Vocabulary is taken verbatim from src/arena/CONTEXT.md — Ship, Hull, Shield,
/// Aether, Relic, Anchor, Drift, Sigil, etc.
pub type ShipId = String;

/// 2-D position or velocity in arena units (origin top-left, x right, y down).
/// Angles are radians; 0 = +x (East), increasing counter-clockwise (PROTOCOL §3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// A `{cur, max}` resource pair used for Hull, Shield, and Aether.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resource {
    pub cur: f32,
    pub max: f32,
}

impl Resource {
    pub fn full(max: f32) -> Self {
        Self { cur: max, max }
    }
}

/// The five Sigils a Ship can carry at most one of.
/// Granted when picking up a Relic; discharged with `Intent::sigil = true`.
#[derive(Debug, Clone, PartialEq)]
pub enum Sigil {
    Afterburner,
    Bulwark,
    Singularity,
    AetherMine,
    ArcLance,
}

/// Ship archetypes. `Skiff` is the only class in v1.
#[derive(Debug, Clone, PartialEq)]
pub enum ShipClass {
    Skiff,
}

/// Caller-supplied descriptor for a Ship entering a Match.
#[derive(Debug, Clone)]
pub struct ShipSpec {
    pub id: ShipId,
    pub class: ShipClass,
    /// The Anchor position — where this ship banks Relics and respawns.
    pub anchor_pos: Vec2,
}

/// Drift (playfield) dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct ArenaDims {
    pub width: f32,
    pub height: f32,
}

/// An Anchor's public view: which ship owns it and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorView {
    pub ship_id: ShipId,
    pub pos: Vec2,
}

/// A Relic lying in the Drift.
#[derive(Debug, Clone, PartialEq)]
pub struct RelicView {
    pub id: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub value: f32,
}

/// An Asteroid — collision hazard and cover.
#[derive(Debug, Clone, PartialEq)]
pub struct AsteroidView {
    pub id: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
}

/// A rune-cannon projectile in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileView {
    pub id: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub owner: ShipId,
}

/// A Singularity Sigil's deployed gravity well.
#[derive(Debug, Clone, PartialEq)]
pub struct SingularityView {
    pub id: String,
    pub pos: Vec2,
    pub radius: f32,
    pub ticks_left: u32,
}

/// An Aether Mine in the Drift.
/// `own = true` means this mine belongs to the observing ship; `own = false`
/// means it is a visible enemy mine (within detection radius).
#[derive(Debug, Clone, PartialEq)]
pub struct MineView {
    pub id: String,
    pub pos: Vec2,
    pub own: bool,
}

/// Per-ship events emitted by `Engine::step`.
///
/// Only lifecycle/no-op events exist in issue 01.  Combat and economy events
/// (tookHull, relicBanked, died, etc.) are added in later issues.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {}
