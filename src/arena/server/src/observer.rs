//! Observer god-mode stream (issue 07).
//!
//! [`ObserverHub`] is the broadcast bus: the match tick loop calls
//! [`ObserverHub::publish_god_view`] after each `engine.step()`, and Viewer WS
//! clients subscribe via [`ObserverHub::subscribe`] to receive full-world JSON
//! frames.
//!
//! ## Design
//!
//! - The hub wraps a [`tokio::sync::broadcast`] channel whose items are
//!   pre-serialised JSON `String`s (ready to forward over a WebSocket without
//!   re-allocating).
//! - [`ObserverHub`] is `Clone + Send + Sync`; it is stored in [`AppState`] and
//!   cloned cheaply per request.
//! - The Viewer WS handler silently ignores any bytes the client sends —
//!   the stream is strictly **read-only**.
//!
//! ## JSON wire format
//!
//! Each frame is a JSON object with `"type": "godView"` and the following
//! fields (all engine `snake_case` names are mapped to `camelCase` here):
//!
//! | Engine field                       | JSON key               |
//! |------------------------------------|------------------------|
//! | `GodView.tick`                     | `"tick"`               |
//! | `GodView.max_ticks`                | `"maxTicks"`           |
//! | `GodView.seed`                     | `"seed"`               |
//! | `GodView.arena`                    | `"arena"`              |
//! | `GodView.ships`                    | `"ships"`              |
//! | `GodShipView.ang_vel`              | `"angVel"`             |
//! | `GodShipView.cannon_cooldown`      | `"cannonCooldown"`     |
//! | `GodShipView.relics_carried`       | `"relicsCarried"`      |
//! | `GodShipView.afterburner_ticks_left` | `"afterburnerTicksLeft"` |
//! | `GodView.anchors`                  | `"anchors"`            |
//! | `AnchorView.ship_id`               | `"shipId"`             |
//! | `GodView.relics`                   | `"relics"`             |
//! | `GodView.asteroids`                | `"asteroids"`          |
//! | `GodView.projectiles`              | `"projectiles"`        |
//! | `GodView.singularities`            | `"singularities"`      |
//! | `SingularityView.ticks_left`       | `"ticksLeft"`          |
//! | `GodView.mines`                    | `"mines"`              |
//! | `GodView.scores`                   | `"scores"`             |
//!
//! The `ships` array includes ALL ships with FULL state including `aether` and
//! `sigil` — fields deliberately hidden in per-bot `Observation`s.  Bots never
//! receive this type; they always receive the fog-filtered `tick` message.
//!
//! ## Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 08 (recording) | call `hub.subscribe()` in the recorder; tap the same frames |
//! | 11 (admin / projector) | replace `AppState.observer_hub` with a hub for a different match |

use std::collections::HashMap;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use serde::Serialize;
use tokio::sync::broadcast;

use arena_engine::GodView;

use crate::routes::AppState;
use crate::ws::{
    AnchorViewJson, ArenaDimsJson, AsteroidViewJson, MineViewJson, ProjectileViewJson,
    RelicViewJson, ResourceJson, SingularityViewJson, Vec2Json, anchor_to_json, asteroid_to_json,
    mine_to_json, projectile_to_json, relic_to_json, resource_to_json, ship_class_to_str,
    sigil_to_str, singularity_to_json, vec2_to_json,
};

// ── ObserverHub ───────────────────────────────────────────────────────────────

/// Broadcast hub for the god-mode observer stream.
///
/// The match tick loop calls [`publish_god_view`](Self::publish_god_view) once
/// per tick after `engine.step()`.  Any number of Viewer clients can
/// [`subscribe`](Self::subscribe) and receive every subsequent frame.
///
/// ## Cloning
///
/// [`ObserverHub`] is `Clone` — the inner `broadcast::Sender` is reference-
/// counted, so clones share the same channel.  Store one clone in
/// [`AppState`](crate::routes::AppState) and call `subscribe()` per connection.
#[derive(Clone)]
pub struct ObserverHub {
    tx: broadcast::Sender<String>,
}

impl ObserverHub {
    /// Create a new hub backed by a broadcast channel with capacity 256.
    ///
    /// 256 frames ≈ ~8 seconds of buffering at 30 Hz — enough to absorb a slow
    /// viewer startup without the tick loop blocking.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    /// Publish a pre-serialised JSON frame string to all active subscribers.
    ///
    /// Silently drops the frame when no subscribers are listening or the
    /// channel is full (the tick loop must never block on the observer).
    pub fn publish(&self, frame: String) {
        // `send` errors only when there are no active receivers or the channel
        // is full.  Both cases are non-fatal for the match loop.
        let _ = self.tx.send(frame);
    }

    /// Serialise `gv` to a `"godView"` JSON frame and publish it.
    pub fn publish_god_view(&self, gv: &GodView) {
        self.publish(god_view_to_json(gv));
    }

    /// Subscribe to the god-mode frame stream.
    ///
    /// The returned receiver will receive all frames published **after** this
    /// call.  Use [`broadcast::Receiver::recv`] (async) or
    /// [`broadcast::Receiver::try_recv`] to consume frames.
    ///
    /// ## Seam: issue 08 (recording)
    ///
    /// The recorder subscribes here and writes every frame to its log.
    ///
    /// ## Seam: issue 11 (admin projector)
    ///
    /// The admin swaps `AppState.observer_hub` for a hub attached to the chosen
    /// match; all active Viewer connections reconnect or are re-subscribed.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

impl Default for ObserverHub {
    fn default() -> Self {
        Self::new()
    }
}

// ── JSON mirror types ─────────────────────────────────────────────────────────

/// Full per-ship state in a god-view frame — includes `aether` and `sigil`
/// which are hidden in the per-bot [`arena_engine::Observation`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GodShipViewJson {
    pub id: String,
    pub class: String,
    pub alive: bool,
    pub invuln: bool,
    pub pos: Vec2Json,
    pub vel: Vec2Json,
    pub heading: f32,
    pub ang_vel: f32,
    pub hull: ResourceJson,
    pub shield: ResourceJson,
    /// Aether resource — **hidden from bots** in `OtherShipView`; present here.
    pub aether: ResourceJson,
    /// Held sigil — **hidden from bots** in `OtherShipView`; present here.
    pub sigil: Option<String>,
    pub cannon_cooldown: u32,
    pub relics_carried: u32,
    pub afterburner_ticks_left: u32,
}

/// The top-level god-mode frame sent to Viewer clients each tick.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GodViewFrameJson {
    /// Always `"godView"`.
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub tick: u32,
    pub max_ticks: u32,
    pub seed: u64,
    pub arena: ArenaDimsJson,
    /// All ships with full state (including bot-hidden aether/sigil).
    pub ships: Vec<GodShipViewJson>,
    pub anchors: Vec<AnchorViewJson>,
    pub relics: Vec<RelicViewJson>,
    pub asteroids: Vec<AsteroidViewJson>,
    pub projectiles: Vec<ProjectileViewJson>,
    pub singularities: Vec<SingularityViewJson>,
    /// ALL mines — not fog-filtered (bots only see mines within detection radius).
    pub mines: Vec<MineViewJson>,
    pub scores: HashMap<String, f32>,
}

// ── Serialisation ─────────────────────────────────────────────────────────────

fn god_ship_to_json(s: &arena_engine::GodShipView) -> GodShipViewJson {
    GodShipViewJson {
        id: s.id.clone(),
        class: ship_class_to_str(&s.class),
        alive: s.alive,
        invuln: s.invuln,
        pos: vec2_to_json(s.pos),
        vel: vec2_to_json(s.vel),
        heading: s.heading,
        ang_vel: s.ang_vel,
        hull: resource_to_json(s.hull),
        shield: resource_to_json(s.shield),
        aether: resource_to_json(s.aether),
        sigil: s.sigil.as_ref().map(sigil_to_str),
        cannon_cooldown: s.cannon_cooldown,
        relics_carried: s.relics_carried,
        afterburner_ticks_left: s.afterburner_ticks_left,
    }
}

/// Serialise a [`GodView`] to a `"godView"` JSON frame string.
///
/// Public so tests can call it directly without going through the hub.
pub fn god_view_to_json(gv: &GodView) -> String {
    let frame = GodViewFrameJson {
        type_: "godView",
        tick: gv.tick,
        max_ticks: gv.max_ticks,
        seed: gv.seed,
        arena: ArenaDimsJson { width: gv.arena.width, height: gv.arena.height },
        ships: gv.ships.iter().map(god_ship_to_json).collect(),
        anchors: gv.anchors.iter().map(anchor_to_json).collect(),
        relics: gv.relics.iter().map(relic_to_json).collect(),
        asteroids: gv.asteroids.iter().map(asteroid_to_json).collect(),
        projectiles: gv.projectiles.iter().map(projectile_to_json).collect(),
        singularities: gv.singularities.iter().map(singularity_to_json).collect(),
        mines: gv.mines.iter().map(mine_to_json).collect(),
        scores: gv.scores.clone(),
    };
    serde_json::to_string(&frame).unwrap_or_default()
}

// ── axum WS handler ───────────────────────────────────────────────────────────

/// axum WebSocket upgrade handler for Viewer (god-mode) connections.
///
/// Mount as `any("/observe", ws_viewer_handler)` in the router.
/// The Viewer connects, immediately starts receiving `"godView"` frames for
/// the current match, and cannot affect match state (read-only).
pub async fn ws_viewer_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_viewer(socket, state))
}

/// Drive a single Viewer WS connection: forward god-view frames, ignore input.
///
/// - Subscribes to `state.observer_hub` on entry.
/// - Forwards every received broadcast frame as a Text message to the client.
/// - Any data sent by the client is silently discarded (read-only semantics).
/// - Returns when the client disconnects or the hub shuts down.
async fn handle_ws_viewer(mut socket: WebSocket, state: AppState) {
    let mut rx = state.observer_hub.subscribe();

    loop {
        tokio::select! {
            // Forward god-view frames from the hub to the Viewer socket.
            result = rx.recv() => {
                match result {
                    Ok(frame) => {
                        if socket
                            .send(Message::Text(frame.into()))
                            .await
                            .is_err()
                        {
                            return; // Viewer disconnected
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Hub outpaced the viewer — skip missed frames and continue.
                        // The viewer's renderer will interpolate across the gap.
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => return, // hub shut down
                }
            }

            // Drain any bytes sent by the Viewer (read-only — match unaffected).
            msg = socket.recv() => {
                match msg {
                    None | Some(Err(_)) => return, // disconnected
                    Some(Ok(_)) => {}               // silently ignored
                }
            }
        }
    }
}
