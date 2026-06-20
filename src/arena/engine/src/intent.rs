use crate::types::Vec2;

/// The per-tick command a bot returns to the Arena (PROTOCOL.md §8).
///
/// Rate-first, inspired by Robocode Tank Royale.  **All fields are optional**:
/// an omitted field (`None`) keeps its previously applied value in the engine.
/// The Arena applies physics clamps; bots never set absolute state.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Intent {
    /// Turn rate fraction in −1..1 (positive = counter-clockwise).
    pub turn: Option<f32>,
    /// Thrust fraction in −1..1 (positive = forward along heading, negative = reverse).
    pub thrust: Option<f32>,
    /// Fire the rune-cannon continuously while `true`; persists across ticks.
    pub fire: Option<bool>,
    /// Discharge the held Sigil once (`true` triggers, then the Sigil is consumed).
    pub sigil: Option<bool>,
    /// Aim point for Sigils that need a target (Singularity, Arc Lance).
    pub sigil_target: Option<Vec2>,
}
