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

/// Per-ship events emitted by `Engine::step` (PROTOCOL.md §7).
///
/// Populated by combat in issue 04; destruction events added in issue 05.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The ship's Shield absorbed `amount` damage from the rune-cannon of `by`.
    TookShield { amount: f32, by: ShipId },
    /// After the Shield was fully depleted, the Hull took `amount` damage from `by`.
    /// Emitted on the same hit as `TookShield` when there is overflow.
    TookHull { amount: f32, by: ShipId },
    /// The ship's Shield was reduced to exactly 0 this tick (useful for reactive bots).
    ShieldDown,
    /// This ship's Hull reached zero and it was destroyed.
    ///
    /// `by` is `Some(killer_id)` when a rune-cannon projectile delivered the lethal
    /// blow; `None` for environmental or self-inflicted death (collision, Singularity,
    /// etc.) — those damage sources arrive in issue 07+.
    Died { by: Option<ShipId> },
    /// This ship's rune-cannon round delivered the lethal blow to `victim`.
    ///
    /// Emitted to the killer on the same tick as the victim's `Died` event.
    /// The killer's score has already been updated with `params.kill_bounty` when
    /// this event is delivered.
    KilledShip { victim: ShipId },
    /// A relic that the ship was carrying was dropped into the Drift when the
    /// ship was destroyed.  Emitted to the destroyed ship (the carrier) for
    /// each relic dropped.
    RelicDropped { relic_id: String, pos: Vec2 },
    /// This ship has just respawned at its Anchor after the `respawn_delay`
    /// tick count elapsed since its destruction.
    Respawned,

    // ── Issue 07: Collision damage ─────────────────────────────────────────
    /// The ship's Shield absorbed `amount` damage from a collision (wall,
    /// asteroid, or ship-ram).  No `by` field — all collision damage is
    /// environmental (no kill bounty).
    CollisionTookShield { amount: f32 },
    /// After the Shield was fully depleted, the Hull took `amount` damage from
    /// a collision.  Emitted on the same hit as `CollisionTookShield` when
    /// there is overflow.
    CollisionTookHull { amount: f32 },

    // ── Issue 08: Sigil framework ──────────────────────────────────────────
    /// This ship picked up a Relic and was granted a random Sigil.
    /// Only emitted when the ship held no Sigil before the pickup.
    SigilGranted { which: Sigil },
    /// The held Sigil was consumed via a discharge Intent.
    /// Emitted on the same tick the Sigil disappears from the ship's state.
    SigilDischarged { which: Sigil },

    // ── Issue 09: Self-buff Sigil effects ─────────────────────────────────
    /// The Afterburner timed window has elapsed; thrust and speed cap revert.
    /// Emitted at the end of the last boosted tick.
    AfterburnerExpired,
    /// The Bulwark damage-immunity window has elapsed; `invuln` is now false.
    /// Emitted at the end of the last protected tick.
    BulwarkExpired,

    // ── Issue 10: World-effect Sigil events ───────────────────────────────
    /// A Singularity gravity well was deployed (emitted to the discharging ship).
    SingularityDeployed { id: String, pos: Vec2 },
    /// An Aether Mine was dropped at `pos` (emitted to the mine owner).
    MineDeployed { id: String, pos: Vec2 },
    /// An Aether Mine detonated; emitted to every ship that took the blast.
    MineDetonated { mine_id: String, pos: Vec2 },
    /// The ship's Hull took `amount` direct damage from an Arc Lance bolt fired
    /// by `by`. Shields are bypassed entirely.
    LanceTookHull { amount: f32, by: ShipId },
}
