//! axum [`Router`] and HTTP handlers for the Arena server (issue 02+).
//!
//! ## Current surface
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | `POST` | `/register` | event password in body | Register a team → token |
//! | `GET`  | `/ws`       | token in `join` message | WS Bot connect & play |
//!
//! ## Seams for future issues
//!
//! - **Issue 04 (WASM upload):** mount `POST /bots` here; extract the Bearer
//!   token from the `Authorization` header and call `registry.resolve` to
//!   identify the team before storing the WASM artifact.
//! - **Issue 11 (facilitator / Admin):** add a `facilitator_password: String`
//!   field to [`AppState`] and gate Admin-only routes on it. The event password
//!   and facilitator password remain separate credentials.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{any, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use arena_engine::Params;

use crate::auth::TokenRegistry;
use crate::ws::ws_bot_handler;

// ── App state ─────────────────────────────────────────────────────────────────

/// Shared application state threaded through all handlers via [`axum::extract::State`].
///
/// Constructed via [`RouterConfig`] / [`build_router`]; cloned cheaply per
/// request (the registry is `Arc<TokenRegistry>`; all other fields are cheap
/// clones).
///
/// ## Adding future state
///
/// Extend this struct rather than reaching for global statics:
/// - Issue 11: add `facilitator_password: String` here.
/// - Issue 07 (observer broadcast): add a `tokio::sync::broadcast::Sender` here.
#[derive(Clone)]
pub struct AppState {
    /// Pre-shared event password configured at server startup.
    ///
    /// Participants supply this in `POST /register` to obtain a token.
    /// It is intentionally distinct from the facilitator password (issue 11).
    pub(crate) event_password: String,

    /// Token registry — shared with WS-join (issue 03) and WASM-upload (issue 04).
    pub registry: Arc<TokenRegistry>,

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
}

// ── Router configuration ──────────────────────────────────────────────────────

/// Full configuration for [`build_router_config`].
///
/// Use when you need non-default match settings (e.g. in tests with a short
/// match and a fast deadline).
pub struct RouterConfig {
    pub event_password: String,
    pub registry: Arc<TokenRegistry>,
    /// Per-tick deadline. Defaults to 33 ms in [`build_router`].
    pub tick_deadline: Duration,
    /// Match RNG seed. Defaults to 42 in [`build_router`].
    pub match_seed: u64,
    /// Engine params. Defaults to [`Params::default`] in [`build_router`].
    pub match_params: Params,
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
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params::default(),
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
        tick_deadline: config.tick_deadline,
        match_seed: config.match_seed,
        match_params: config.match_params,
    };
    Router::new()
        .route("/register", post(post_register))
        .route("/ws", any(ws_bot_handler))
        .with_state(state)
}
