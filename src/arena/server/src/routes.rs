//! axum [`Router`] and HTTP handlers for the Arena server (issue 02+).
//!
//! ## Current surface
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | `POST` | `/register` | event password in body | Register a team → token |
//! | `GET`  | `/ws`       | token in `join` message | WS Bot connect & play |
//! | `POST` | `/bots`     | `Authorization: Bearer <token>` | Upload a WASM Bot artifact |
//!
//! ## Seams for future issues
//!
//! - **Issue 05 (wasmtime host):** call `state.wasm_store.get(team)` to fetch
//!   the stored bytes and instantiate the WASM module before a match.
//! - **Issue 06 (connection resolver):** `state.wasm_store.get(team).is_some()`
//!   drives the WS → WASM → Default priority decision (ADR-0001).
//! - **Issue 11 (facilitator / Admin):** add a `facilitator_password: String`
//!   field to [`AppState`] and gate Admin-only routes on it. The Admin can call
//!   `state.wasm_store.store(team, bytes)` to upload/replace on behalf of a team.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{any, post},
    Json, Router,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use arena_engine::Params;

use crate::auth::TokenRegistry;
use crate::observer::{ObserverHub, ws_viewer_handler};
use crate::store::WasmBotStore;
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
/// - Issue 11: add `facilitator_password: String` here.
/// - Issue 07 (observer broadcast): `observer_hub` added here (this issue).
#[derive(Clone)]
pub struct AppState {
    /// Pre-shared event password configured at server startup.
    ///
    /// Participants supply this in `POST /register` to obtain a token.
    /// It is intentionally distinct from the facilitator password (issue 11).
    pub(crate) event_password: String,

    /// Token registry — shared with WS-join (issue 03) and WASM-upload (issue 04).
    pub registry: Arc<TokenRegistry>,

    /// WASM Bot artifact store — shared with the wasmtime host (issue 05),
    /// connection resolver (issue 06), and Admin (issue 11).
    pub wasm_store: Arc<WasmBotStore>,

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
}

// ── Router configuration ──────────────────────────────────────────────────────

/// Full configuration for [`build_router_config`].
///
/// Use when you need non-default match settings (e.g. in tests with a short
/// match and a fast deadline).
pub struct RouterConfig {
    pub event_password: String,
    pub registry: Arc<TokenRegistry>,
    /// WASM Bot artifact store.  Create with [`WasmBotStore::new`] and retain
    /// the `Arc` if tests need to inspect the store after requests.
    pub wasm_store: Arc<WasmBotStore>,
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
        registry,
        wasm_store: WasmBotStore::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params::default(),
        observer_hub: ObserverHub::new(),
    })
}

/// Build the axum [`Router`] with explicit match configuration.
///
/// Intended for tests that need a short match (`max_ticks: N`) and/or a fast
/// deadline so the test suite runs without real pacing sleeps.
pub fn build_router_config(config: RouterConfig) -> Router {
    let state = AppState {
        event_password: config.event_password,
        registry: config.registry,
        wasm_store: config.wasm_store,
        tick_deadline: config.tick_deadline,
        match_seed: config.match_seed,
        match_params: config.match_params,
        observer_hub: config.observer_hub,
    };
    Router::new()
        .route("/register", post(post_register))
        .route("/bots", post(post_bots))
        .route("/ws", any(ws_bot_handler))
        .route("/observe", any(ws_viewer_handler))
        .with_state(state)
}

