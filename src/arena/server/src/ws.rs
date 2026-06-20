//! WebSocket Bot persistent connection & per-match driver (ADR-0001 v2).
//!
//! ## Protocol flow (PROTOCOL.md §5) — persistent model
//!
//! ```text
//! Bot                                  Arena
//!  |──── WebSocket connect ────────────▶|
//!  |◀──── welcome ──────────────────────|
//!  |──── join ─────────────────────────▶|
//!  |◀──── assigned ─────────────────────|  (or close on bad token)
//!  |                                    |  ← bot stays connected, idle
//!  |◀──── matchStart ───────────────────|  per match (facilitator starts one)
//!  |◀──── tick ─────────────────────────|  per tick (bot's PRIVATE observation)
//!  |──── action ───────────────────────▶|  per tick, before deadline
//!  |◀──── matchEnd ─────────────────────|  per match
//!  |                 ... repeats ...     |  same socket, many matches
//!  |──── close ────────────────────────▶|  bot closes when done
//! ```
//!
//! ## WsSession — the persistent socket owner
//!
//! [`WsSession`] owns the WebSocket for the entire connection lifetime via
//! a single long-lived bridge task.  It exposes:
//!
//! - An **outbound** `tokio::sync::mpsc::Sender<String>` (clonable) — the
//!   bridge forwards every string frame to the socket.  Used for both per-tick
//!   observations AND matchStart/matchEnd envelopes.
//! - An **inbound** `Arc<Mutex<std::sync::mpsc::Receiver<Intent>>>` — the
//!   bridge reads socket text frames, parses them with [`parse_action`], and
//!   delivers [`Intent`]s.  Each per-match [`WsBotDriver`] locks this to
//!   `recv_timeout` within the tick deadline.
//!
//! `WsSession` implements [`BotSessionSource`](crate::resolver::BotSessionSource)
//! so the [`WsConnectionRegistry`](crate::resolver::WsConnectionRegistry) can
//! store it and the [`ConnectionResolver`](crate::resolver::ConnectionResolver)
//! can mint a fresh [`WsBotDriver`] per match without consuming the session.
//!
//! ## Serde / PROTOCOL mismatches (mapped here; PROTOCOL.md not modified)
//!
//! All engine types use `snake_case` field names; PROTOCOL §6 uses `camelCase`.
//! The mapping is done in this module via mirror JSON types:
//!
//! | Engine field              | PROTOCOL JSON key          |
//! |---------------------------|----------------------------|
//! | `Observation.self_view`   | `"self"`                   |
//! | `Observation.max_ticks`   | `"maxTicks"`               |
//! | `SelfView.ang_vel`        | `"angVel"`                 |
//! | `SelfView.cannon_cooldown`| `"cannonCooldown"`         |
//! | `SelfView.relics_carried` | `"relicsCarried"`          |
//! | `SelfView.afterburner_ticks_left` | `"afterburnerTicksLeft"` |
//! | `AnchorView.ship_id`      | `"shipId"`                 |
//! | `SingularityView.ticks_left` | `"ticksLeft"`           |
//! | `OtherShipView.relics_carried` | `"relicsCarried"`     |
//! | `Intent.sigil_target`     | `"sigilTarget"`            |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use arena_engine::{Intent, ShipClass, Vec2};

use crate::health::BotHealthEntry;
use crate::resolver::BotSessionSource;
use crate::routes::AppState;
use crate::runner::BotDriver;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Protocol version emitted in the `welcome` message.
pub const PROTOCOL_VERSION: &str = "1";
/// Game-type tag emitted in the `welcome` message.
pub const GAME_TYPE: &str = "ShatterbeltSalvagers";

// ── Outgoing wire types (Arena → Bot) ────────────────────────────────────────

/// Sent immediately on WebSocket connection (PROTOCOL.md §5).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeMsg {
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub protocol_version: String,
    pub session_id: String,
    pub game_type: &'static str,
}

/// Sent after successful token validation (PROTOCOL.md §5).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignedMsg {
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub ship_id: String,
}

/// Sent when the match begins (PROTOCOL.md §5).
#[derive(Debug, Clone, Serialize)]
pub struct MatchStartMsg {
    #[serde(rename = "type")]
    pub type_: &'static str,
}

/// Per-tick observation sent to the bot (PROTOCOL.md §6).
///
/// This is the bot's **private** fog-respecting view. The engine already
/// applies fog in `Engine::observation(ship_id)`:
/// - Enemy `aether` and `sigil` are absent (`OtherShipView` never exposes them).
/// - Enemy mines appear only when within the detection radius.
/// - `invuln` is exposed (so bots don't waste shots on immune ships).
///
/// We serialize exactly what the engine gives us; no god-view fields are added.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickMsg {
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub tick: u32,
    pub max_ticks: u32,
    pub seed: u64,
    pub arena: ArenaDimsJson,
    #[serde(rename = "self")]
    pub self_view: SelfViewJson,
    pub anchors: Vec<AnchorViewJson>,
    pub ships: Vec<OtherShipViewJson>,
    pub relics: Vec<RelicViewJson>,
    pub asteroids: Vec<AsteroidViewJson>,
    pub projectiles: Vec<ProjectileViewJson>,
    pub singularities: Vec<SingularityViewJson>,
    pub mines: Vec<MineViewJson>,
    pub scores: HashMap<String, f32>,
    pub events: Vec<EventJson>,
}

/// Sent at match end (PROTOCOL.md §5).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchEndMsg {
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub results: MatchResultsJson,
}

/// Match results payload inside [`MatchEndMsg`].
#[derive(Debug, Serialize)]
pub struct MatchResultsJson {
    pub winner: Option<String>,
    pub scores: HashMap<String, f32>,
    pub ticks: u32,
}

// ── Incoming wire types (Bot → Arena) ────────────────────────────────────────

/// First message from the bot after connect (PROTOCOL.md §5).
///
/// The bot echoes the `sessionId` from `welcome`, and supplies its `token`
/// (obtained via `POST /register`) to identify itself.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinMsg {
    pub session_id: String,
    pub token: String,
    pub name: String,
    pub preferred_class: Option<String>,
}

/// Per-tick action from the bot (PROTOCOL.md §8).
///
/// All fields are optional — omitted fields keep the engine's per-field
/// persisted value (PROTOCOL.md §2 / ADR-0003).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionMsg {
    #[serde(rename = "type", default)]
    pub type_: Option<String>,
    pub turn: Option<f32>,
    pub thrust: Option<f32>,
    pub fire: Option<bool>,
    pub sigil: Option<bool>,
    pub sigil_target: Option<Vec2Json>,
}

// ── JSON mirror types for engine structs ─────────────────────────────────────

/// PROTOCOL §3: `{ "x": float, "y": float }`.
/// Same naming in engine and PROTOCOL — no rename needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vec2Json {
    pub x: f32,
    pub y: f32,
}

/// `{ "cur": float, "max": float }` — Hull, Shield, and Aether.
#[derive(Debug, Serialize)]
pub struct ResourceJson {
    pub cur: f32,
    pub max: f32,
}

/// Arena dimensions, PROTOCOL §6 `"arena"` field.
#[derive(Debug, Serialize)]
pub struct ArenaDimsJson {
    pub width: f32,
    pub height: f32,
}

/// PROTOCOL §6 `"self"` block — observing ship's full state.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfViewJson {
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
    pub aether: ResourceJson,
    pub sigil: Option<String>,
    pub cannon_cooldown: u32,
    pub relics_carried: u32,
    pub afterburner_ticks_left: u32,
}

/// PROTOCOL §6 `"ships"` array entry — enemy ship view.
/// `aether` and `sigil` are intentionally absent (fog / bluff room).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherShipViewJson {
    pub id: String,
    pub class: String,
    pub alive: bool,
    pub invuln: bool,
    pub pos: Vec2Json,
    pub vel: Vec2Json,
    pub heading: f32,
    pub hull: ResourceJson,
    pub shield: ResourceJson,
    pub relics_carried: u32,
}

/// PROTOCOL §6 `"anchors"` array entry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorViewJson {
    pub ship_id: String,
    pub pos: Vec2Json,
}

#[derive(Debug, Serialize)]
pub struct RelicViewJson {
    pub id: String,
    pub pos: Vec2Json,
    pub vel: Vec2Json,
    pub value: f32,
}

#[derive(Debug, Serialize)]
pub struct AsteroidViewJson {
    pub id: String,
    pub pos: Vec2Json,
    pub vel: Vec2Json,
    pub radius: f32,
}

#[derive(Debug, Serialize)]
pub struct ProjectileViewJson {
    pub id: String,
    pub pos: Vec2Json,
    pub vel: Vec2Json,
    pub owner: String,
}

/// PROTOCOL §6 `"singularities"` array entry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingularityViewJson {
    pub id: String,
    pub pos: Vec2Json,
    pub radius: f32,
    pub ticks_left: u32,
}

#[derive(Debug, Serialize)]
pub struct MineViewJson {
    pub id: String,
    pub pos: Vec2Json,
    pub own: bool,
}

// Re-export from the shared events_json module so the TickMsg.events field
// continues to compile unchanged, and downstream code (observer.rs) can also
// use EventJson without duplicating the mapping.
pub use crate::events_json::EventJson;

// ── WsSession — persistent socket owner ──────────────────────────────────────

/// Persistent WS bot session: owns the WebSocket for the entire connection
/// lifetime via a single long-lived bridge task.
///
/// ## Channels
///
/// - **Outbound** (`outbound_tx`): `tokio::sync::mpsc::Sender<String>` (capacity
///   16) — the bridge task drains this and writes each string to the socket.
///   Used for tick observations AND matchStart/matchEnd envelopes.
/// - **Inbound** (`action_rx`): `Arc<Mutex<std::sync::mpsc::Receiver<Intent>>>`
///   — the bridge task parses incoming socket text frames with
///   [`parse_action`] and sends [`Intent`]s into the sync channel.  Each
///   per-match [`WsBotDriver`] holds a clone of the `Arc` and locks it
///   during `decide` to drain stale intents and block on
///   `recv_timeout(deadline)`.
///
/// ## Envelope sending
///
/// Call [`WsSession::try_send_envelope`] to push a matchStart/matchEnd JSON
/// frame.  This is non-blocking (`try_send`) — if the channel is momentarily
/// full the frame is silently dropped (a warning is logged).  The capacity of
/// 16 provides enough headroom for concurrent envelope and tick frames.
///
/// ## Per-match driver
///
/// Call [`WsSession::make_driver`] to mint a fresh [`WsBotDriver`] for each
/// match.  The driver holds a clone of the outbound sender and a clone of the
/// inbound `Arc<Mutex<Receiver>>`, so it shares the same underlying socket
/// without requiring exclusive ownership.
pub struct WsSession {
    /// Outbound channel shared with the bridge task and per-match drivers.
    outbound_tx: tokio::sync::mpsc::Sender<String>,
    /// Inbound parsed-action channel shared across per-match drivers.
    action_rx: Arc<Mutex<std::sync::mpsc::Receiver<Intent>>>,
}

impl WsSession {
    /// Build a `WsSession` from a live WebSocket and return the bridge future.
    ///
    /// The returned future must be `tokio::spawn`ed before any match can
    /// drive this session.  The session is live for as long as the bridge
    /// future is running (i.e. until the socket closes).
    pub fn new(socket: WebSocket) -> (Self, impl std::future::Future<Output = ()> + Send) {
        let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (action_stx, action_rx) = std::sync::mpsc::sync_channel::<Intent>(8);
        let action_rx = Arc::new(Mutex::new(action_rx));
        let bridge = ws_bridge_task(socket, outbound_rx, action_stx);
        (Self { outbound_tx, action_rx }, bridge)
    }

    /// Mint a fresh per-match [`WsBotDriver`] backed by this session's channels.
    ///
    /// The driver holds:
    /// - A **clone** of the outbound `Sender` (so it can send tick JSON).
    /// - A **clone** of the `Arc<Mutex<Receiver>>` (so it can recv intents).
    /// - The `deadline` for `recv_timeout`.
    /// - An optional health entry for per-tick health tracking.
    ///
    /// Only one match runs per team at a time, so there is no contention on
    /// the receiver lock.
    pub fn make_driver(
        &self,
        deadline: Duration,
        health: Option<Arc<BotHealthEntry>>,
    ) -> Box<dyn BotDriver> {
        Box::new(WsBotDriver {
            outbound_tx: self.outbound_tx.clone(),
            action_rx: Arc::clone(&self.action_rx),
            deadline,
            health,
            skipped_ticks: 0,
            last_seen: None,
        })
    }

    /// Queue a raw JSON envelope frame (matchStart / matchEnd) to the bot.
    ///
    /// Uses `try_send` so it never blocks the async caller.  Returns `true`
    /// if the frame was queued, `false` if the channel was full or closed.
    pub fn try_send_envelope(&self, json: String) -> bool {
        self.outbound_tx.try_send(json).is_ok()
    }
}

impl BotSessionSource for WsSession {
    fn make_driver(
        &self,
        deadline: Duration,
        health: Option<Arc<BotHealthEntry>>,
    ) -> Box<dyn BotDriver> {
        WsSession::make_driver(self, deadline, health)
    }

    fn try_send_envelope(&self, json: String) -> bool {
        WsSession::try_send_envelope(self, json)
    }
}

// ── WsBotDriver — per-match BotDriver seam ────────────────────────────────────

/// A [`BotDriver`] backed by a persistent [`WsSession`].
///
/// Minted once per match via [`WsSession::make_driver`].  Shares the same
/// underlying socket (via the session's channels) with any subsequent match
/// drivers for the same team.
///
/// ## Design for spawn_blocking use
///
/// [`decide`](BotDriver::decide) is synchronous and uses
/// `tokio::sync::mpsc::Sender::blocking_send` + `std::sync::mpsc::Receiver::recv_timeout`,
/// both of which are safe to call from a `tokio::task::spawn_blocking` context.
///
/// ## Health tracking (issue 12)
///
/// When a [`BotHealthEntry`] is injected via [`WsSession::make_driver`], each
/// tick updates `last_seen` (on time) or `skipped_ticks` (on deadline miss or
/// socket drop).
pub struct WsBotDriver {
    /// Sends serialised tick JSON (and nothing else) to the bridge task.
    outbound_tx: tokio::sync::mpsc::Sender<String>,
    /// Shared inbound intent channel.  Locked exclusively during each tick's
    /// drain + recv_timeout so no two drivers race (only one match at a time
    /// per team is guaranteed by the orchestrator).
    action_rx: Arc<Mutex<std::sync::mpsc::Receiver<Intent>>>,
    /// Per-tick deadline for receiving an action.
    deadline: Duration,
    // ── Issue 12 health tracking ──────────────────────────────────────────────
    skipped_ticks: u64,
    last_seen: Option<std::time::Instant>,
    health: Option<Arc<BotHealthEntry>>,
}

impl WsBotDriver {
    /// Number of ticks where no intent was received before the deadline.
    pub fn skipped_ticks(&self) -> u64 {
        self.skipped_ticks
    }

    /// Wall-clock time of the most recent successful action receipt.
    pub fn last_seen(&self) -> Option<std::time::Instant> {
        self.last_seen
    }
}

impl BotDriver for WsBotDriver {
    fn kind(&self) -> &'static str {
        "ws"
    }

    /// Serialize `obs` to a `tick` message, send to the bot, and wait up to
    /// `deadline` for an `action` reply.
    ///
    /// - Returns `Some(intent)` if a fresh action arrived before the deadline.
    /// - Returns `None` on deadline miss, letting the engine carry the ship's
    ///   previous intent forward (per-field persistence, PROTOCOL §2 / ADR-0003).
    ///
    /// Stale actions from previous deadline misses are drained before sending
    /// the new observation so they are not mistakenly treated as fresh.
    fn decide(&mut self, tick: u32, obs: &arena_engine::Observation) -> Option<Intent> {
        // Lock the shared inbound channel for the duration of this tick.
        // Only one match runs per team at a time, so no contention occurs.
        let action_rx = self.action_rx.lock().unwrap();

        // Drain any stale actions from previous deadline misses.
        while action_rx.try_recv().is_ok() {}

        // Serialise and send the observation to the bridge task.
        let obs_json = obs_to_tick_json(tick, obs);
        if self.outbound_tx.blocking_send(obs_json).is_err() {
            // Socket bridge has dropped (bot disconnected mid-match).
            self.skipped_ticks += 1;
            if let Some(h) = &self.health {
                h.increment_skipped();
                h.set_connected(false);
            }
            return None;
        }

        // Wait up to deadline for the bot's action.
        match action_rx.recv_timeout(self.deadline) {
            Ok(intent) => {
                self.last_seen = Some(std::time::Instant::now());
                if let Some(h) = &self.health {
                    h.touch();
                    h.set_connected(true);
                }
                Some(intent)
            }
            Err(_) => {
                self.skipped_ticks += 1;
                if let Some(h) = &self.health {
                    h.increment_skipped();
                }
                None
            }
        }
    }
}

/// Async task bridging between the sync [`WsBotDriver`] channels and the WebSocket.
///
/// Runs for the lifetime of the WS connection:
/// - Reads outbound messages (tick observations AND envelope frames) from
///   `outbound_rx`, writes them as text frames to the socket.
/// - Reads action text messages from the socket, parses with [`parse_action`],
///   forwards parsed intents to `action_tx`.
async fn ws_bridge_task(
    socket: WebSocket,
    mut outbound_rx: tokio::sync::mpsc::Receiver<String>,
    action_tx: std::sync::mpsc::SyncSender<Intent>,
) {
    let (mut sink, mut stream) = socket.split();

    let write_fut = async {
        while let Some(msg) = outbound_rx.recv().await {
            if sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    };

    let read_fut = async {
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(text) = msg {
                if let Ok(intent) = parse_action(text.as_str()) {
                    let _ = action_tx.send(intent);
                }
            }
        }
    };

    tokio::select! {
        _ = write_fut => {},
        _ = read_fut => {},
    }
}

// ── axum WS handler ───────────────────────────────────────────────────────────

/// axum WebSocket upgrade handler for bot connections.
///
/// Mount as `any("/ws", ws_bot_handler)` in the router.
/// Accepts the WebSocket upgrade and delegates to [`handle_ws_bot`].
pub async fn ws_bot_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_bot(socket, state))
}

/// Drive a WS bot connection for its full lifetime.
///
/// Steps:
/// 1. **Handshake** (PROTOCOL.md §5): send `welcome`, receive `join`,
///    validate token, send `assigned` (or close on bad token).
/// 2. **Register**: build a [`WsSession`], spawn its bridge task, register
///    the session in `state.ws_registry` under the team name, and mark the
///    health entry as connected.  The bot is now idle — it waits for the
///    orchestrator to start a match.
/// 3. **Persist**: await the bridge task for the entire connection lifetime.
///    The orchestrator sends matchStart/matchEnd envelopes and per-tick
///    observations through the session's outbound channel; the bot's actions
///    arrive through the inbound channel.
/// 4. **Cleanup**: when the bridge exits (socket closed by bot or error),
///    remove the session from the registry and mark health as disconnected.
///
/// NO match is run here.  Match orchestration lives in `admin.rs`.
async fn handle_ws_bot(mut socket: WebSocket, state: AppState) {
    // ── 1. Handshake ──────────────────────────────────────────────────────────

    let session_id = Uuid::new_v4().to_string();

    // Send welcome
    let welcome = WelcomeMsg {
        type_: "welcome",
        protocol_version: PROTOCOL_VERSION.to_owned(),
        session_id: session_id.clone(),
        game_type: GAME_TYPE,
    };
    if socket
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    // Receive join
    let join_text = match recv_text(&mut socket).await {
        Some(t) => t,
        None => return,
    };
    let join: JoinMsg = match serde_json::from_str(&join_text) {
        Ok(j) => j,
        Err(_) => {
            close_with_error(&mut socket, 1008, "malformed join message").await;
            return;
        }
    };

    // Validate token via the shared registry
    let team = match state.registry.resolve(&join.token) {
        Some(t) => t,
        None => {
            close_with_error(&mut socket, 1008, "invalid or absent token").await;
            return;
        }
    };

    // Inform the bot of its stable team identity.
    // `assigned.shipId` is the team name; per-match ship ids come from
    // `self.id` in each tick observation (assigned by the match specs).
    let assigned = AssignedMsg {
        type_: "assigned",
        ship_id: team.clone(),
    };
    if socket
        .send(Message::Text(
            serde_json::to_string(&assigned).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    // ── 2. Build WsSession and spawn bridge ───────────────────────────────────

    let (session, bridge_future) = WsSession::new(socket);
    let session: Arc<dyn BotSessionSource> = Arc::new(session);
    let bridge_handle = tokio::spawn(bridge_future);

    // ── 3. Register session and mark health connected ─────────────────────────

    state.ws_registry.register(team.clone(), Arc::clone(&session));
    let health_entry = state
        .health_store
        .register(crate::health::BotHealthEntry::new(&team, "ws"));
    health_entry.set_connected(true);

    // ── 4. Await bridge — live for the entire connection lifetime ─────────────
    //
    // The match orchestrator sends matchStart/matchEnd envelopes through the
    // session's outbound channel; per-tick observations come from WsBotDriver
    // via the same channel.  We just wait here until the bridge exits.
    let _ = bridge_handle.await;

    // ── 5. Cleanup on disconnect ──────────────────────────────────────────────

    state.ws_registry.remove(&team);
    health_entry.set_connected(false);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Receive one text message from the socket, skipping non-text frames.
///
/// Returns `None` when the connection closes or an error occurs.
async fn recv_text(socket: &mut WebSocket) -> Option<String> {
    loop {
        match socket.recv().await? {
            Ok(Message::Text(t)) => return Some(t.as_str().to_owned()),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue, // skip pings, pongs, binary
        }
    }
}

/// Send a WebSocket close frame with an error code and reason.
async fn close_with_error(socket: &mut WebSocket, code: u16, reason: &'static str) {
    socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from_static(reason),
        })))
        .await
        .ok();
}

/// Parse an `action` JSON string into an engine [`Intent`].
///
/// Permissive: accepts any JSON object with optional turn/thrust/fire/sigil/sigilTarget
/// fields, regardless of whether `"type": "action"` is present.
pub fn parse_action(text: &str) -> Result<Intent, serde_json::Error> {
    let msg: ActionMsg = serde_json::from_str(text)?;
    Ok(Intent {
        turn: msg.turn,
        thrust: msg.thrust,
        fire: msg.fire,
        sigil: msg.sigil,
        sigil_target: msg.sigil_target.map(|v| Vec2::new(v.x, v.y)),
    })
}

// ── Observation serialisation ─────────────────────────────────────────────────

/// Serialise an engine [`arena_engine::Observation`] to the PROTOCOL §6 `tick`
/// JSON message.
///
/// All field name mismatches between the engine (snake_case) and the PROTOCOL
/// (camelCase) are handled here via the mirror JSON types defined above.
pub fn obs_to_tick_json(tick: u32, obs: &arena_engine::Observation) -> String {
    let msg = TickMsg {
        type_: "tick",
        tick,
        max_ticks: obs.max_ticks,
        seed: obs.seed,
        arena: ArenaDimsJson {
            width: obs.arena.width,
            height: obs.arena.height,
        },
        self_view: self_view_to_json(&obs.self_view),
        anchors: obs.anchors.iter().map(anchor_to_json).collect(),
        ships: obs.ships.iter().map(other_ship_to_json).collect(),
        relics: obs.relics.iter().map(relic_to_json).collect(),
        asteroids: obs.asteroids.iter().map(asteroid_to_json).collect(),
        projectiles: obs.projectiles.iter().map(projectile_to_json).collect(),
        singularities: obs
            .singularities
            .iter()
            .map(singularity_to_json)
            .collect(),
        mines: obs.mines.iter().map(mine_to_json).collect(),
        scores: obs.scores.clone(),
        events: obs.events.iter().filter_map(event_to_json).collect(),
    };
    serde_json::to_string(&msg).unwrap_or_default()
}

pub(crate) fn vec2_to_json(v: Vec2) -> Vec2Json {
    Vec2Json { x: v.x, y: v.y }
}

pub(crate) fn resource_to_json(r: arena_engine::Resource) -> ResourceJson {
    ResourceJson { cur: r.cur, max: r.max }
}

pub(crate) fn sigil_to_str(s: &arena_engine::Sigil) -> String {
    use arena_engine::Sigil;
    match s {
        Sigil::Afterburner => "Afterburner",
        Sigil::Bulwark => "Bulwark",
        Sigil::Singularity => "Singularity",
        Sigil::AetherMine => "AetherMine",
        Sigil::ArcLance => "ArcLance",
    }
    .to_owned()
}

pub(crate) fn ship_class_to_str(c: &arena_engine::ShipClass) -> String {
    match c {
        ShipClass::Skiff => "skiff".to_owned(),
    }
}

fn self_view_to_json(s: &arena_engine::SelfView) -> SelfViewJson {
    SelfViewJson {
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

fn other_ship_to_json(s: &arena_engine::OtherShipView) -> OtherShipViewJson {
    OtherShipViewJson {
        id: s.id.clone(),
        class: ship_class_to_str(&s.class),
        alive: s.alive,
        invuln: s.invuln,
        pos: vec2_to_json(s.pos),
        vel: vec2_to_json(s.vel),
        heading: s.heading,
        hull: resource_to_json(s.hull),
        shield: resource_to_json(s.shield),
        relics_carried: s.relics_carried,
    }
}

pub(crate) fn anchor_to_json(a: &arena_engine::AnchorView) -> AnchorViewJson {
    AnchorViewJson {
        ship_id: a.ship_id.clone(),
        pos: vec2_to_json(a.pos),
    }
}

pub(crate) fn relic_to_json(r: &arena_engine::RelicView) -> RelicViewJson {
    RelicViewJson {
        id: r.id.clone(),
        pos: vec2_to_json(r.pos),
        vel: vec2_to_json(r.vel),
        value: r.value,
    }
}

pub(crate) fn asteroid_to_json(a: &arena_engine::AsteroidView) -> AsteroidViewJson {
    AsteroidViewJson {
        id: a.id.clone(),
        pos: vec2_to_json(a.pos),
        vel: vec2_to_json(a.vel),
        radius: a.radius,
    }
}

pub(crate) fn projectile_to_json(p: &arena_engine::ProjectileView) -> ProjectileViewJson {
    ProjectileViewJson {
        id: p.id.clone(),
        pos: vec2_to_json(p.pos),
        vel: vec2_to_json(p.vel),
        owner: p.owner.clone(),
    }
}

pub(crate) fn singularity_to_json(s: &arena_engine::SingularityView) -> SingularityViewJson {
    SingularityViewJson {
        id: s.id.clone(),
        pos: vec2_to_json(s.pos),
        radius: s.radius,
        ticks_left: s.ticks_left,
    }
}

pub(crate) fn mine_to_json(m: &arena_engine::MineView) -> MineViewJson {
    MineViewJson {
        id: m.id.clone(),
        pos: vec2_to_json(m.pos),
        own: m.own,
    }
}

/// Convert an engine [`arena_engine::Event`] to [`EventJson`].
///
/// Thin re-export of [`crate::events_json::event_to_json`] kept here so that
/// the `observation_to_tick_json` call-site does not need to change.
use crate::events_json::event_to_json;
