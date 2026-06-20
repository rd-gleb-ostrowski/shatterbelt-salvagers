//! axum [`Router`] and HTTP handlers for the Arena server (issue 02+).
//!
//! ## Current surface
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | `POST` | `/register` | event password in body | Register a team → token |
//! | `GET`  | `/ws`       | token in `join` message | WS Bot connect & play |
//! | `POST` | `/bots`     | `Authorization: Bearer <token>` | Upload a WASM Bot artifact |
//! | `GET`  | `/recordings` | — | List all recorded matches |
//! | `POST` | `/recordings/{id}/replay` | — | Replay a recording through the observer hub |
//!
//! ## Seams for future issues
//!
//! - **Issue 05 (wasmtime host):** call `state.wasm_store.get(team)` to fetch
//!   the stored bytes and instantiate the WASM module before a match.
//! - **Issue 06 (connection resolver):** `state.wasm_store.get(team).is_some()`
//!   drives the WS → WASM → Default priority decision (ADR-0001).
//! - **Issue 10 (TrueSkill):** consume `state.recording_store.list()` winner +
//!   scores after each match to update ratings.
//! - **Issue 11 (facilitator / Admin):** add a `facilitator_password: String`
//!   field to [`AppState`] and gate Admin-only routes on it.  `GET /recordings`
//!   and `POST /recordings/{id}/replay` are already available as seams.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{any, delete, get, post},
    Json, Router,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use arena_engine::Params;

use crate::admin::{self, ExhibitionSupervisor, MatchRegistry};
use crate::auth::TokenRegistry;
use crate::health::{BotHealthStore, DqStore};
use crate::ladder::Ladder;
use crate::observer::{ObserverHub, ws_viewer_handler};
use crate::pacer::NoopPacer;
use crate::recording::RecordingStore;
use crate::replay::run_replay;
use crate::resolver::WsConnectionRegistry;
use crate::store::{DefaultBotStore, DisabledStore, WasmBotStore};
use crate::ws::ws_bot_handler;

// ── App state ─────────────────────────────────────────────────────────────────

/// Shared application state threaded through all handlers via [`axum::extract::State`].
///
/// Constructed via [`RouterConfig`] / [`build_router`]; cloned cheaply per
/// request (the registry and store are `Arc<…>`; all other fields are cheap
/// clones).
///
/// ## Adding future state
///
/// Extend this struct rather than reaching for global statics:
/// - Issue 10: read `recording_store` to update TrueSkill ratings.
/// - Issue 11: add `facilitator_password: String` here.
/// - Issue 07 (observer broadcast): `observer_hub` added here (this issue).
#[derive(Clone)]
pub struct AppState {
    /// Pre-shared event password configured at server startup.
    ///
    /// Participants supply this in `POST /register` to obtain a token.
    /// It is intentionally distinct from the facilitator password (issue 11).
    pub(crate) event_password: String,

    /// Pre-shared facilitator password (distinct from event_password) gating admin endpoints.
    /// PROTOCOL §4: a SEPARATE facilitator password gates match-control/admin/ladder/kicks.
    pub(crate) facilitator_password: String,

    /// Token registry — shared with WS-join (issue 03) and WASM-upload (issue 04).
    pub registry: Arc<TokenRegistry>,

    /// WASM Bot artifact store — shared with the wasmtime host (issue 05),
    /// connection resolver (issue 06), and Admin (issue 11).
    pub wasm_store: Arc<WasmBotStore>,

    /// Live WS bot connection registry — bots connect here before a match starts.
    pub ws_registry: Arc<WsConnectionRegistry>,

    /// Registry of currently-running live matches (by match_id).
    pub match_registry: Arc<MatchRegistry>,

    /// Exhibition supervisor — keeps one live match always running.
    pub exhibition: Arc<ExhibitionSupervisor>,

    /// Per-tick deadline for receiving an action from a WS bot.
    ///
    /// Default: **33 ms** (~30 Hz). Tests inject a shorter value (e.g. 50 ms)
    /// to keep the suite fast without real pacing sleeps.
    pub tick_deadline: Duration,

    /// RNG seed for new matches. Fixed in tests for determinism.
    pub match_seed: u64,

    /// Engine params for new matches.
    ///
    /// Tests use `Params { max_ticks: N, ..Params::default() }` for short matches.
    pub match_params: Params,

    /// God-mode observer broadcast hub (issue 07).
    ///
    /// The match tick loop calls `observer_hub.publish_god_view()` after each
    /// tick; Viewer WS clients subscribe via `GET /observe`.
    ///
    /// ## Seam: issue 11 (admin projector)
    ///
    /// The admin replaces this hub (or swaps which match feeds it) to push a
    /// specific match to the projector.
    pub observer_hub: ObserverHub,

    /// In-memory store of finished-match recordings (issue 08).
    ///
    /// Every match completion appends a [`Recording`](crate::recording::Recording)
    /// here.  The HTTP handler `GET /recordings` lists them; `POST
    /// /recordings/{id}/replay` replays one through the observer hub.
    ///
    /// ## Seam: issue 10 (TrueSkill ladder)
    ///
    /// After a match finishes, read `winner` + `scores` from the stored
    /// [`RecordingMeta`](crate::recording::RecordingMeta) to drive rating updates.
    ///
    /// ## Seam: issue 11 (Admin UI)
    ///
    /// Expose `recording_store.list()` and `recording_store.get(id)` via
    /// admin-gated HTTP endpoints for download / replay.
    pub recording_store: Arc<RecordingStore>,

    /// Per-bot health registry (issue 12).
    ///
    /// Populated by the resolver at match start.  Read by `GET /admin/bots`.
    pub health_store: Arc<BotHealthStore>,

    /// Disqualified teams (issue 12).
    ///
    /// Written by `POST /admin/bots/{team}/kick`.  Shared with the resolver
    /// and every live match's [`ExclusionDriver`](crate::health::ExclusionDriver).
    pub dq_store: Arc<DqStore>,

    /// TrueSkill ladder (issue 10).
    ///
    /// Updated by every finished match (headless and live).  Exposed via
    /// `GET /ladder/standings` (public) and `POST /ladder/reset` (facilitator-gated).
    pub ladder: Arc<Ladder>,

    /// Reversible bot-disable set (issue 13).
    ///
    /// Written by `POST /admin/bots/{team}/disable` and cleared by `.../enable`.
    /// Shared with the resolver — disabled teams fall back to Default Bot.
    pub disabled_store: Arc<DisabledStore>,

    /// Custom Default Bot artifact (issue 13).
    ///
    /// When set, resolver Priority-3 instantiates a WASM driver from this
    /// artifact instead of the built-in heuristic.
    pub default_bot_store: Arc<DefaultBotStore>,
}

// ── Router configuration ──────────────────────────────────────────────────────

/// Full configuration for [`build_router_config`].
///
/// Use when you need non-default match settings (e.g. in tests with a short
/// match and a fast deadline).
pub struct RouterConfig {
    pub event_password: String,
    pub facilitator_password: String,
    pub registry: Arc<TokenRegistry>,
    /// WASM Bot artifact store.  Create with [`WasmBotStore::new`] and retain
    /// the `Arc` if tests need to inspect the store after requests.
    pub wasm_store: Arc<WasmBotStore>,
    pub ws_registry: Arc<WsConnectionRegistry>,
    /// Per-tick deadline. Defaults to 33 ms in [`build_router`].
    pub tick_deadline: Duration,
    /// Match RNG seed. Defaults to 42 in [`build_router`].
    pub match_seed: u64,
    /// Engine params. Defaults to [`Params::default`] in [`build_router`].
    pub match_params: Params,
    /// God-mode observer hub. Defaults to a fresh [`ObserverHub`] in [`build_router`].
    ///
    /// Retain a clone before passing to [`build_router_config`] if you need to
    /// subscribe to the hub directly in tests.
    pub observer_hub: ObserverHub,
    /// Recording store. Defaults to a fresh [`RecordingStore`] in [`build_router`].
    ///
    /// Retain the `Arc` clone before passing to [`build_router_config`] if you
    /// need to inspect recordings after a match in tests.
    pub recording_store: Arc<RecordingStore>,
    /// Bot health store. Defaults to a fresh [`BotHealthStore`] in [`build_router`].
    pub health_store: Arc<BotHealthStore>,
    /// Disqualification store. Defaults to a fresh [`DqStore`] in [`build_router`].
    pub dq_store: Arc<DqStore>,
    /// TrueSkill ladder.  Defaults to a fresh [`Ladder`] in [`build_router`].
    ///
    /// Retain the `Arc` clone before passing to [`build_router_config`] if you
    /// need to inspect or seed the ladder in tests.
    pub ladder: Arc<Ladder>,
    /// Reversible disabled-team set.  Defaults to a fresh [`DisabledStore`] in [`build_router`].
    pub disabled_store: Arc<DisabledStore>,
    /// Custom Default Bot artifact.  Defaults to a fresh (empty) [`DefaultBotStore`] in [`build_router`].
    pub default_bot_store: Arc<DefaultBotStore>,
}

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    password: String,
    team: String,
}

#[derive(Serialize)]
struct RegisterOk {
    token: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /register` — exchange the event password + team name for a token.
///
/// - **200 OK** `{"token": "…"}` — password correct; token issued (or re-issued).
/// - **401 Unauthorized** `{"error": "invalid password"}` — wrong password.
/// - **422 Unprocessable Entity** — missing/malformed JSON body (axum default).
async fn post_register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> impl IntoResponse {
    if body.password != state.event_password {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid password"})),
        )
            .into_response();
    }

    if body.team.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "team name must not be blank"})),
        )
            .into_response();
    }

    let token = state.registry.register(&body.team);
    (StatusCode::OK, Json(RegisterOk { token })).into_response()
}

/// `POST /bots` — upload (or replace) the WASM Bot artifact for a team.
///
/// # Auth
///
/// The participant's token (obtained from `POST /register`) must be supplied as:
/// ```text
/// Authorization: Bearer <token>
/// ```
/// This is consistent with the WS `join` path — both use the token issued at
/// registration to identify the team.
///
/// # Request body
///
/// Raw bytes of a compiled `.wasm` artifact.  The first four bytes must be the
/// WASM magic header (`\0asm` = `[0x00, 0x61, 0x73, 0x6d]`); anything else is
/// rejected as a bad request.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Artifact stored (or replaced) for the team. |
/// | **400 Bad Request** | Body is empty or does not start with the WASM magic bytes. |
/// | **401 Unauthorized** | `Authorization` header absent, malformed, or token unknown/revoked. |
async fn post_bots(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // ── 1. Extract token from `Authorization: Bearer <token>` ─────────────────
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "missing or malformed Authorization header; expected: Bearer <token>"})),
            )
                .into_response();
        }
    };

    // ── 2. Resolve token → team identity ──────────────────────────────────────
    let team = match state.registry.resolve(&token) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid or revoked token"})),
            )
                .into_response();
        }
    };

    // ── 3. Validate WASM magic bytes ───────────────────────────────────────────
    // The first four bytes of every valid WebAssembly module are `\0asm`.
    // Reject anything that doesn't look like a WASM artifact so teams get an
    // early signal rather than a cryptic error later at instantiation time.
    if body.len() < 4 || &body[..4] != b"\0asm" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "body must be a valid WASM artifact (missing magic bytes \\0asm)"})),
        )
            .into_response();
    }

    // ── 4. Store the artifact ──────────────────────────────────────────────────
    state.wasm_store.store(&team, body.to_vec());

    StatusCode::OK.into_response()
}

// ── Recording handlers ────────────────────────────────────────────────────────

/// JSON response shape for a single recording entry in `GET /recordings`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingListItem {
    match_id: String,
    seed: u64,
    tick_count: u32,
    winner: Option<String>,
}

/// `GET /recordings` — list all finished-match recordings.
///
/// Returns a JSON array of lightweight metadata objects.  The `matchId` can
/// be used with `POST /recordings/{id}/replay` to replay a match.
///
/// ## Seam: issue 11 (Admin UI)
///
/// Gate this behind `Authorization: Facilitator <password>` in issue 11.
async fn get_recordings(State(state): State<AppState>) -> impl IntoResponse {
    let items: Vec<RecordingListItem> = state
        .recording_store
        .list()
        .into_iter()
        .map(|m| RecordingListItem {
            match_id: m.match_id,
            seed: m.seed,
            tick_count: m.tick_count,
            winner: m.winner,
        })
        .collect();
    (StatusCode::OK, Json(items))
}

/// `POST /recordings/{id}/replay` — replay a recorded match through the observer hub.
///
/// Reconstructs the match from its stored seed + intent log via
/// [`arena_engine::harness::replay_match`] and publishes every tick's god-view
/// frame to the [`ObserverHub`].  Viewers subscribed to `/observe` will receive
/// the replay frames in real time.
///
/// Uses [`NoopPacer`] (instant replay) so the HTTP handler returns quickly.
/// For real-time replay at 30 Hz, issue 11 can spawn a background task with
/// [`LivePacer`](crate::pacer::LivePacer).
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Replay completed; frames published to observer hub. |
/// | **404 Not Found** | No recording with the given `id`. |
async fn post_replay(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let recording = match state.recording_store.get(&id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "recording not found"})),
            )
                .into_response();
        }
    };
    run_replay(&recording, &state.observer_hub, Box::new(NoopPacer));
    StatusCode::OK.into_response()
}

/// Extract a Bearer token from the `Authorization` header.
///
/// Returns `None` if the header is absent, not valid UTF-8, or not of the form
/// `Bearer <token>`.
fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_owned)
}

// ── Ladder handlers ───────────────────────────────────────────────────────────

/// JSON response shape for a single ladder entry in `GET /ladder/standings`.
///
/// Fields use **camelCase** as per the API contract documented in `ADR-0005`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LadderEntryDto {
    competitor: String,
    mu: f64,
    sigma: f64,
    conservative_skill: f64,
    matches: u32,
}

/// `GET /ladder/standings` — public read-only standings endpoint.
///
/// Returns a JSON array of competitors ordered by **conservative skill**
/// (μ − 3σ) descending.  No authentication required — the Viewer's ladder
/// panel calls this directly.
///
/// ## Response shape
///
/// ```json
/// [
///   { "competitor": "alpha", "mu": 27.1, "sigma": 7.5,
///     "conservativeSkill": 4.6, "matches": 3 },
///   …
/// ]
/// ```
async fn get_ladder_standings(State(state): State<AppState>) -> impl IntoResponse {
    let entries: Vec<LadderEntryDto> = state
        .ladder
        .standings()
        .into_iter()
        .map(|e| LadderEntryDto {
            conservative_skill: e.conservative_skill(),
            competitor: e.competitor,
            mu: e.mu,
            sigma: e.sigma,
            matches: e.matches,
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

/// `POST /ladder/reset` — facilitator-gated ladder wipe.
///
/// Clears all TrueSkill ratings and match counts, returning the ladder to its
/// empty initial state.  Useful for starting a fresh competition without
/// restarting the server.
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Ladder cleared. |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
async fn post_ladder_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = admin::check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    state.ladder.reset();
    StatusCode::OK.into_response()
}

// ── Router constructors ───────────────────────────────────────────────────────

/// Build the axum [`Router`] for the Arena server with default WS match settings.
///
/// ## Parameters
///
/// - `event_password` — the pre-shared event password participants use at
///   `POST /register`. Configure from an environment variable at startup;
///   never hard-code it.
/// - `registry` — the [`TokenRegistry`] used to issue and resolve tokens.
///   Pass the same `Arc<TokenRegistry>` to the WS-join (issue 03) and
///   WASM-upload (issue 04) handlers so they can validate incoming tokens.
///
/// A fresh [`WasmBotStore`] is created internally. If you need to retain a
/// reference to the store (e.g. in tests that inspect stored artifacts), use
/// [`build_router_config`] with a [`RouterConfig`] that carries a pre-created
/// store.
///
/// ## Testing
///
/// In tests for HTTP-only behavior, call `build_router(known_password,
/// TokenRegistry::new())` and drive requests via `tower::ServiceExt::oneshot`
/// — no real TCP port needed.
///
/// For WS tests that need a short match or fast deadline, use
/// [`build_router_config`] with a [`RouterConfig`] instead.
pub fn build_router(event_password: String, registry: Arc<TokenRegistry>) -> Router {
    build_router_config(RouterConfig {
        event_password,
        facilitator_password: String::new(),
        registry,
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params::default(),
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        health_store: BotHealthStore::new(),
        dq_store: DqStore::new(),
        ladder: Ladder::new(),
        disabled_store: DisabledStore::new(),
        default_bot_store: DefaultBotStore::new(),
    })
}

/// Build the axum [`Router`] with explicit match configuration.
///
/// Intended for tests that need a short match (`max_ticks: N`) and/or a fast
/// deadline so the test suite runs without real pacing sleeps.
pub fn build_router_config(config: RouterConfig) -> Router {
    let state = AppState {
        event_password: config.event_password,
        facilitator_password: config.facilitator_password,
        registry: config.registry,
        wasm_store: config.wasm_store,
        ws_registry: config.ws_registry,
        match_registry: MatchRegistry::new(),
        exhibition: ExhibitionSupervisor::new(),
        tick_deadline: config.tick_deadline,
        match_seed: config.match_seed,
        match_params: config.match_params,
        observer_hub: config.observer_hub,
        recording_store: config.recording_store,
        health_store: config.health_store,
        dq_store: config.dq_store,
        ladder: config.ladder,
        disabled_store: config.disabled_store,
        default_bot_store: config.default_bot_store,
    };
    Router::new()
        .route("/register", post(post_register))
        .route("/bots", post(post_bots))
        .route("/ws", any(ws_bot_handler))
        .route("/observe", any(ws_viewer_handler))
        .route("/recordings", get(get_recordings))
        .route("/recordings/{id}/replay", post(post_replay))
        .route("/admin/matches", post(admin::post_admin_start_match))
        .route(
            "/admin/matches/{id}/pause",
            post(admin::post_admin_pause_match),
        )
        .route(
            "/admin/matches/{id}/resume",
            post(admin::post_admin_resume_match),
        )
        .route("/admin/matches/{id}/tps", post(admin::post_admin_set_tps))
        .route("/admin/matches/{id}", delete(admin::delete_admin_match))
        .route("/admin/exhibition", get(admin::get_admin_exhibition))
        .route(
            "/admin/exhibition/start",
            post(admin::post_admin_exhibition_start),
        )
        .route(
            "/admin/exhibition/stop",
            post(admin::post_admin_exhibition_stop),
        )
        // ── Issue 12: health & moderation ─────────────────────────────────
        .route("/admin/bots", get(admin::get_admin_bots))
        .route("/admin/bots/{team}/kick", post(admin::post_admin_kick_bot))
        // ── Issue 13: bot/team management ─────────────────────────────────
        .route("/admin/bots/{team}", post(admin::post_admin_upload_bot))
        .route(
            "/admin/bots/{team}/disable",
            post(admin::post_admin_disable_bot),
        )
        .route(
            "/admin/bots/{team}/enable",
            post(admin::post_admin_enable_bot),
        )
        .route("/admin/default-bot", post(admin::post_admin_set_default_bot))
        .route(
            "/admin/default-bot",
            delete(admin::delete_admin_default_bot),
        )
        // ── Issue 10: TrueSkill ladder ────────────────────────────────────
        .route("/ladder/standings", get(get_ladder_standings))
        .route("/ladder/reset", post(post_ladder_reset))
        .with_state(state)
}

