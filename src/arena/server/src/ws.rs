//! WebSocket Bot connection & live match drive (issue 03).
//!
//! ## Protocol flow (PROTOCOL.md §5)
//!
//! ```text
//! Bot                                  Arena
//!  |──── WebSocket connect ────────────▶|
//!  |◀──── welcome ──────────────────────|
//!  |──── join ─────────────────────────▶|
//!  |◀──── assigned ─────────────────────|  (or close on bad token)
//!  |◀──── matchStart ───────────────────|
//!  |◀──── tick ─────────────────────────|  per tick (bot's PRIVATE observation)
//!  |──── action ───────────────────────▶|  per tick, before deadline
//!  |◀──── matchEnd ─────────────────────|
//! ```
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
//!
//! ## Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 06 (connection resolver) | [`WsBotDriver`] implements [`BotDriver`] — plug into resolver |
//! | 07 (observer god stream) | call `engine.god_view()` after each `engine.step()` in `run_ws_match` |
//! | 12 (bot health) | Add `skipped_ticks`/`last_seen` to [`WsBotDriver`]; increment on deadline miss |

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use uuid::Uuid;

use arena_engine::{Engine, Intent, ShipClass, ShipId, ShipSpec, Vec2};

use crate::bot::DefaultBotDriver;
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
    RelicDropped { relic_id: String, pos: Vec2Json },
    SigilGranted { which: String },
    SigilDischarged { which: String },
    AfterburnerExpired,
    BulwarkExpired,
    SingularityDeployed { id: String, pos: Vec2Json },
    MineDeployed { id: String, pos: Vec2Json },
    MineDetonated { mine_id: String, pos: Vec2Json },
    KilledShip { victim: String },
    Died { by: Option<String> },
    Respawned,
}

// ── WsBotDriver — BotDriver seam for issue 06 ────────────────────────────────

/// A [`BotDriver`] backed by a WebSocket connection.
///
/// Each call to [`BotDriver::decide`] serialises the observation to a `tick`
/// JSON message, sends it to the connected bot via a channel (which a background
/// async task forwards over the socket), then blocks for up to `deadline`
/// waiting for an `action` reply.
///
/// ## Design for spawn_blocking use
///
/// [`decide`](BotDriver::decide) is synchronous; it uses
/// `tokio::sync::mpsc::Sender::blocking_send` and
/// `std::sync::mpsc::Receiver::recv_timeout`, both of which are safe to call
/// from a `tokio::task::spawn_blocking` context. The match runner (MatchRunner
/// + NoopPacer) should be run inside `spawn_blocking` when WS bots are present.
///
/// For issue 03 the handler drives the match loop inline (async), so this driver
/// is not exercised directly — it is the seam for issue 06 (connection resolver).
///
/// ## Seam for issue 12 (bot health)
///
/// Add `skipped_ticks: u32` and `last_seen: Instant` fields;
/// increment `skipped_ticks` on `recv_timeout` error; reset on success.
pub struct WsBotDriver {
    /// Sends serialised tick JSON to the socket bridge task.
    obs_tx: tokio::sync::mpsc::Sender<String>,
    /// Receives parsed intents from the socket bridge task.
    action_rx: std::sync::mpsc::Receiver<Intent>,
    /// Per-tick deadline for receiving an action.
    deadline: Duration,
    // ── Issue 12 health seam ──────────────────────────────────────────────
    // skipped_ticks: u32,
    // last_seen: Option<std::time::Instant>,
}

impl WsBotDriver {
    /// Construct a `WsBotDriver` and its corresponding async socket bridge.
    ///
    /// The returned `Future` must be `tokio::spawn`ed before running the match.
    /// The driver communicates with the bridge via the returned channels:
    /// - Driver → bridge: serialised tick JSON (tokio mpsc)
    /// - Bridge → driver: parsed intents (std sync mpsc)
    ///
    /// ## Usage (issue 06 connection resolver)
    ///
    /// ```rust,ignore
    /// let (driver, bridge) = WsBotDriver::new(socket, state.tick_deadline);
    /// tokio::spawn(bridge);
    /// let drivers: Vec<Box<dyn BotDriver>> = vec![Box::new(driver), ...];
    /// tokio::task::spawn_blocking(move || {
    ///     MatchRunner::new(seed, params, specs, drivers, Box::new(NoopPacer))
    ///         .run_to_completion()
    /// }).await
    /// ```
    pub fn new(
        socket: WebSocket,
        deadline: Duration,
    ) -> (Self, impl std::future::Future<Output = ()> + Send) {
        let (obs_tx, obs_rx) = tokio::sync::mpsc::channel::<String>(1);
        let (action_stx, action_rx) = std::sync::mpsc::sync_channel::<Intent>(8);
        let bridge = ws_bridge_task(socket, obs_rx, action_stx);
        (Self { obs_tx, action_rx, deadline }, bridge)
    }
}

impl BotDriver for WsBotDriver {
    /// Serialize `obs` to a `tick` message, forward to the bot, and wait up to
    /// `deadline` for an `action` reply.
    ///
    /// - Returns `Some(intent)` if a fresh action arrived before the deadline.
    /// - Returns `None` on deadline miss, letting the engine carry the ship's
    ///   previous intent forward (per-field persistence, PROTOCOL §2 / ADR-0003).
    ///
    /// Stale actions from previous deadline misses are drained before sending
    /// the new observation so they are not mistakenly treated as fresh.
    fn decide(&mut self, tick: u32, obs: &arena_engine::Observation) -> Option<Intent> {
        // Drain any stale actions from previous deadline misses.
        while self.action_rx.try_recv().is_ok() {}

        // Serialise and send the observation to the socket bridge.
        let obs_json = obs_to_tick_json(tick, obs);
        self.obs_tx.blocking_send(obs_json).ok()?;

        // Wait up to deadline for the bot's action.
        self.action_rx.recv_timeout(self.deadline).ok()
    }
}

/// Async task bridging between the sync [`WsBotDriver`] channels and the WebSocket.
///
/// Runs for the lifetime of the WS connection:
/// - Reads serialised tick observations from the driver, writes to socket.
/// - Reads action text messages from the socket, parses, writes to driver.
async fn ws_bridge_task(
    socket: WebSocket,
    mut obs_rx: tokio::sync::mpsc::Receiver<String>,
    action_tx: std::sync::mpsc::SyncSender<Intent>,
) {
    let (mut sink, mut stream) = socket.split();

    let write_fut = async {
        while let Some(obs_json) = obs_rx.recv().await {
            if sink.send(Message::Text(obs_json.into())).await.is_err() {
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

/// Drive a single WS bot connection through the full match lifecycle.
///
/// Steps:
/// 1. **Handshake** (PROTOCOL.md §5): send `welcome`, receive `join`,
///    validate token, send `assigned` (or close on bad token).
/// 2. **Match** (PROTOCOL.md §5–8): send `matchStart`, then per tick:
///    send `tick` (bot's private fog-respecting observation),
///    await `action` up to `state.tick_deadline`,
///    advance the engine, repeat until match over.
/// 3. **matchEnd**: send `matchEnd` with results.
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
    let _team = match state.registry.resolve(&join.token) {
        Some(t) => t,
        None => {
            close_with_error(&mut socket, 1008, "invalid or absent token").await;
            return;
        }
    };

    // Assign the WS bot to ship-0 (issue 06 connection resolver will handle
    // multi-bot slot assignment; for now one WS bot per match, always ship-0).
    let ws_ship_id: ShipId = "ship-0".to_owned();

    let assigned = AssignedMsg {
        type_: "assigned",
        ship_id: ws_ship_id.clone(),
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

    // ── 2. Match setup ────────────────────────────────────────────────────────

    let params = state.match_params.clone();
    let seed = state.match_seed;
    let default_ship_id: ShipId = "ship-1".to_owned();

    // Place ships symmetrically in the arena.
    let specs = vec![
        ShipSpec {
            id: ws_ship_id.clone(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.2, params.arena_h * 0.5),
        },
        ShipSpec {
            id: default_ship_id.clone(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.8, params.arena_h * 0.5),
        },
    ];

    let mut engine = Engine::new(seed, params.clone(), specs);
    let mut default_driver = DefaultBotDriver::new(&params);

    // ── 3. matchStart ─────────────────────────────────────────────────────────

    let match_start = MatchStartMsg { type_: "matchStart" };
    if socket
        .send(Message::Text(
            serde_json::to_string(&match_start).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    // ── 4. Match loop ─────────────────────────────────────────────────────────

    while !engine.is_match_over() {
        let tick = engine.tick();

        // Send the bot its private fog-respecting observation.
        if let Some(ws_obs) = engine.observation(&ws_ship_id) {
            let tick_json = obs_to_tick_json(tick, &ws_obs);
            if socket
                .send(Message::Text(tick_json.into()))
                .await
                .is_err()
            {
                return; // bot disconnected
            }
        }

        // Wait up to deadline for the bot's action (PROTOCOL §2: missed deadline
        // → previous intent persists in the engine; server does not track it).
        let ws_intent = timeout(state.tick_deadline, recv_text(&mut socket))
            .await
            .ok()
            .flatten()
            .and_then(|text| parse_action(&text).ok());

        // Collect all intents for this tick.
        let mut intents: Vec<(ShipId, Intent)> = Vec::new();
        if let Some(intent) = ws_intent {
            intents.push((ws_ship_id.clone(), intent));
        }
        if let Some(obs) = engine.observation(&default_ship_id) {
            if let Some(intent) = default_driver.decide(tick, &obs) {
                intents.push((default_ship_id.clone(), intent));
            }
        }

        // Issue 07 seam: call engine.god_view() here to broadcast to Viewers.
        engine.step(intents);
    }

    // ── 5. matchEnd ───────────────────────────────────────────────────────────

    let ship_ids = [ws_ship_id.clone(), default_ship_id.clone()];
    let scores: HashMap<String, f32> = ship_ids
        .iter()
        .map(|id| (id.clone(), engine.score(id).unwrap_or(0.0)))
        .collect();

    let match_end = MatchEndMsg {
        type_: "matchEnd",
        results: MatchResultsJson {
            winner: engine.winner(),
            scores,
            ticks: engine.tick(),
        },
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&match_end).unwrap_or_default().into(),
        ))
        .await
        .ok();
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

fn vec2_to_json(v: Vec2) -> Vec2Json {
    Vec2Json { x: v.x, y: v.y }
}

fn resource_to_json(r: arena_engine::Resource) -> ResourceJson {
    ResourceJson { cur: r.cur, max: r.max }
}

fn sigil_to_str(s: &arena_engine::Sigil) -> String {
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

fn ship_class_to_str(c: &arena_engine::ShipClass) -> String {
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

fn anchor_to_json(a: &arena_engine::AnchorView) -> AnchorViewJson {
    AnchorViewJson {
        ship_id: a.ship_id.clone(),
        pos: vec2_to_json(a.pos),
    }
}

fn relic_to_json(r: &arena_engine::RelicView) -> RelicViewJson {
    RelicViewJson {
        id: r.id.clone(),
        pos: vec2_to_json(r.pos),
        vel: vec2_to_json(r.vel),
        value: r.value,
    }
}

fn asteroid_to_json(a: &arena_engine::AsteroidView) -> AsteroidViewJson {
    AsteroidViewJson {
        id: a.id.clone(),
        pos: vec2_to_json(a.pos),
        vel: vec2_to_json(a.vel),
        radius: a.radius,
    }
}

fn projectile_to_json(p: &arena_engine::ProjectileView) -> ProjectileViewJson {
    ProjectileViewJson {
        id: p.id.clone(),
        pos: vec2_to_json(p.pos),
        vel: vec2_to_json(p.vel),
        owner: p.owner.clone(),
    }
}

fn singularity_to_json(s: &arena_engine::SingularityView) -> SingularityViewJson {
    SingularityViewJson {
        id: s.id.clone(),
        pos: vec2_to_json(s.pos),
        radius: s.radius,
        ticks_left: s.ticks_left,
    }
}

fn mine_to_json(m: &arena_engine::MineView) -> MineViewJson {
    MineViewJson {
        id: m.id.clone(),
        pos: vec2_to_json(m.pos),
        own: m.own,
    }
}

/// Convert an engine [`arena_engine::Event`] to [`EventJson`].
///
/// Returns `None` for event variants not yet mapped to PROTOCOL §7 JSON
/// (none currently — all engine variants are covered).
fn event_to_json(e: &arena_engine::Event) -> Option<EventJson> {
    use arena_engine::Event;
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
