//! axum [`Router`] and HTTP handlers for the Arena server (issue 02+).
//!
//! ## Current surface
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | `POST` | `/register` | event password in body | Register a team → token |
//!
//! ## Seams for future issues
//!
//! - **Issue 03 (WS join):** mount the WebSocket upgrade handler here; validate
//!   the token from the `join` message via `AppState::registry.resolve`.
//! - **Issue 04 (WASM upload):** mount `POST /bots` here; extract the Bearer
//!   token from the `Authorization` header and call `registry.resolve` to
//!   identify the team before storing the WASM artifact.
//! - **Issue 11 (facilitator / Admin):** add a `facilitator_password: String`
//!   field to [`AppState`] and gate Admin-only routes on it. The event password
//!   and facilitator password remain separate credentials.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::TokenRegistry;

// ── App state ─────────────────────────────────────────────────────────────────

/// Shared application state threaded through all handlers via [`axum::extract::State`].
///
/// Constructed once in [`build_router`] and cloned cheaply per request (the
/// registry is `Arc<TokenRegistry>` and the password is a `String` clone).
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

    /// Token registry — shared with future WS-join and WASM-upload handlers.
    pub registry: Arc<TokenRegistry>,
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

// ── Router constructor ────────────────────────────────────────────────────────

/// Build the axum [`Router`] for the Arena server.
///
/// ## Parameters
///
/// - `event_password` — the pre-shared event password participants use at
///   `POST /register`. Configure this from an environment variable or config
///   file at startup; never hard-code it.
/// - `registry` — the [`TokenRegistry`] used to issue and resolve tokens.
///   Pass the same `Arc<TokenRegistry>` to the WS-join (issue 03) and
///   WASM-upload (issue 04) handlers so they can validate incoming tokens.
///
/// ## Testing
///
/// In tests, call `build_router(known_password, TokenRegistry::new())` and
/// drive requests via `tower::ServiceExt::oneshot` — no real TCP port needed.
pub fn build_router(event_password: String, registry: Arc<TokenRegistry>) -> Router {
    let state = AppState {
        event_password,
        registry,
    };
    Router::new()
        .route("/register", post(post_register))
        .with_state(state)
}
