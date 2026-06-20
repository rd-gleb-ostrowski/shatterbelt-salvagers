//! Shared event serialisation — used by both per-bot `TickMsg.events` (ws.rs)
//! and the god-view `GodViewFrameJson.events` (observer.rs).
//!
//! [`EventJson`] is the PROTOCOL §7 wire type; [`event_to_json`] converts an
//! engine [`arena_engine::Event`] to it.  Returns `None` for any variant not
//! yet mapped (currently none — all variants are covered).

use serde::Serialize;

use crate::ws::vec2_to_json;

/// PROTOCOL §7 event — serialised with an `"event"` discriminant field.
///
/// Variant names become camelCase in JSON via `rename_all = "camelCase"`.
/// E.g. `TookShield` → `"tookShield"`, `ShieldDown` → `"shieldDown"`.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum EventJson {
    TookShield { amount: f32, by: String },
    TookHull { amount: f32, by: String },
    ShieldDown,
    LanceTookHull { amount: f32, by: String },
    CollisionTookShield { amount: f32 },
    CollisionTookHull { amount: f32 },
    RelicDropped { relic_id: String, pos: crate::ws::Vec2Json },
    SigilGranted { which: String },
    SigilDischarged { which: String },
    AfterburnerExpired,
    BulwarkExpired,
    SingularityDeployed { id: String, pos: crate::ws::Vec2Json },
    MineDeployed { id: String, pos: crate::ws::Vec2Json },
    MineDetonated { mine_id: String, pos: crate::ws::Vec2Json },
    KilledShip { victim: String },
    Died { by: Option<String> },
    Respawned,
}

/// Convert an engine [`arena_engine::Event`] to [`EventJson`].
///
/// Returns `None` for event variants not yet mapped to PROTOCOL §7 JSON
/// (none currently — all engine variants are covered).
pub(crate) fn event_to_json(e: &arena_engine::Event) -> Option<EventJson> {
    use arena_engine::Event;
    use crate::ws::sigil_to_str;
    Some(match e {
        Event::TookShield { amount, by } => {
            EventJson::TookShield { amount: *amount, by: by.clone() }
        }
        Event::TookHull { amount, by } => {
            EventJson::TookHull { amount: *amount, by: by.clone() }
        }
        Event::ShieldDown => EventJson::ShieldDown,
        Event::LanceTookHull { amount, by } => {
            EventJson::LanceTookHull { amount: *amount, by: by.clone() }
        }
        Event::CollisionTookShield { amount } => {
            EventJson::CollisionTookShield { amount: *amount }
        }
        Event::CollisionTookHull { amount } => {
            EventJson::CollisionTookHull { amount: *amount }
        }
        Event::RelicDropped { relic_id, pos } => EventJson::RelicDropped {
            relic_id: relic_id.clone(),
            pos: vec2_to_json(*pos),
        },
        Event::SigilGranted { which } => {
            EventJson::SigilGranted { which: sigil_to_str(which) }
        }
        Event::SigilDischarged { which } => {
            EventJson::SigilDischarged { which: sigil_to_str(which) }
        }
        Event::AfterburnerExpired => EventJson::AfterburnerExpired,
        Event::BulwarkExpired => EventJson::BulwarkExpired,
        Event::SingularityDeployed { id, pos } => EventJson::SingularityDeployed {
            id: id.clone(),
            pos: vec2_to_json(*pos),
        },
        Event::MineDeployed { id, pos } => {
            EventJson::MineDeployed { id: id.clone(), pos: vec2_to_json(*pos) }
        }
        Event::MineDetonated { mine_id, pos } => EventJson::MineDetonated {
            mine_id: mine_id.clone(),
            pos: vec2_to_json(*pos),
        },
        Event::KilledShip { victim } => {
            EventJson::KilledShip { victim: victim.clone() }
        }
        Event::Died { by } => EventJson::Died { by: by.clone() },
        Event::Respawned => EventJson::Respawned,
    })
}
