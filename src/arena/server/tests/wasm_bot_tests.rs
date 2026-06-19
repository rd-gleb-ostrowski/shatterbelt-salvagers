//! Integration tests for the WASM Bot upload endpoint (issue 04).
//!
//! Tests assert **observable HTTP behaviour** and **observable store effects**
//! via `tower::ServiceExt::oneshot` — no real TCP port, no sleeping.
//!
//! The WASM artifact store is inspected through its public `get` method only;
//! no private internals are accessed.
//!
//! ## TDD order (RED → GREEN for each before moving to the next)
//!
//! 1. `upload_valid_token_and_wasm_returns_200_and_stores_bytes`
//!    — `POST /bots` with a valid token + WASM magic bytes → 200, store has bytes.
//! 2. `second_upload_replaces_first_artifact`
//!    — Re-uploading for the same team replaces the stored artifact.
//! 3. `upload_with_missing_auth_header_is_rejected`
//!    — No `Authorization` header → 401, nothing stored.
//! 4. `upload_with_invalid_token_is_rejected`
//!    — Unknown token in header → 401, nothing stored.
//! 5. `two_teams_uploads_are_stored_independently`
//!    — Each team's artifact is isolated; no cross-contamination.
//! 6. `upload_with_non_wasm_body_is_rejected`
//!    — Body lacking the `\0asm` magic → 400.
//! 7. `upload_with_empty_body_is_rejected`
//!    — Empty body → 400.

use std::sync::Arc;
use std::time::Duration;

use arena_engine::Params;
use arena_server::{
    auth::TokenRegistry,
    routes::{build_router_config, RouterConfig},
    store::WasmBotStore,
};
use axum::body::Body;
use http::{Request, StatusCode};
use tower::ServiceExt;

// ── Constants & helpers ───────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "test-secret";

/// Minimal valid WASM magic header (`\0asm` version 1).
const WASM_MAGIC: &[u8] = b"\x00asm\x01\x00\x00\x00";

/// Build the router and return it together with both shared handles so tests
/// can verify store effects and resolve tokens independently.
fn app_with_store() -> (axum::Router, Arc<TokenRegistry>, Arc<WasmBotStore>) {
    let registry = TokenRegistry::new();
    let store = WasmBotStore::new();
    let app = build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        registry: registry.clone(),
        wasm_store: store.clone(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params::default(),
        observer_hub: arena_server::observer::ObserverHub::new(),
        recording_store: arena_server::recording::RecordingStore::new(),
    });
    (app, registry, store)
}

/// Register a team via `POST /register` and return its token.
async fn register_team(app: axum::Router, team: &str) -> (axum::Router, String) {
    let req = Request::builder()
        .method("POST")
        .uri("/register")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "password": EVENT_PASSWORD, "team": team }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "registration must succeed");
    use http_body_util::BodyExt;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token = json["token"].as_str().unwrap().to_owned();
    (app, token)
}

/// `POST /bots` with raw bytes and an optional `Authorization` header value.
async fn post_bots(
    app: axum::Router,
    auth_header: Option<&str>,
    body: Vec<u8>,
) -> StatusCode {
    let mut builder = Request::builder().method("POST").uri("/bots");
    if let Some(auth) = auth_header {
        builder = builder.header("authorization", auth);
    }
    let req = builder.body(Body::from(body)).unwrap();
    app.oneshot(req).await.unwrap().status()
}

// ── Test 1: valid token + WASM bytes → 200 and artifact is stored ─────────────
//
// RED → GREEN: `POST /bots` handler exists, validates the token, stores bytes.
//
// Observable: HTTP 200 and `store.get(team)` returns the uploaded bytes.

#[tokio::test]
async fn upload_valid_token_and_wasm_returns_200_and_stores_bytes() {
    let (app, registry, store) = app_with_store();
    let (app, token) = register_team(app, "TeamAlpha").await;

    let status = post_bots(
        app,
        Some(&format!("Bearer {token}")),
        WASM_MAGIC.to_vec(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "valid upload must return 200");

    let team = registry.resolve(&token).expect("token must still resolve");
    let stored = store.get(&team).expect("artifact must be present in store");
    assert_eq!(stored, WASM_MAGIC, "stored bytes must match what was uploaded");
}

// ── Test 2: second upload replaces the first artifact ─────────────────────────
//
// RED → GREEN: re-upload for the same team atomically replaces the artifact.
//
// Observable: `store.get(team)` returns the *second* payload, not the first.

#[tokio::test]
async fn second_upload_replaces_first_artifact() {
    let (app, registry, store) = app_with_store();
    let (app, token) = register_team(app, "TeamBeta").await;

    let first_bytes: Vec<u8> = {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(b"first-payload");
        v
    };
    let second_bytes: Vec<u8> = {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(b"second-payload");
        v
    };

    let s1 = post_bots(
        app.clone(),
        Some(&format!("Bearer {token}")),
        first_bytes.clone(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);

    let s2 = post_bots(
        app,
        Some(&format!("Bearer {token}")),
        second_bytes.clone(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "re-upload must return 200");

    let team = registry.resolve(&token).unwrap();
    let stored = store.get(&team).expect("artifact must be present");
    assert_eq!(
        stored, second_bytes,
        "re-upload must replace the first artifact"
    );
    assert_ne!(stored, first_bytes, "first artifact must be overwritten");
}

// ── Test 3: absent Authorization header is rejected with 401 ──────────────────
//
// RED → GREEN: missing `Authorization` header returns 401, nothing stored.
//
// Observable: HTTP 401 and store remains empty for the team.

#[tokio::test]
async fn upload_with_missing_auth_header_is_rejected() {
    let (app, _registry, store) = app_with_store();

    let status = post_bots(app, None, WASM_MAGIC.to_vec()).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "absent auth header must yield 401"
    );
    // Store must be completely empty — no artifact for any team.
    // (We have no registered team; if the handler stored anything it would be a bug.)
    assert!(
        store.get("TeamGamma").is_none(),
        "nothing must be stored when auth is missing"
    );
}

// ── Test 4: invalid / unknown token is rejected with 401 ──────────────────────
//
// RED → GREEN: an unrecognised token returns 401, nothing stored.
//
// Observable: HTTP 401 and `store.get` returns `None` for the team.

#[tokio::test]
async fn upload_with_invalid_token_is_rejected() {
    let (app, registry, store) = app_with_store();
    // Register a team so we have a known team name but use a different token.
    let (app, real_token) = register_team(app, "TeamDelta").await;
    let team = registry.resolve(&real_token).unwrap();

    let status = post_bots(
        app,
        Some("Bearer totally-bogus-token"),
        WASM_MAGIC.to_vec(),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "unknown token must yield 401"
    );
    assert!(
        store.get(&team).is_none(),
        "nothing must be stored when token is invalid"
    );
}

// ── Test 5: two teams' artifacts are stored independently ────────────────────
//
// RED → GREEN: uploads for different teams don't cross-contaminate.
//
// Observable: each team's `store.get` returns only their own bytes.

#[tokio::test]
async fn two_teams_uploads_are_stored_independently() {
    let (app, registry, store) = app_with_store();
    let (app, token_echo) = register_team(app, "TeamEcho").await;
    let (app, token_foxtrot) = register_team(app, "TeamFoxtrot").await;

    let echo_bytes: Vec<u8> = {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(b"echo-bot");
        v
    };
    let foxtrot_bytes: Vec<u8> = {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(b"foxtrot-bot");
        v
    };

    let s1 = post_bots(
        app.clone(),
        Some(&format!("Bearer {token_echo}")),
        echo_bytes.clone(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);

    let s2 = post_bots(
        app,
        Some(&format!("Bearer {token_foxtrot}")),
        foxtrot_bytes.clone(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);

    let team_echo = registry.resolve(&token_echo).unwrap();
    let team_foxtrot = registry.resolve(&token_foxtrot).unwrap();

    assert_eq!(
        store.get(&team_echo).expect("TeamEcho must have artifact"),
        echo_bytes,
        "TeamEcho must have their own artifact"
    );
    assert_eq!(
        store.get(&team_foxtrot).expect("TeamFoxtrot must have artifact"),
        foxtrot_bytes,
        "TeamFoxtrot must have their own artifact"
    );
    assert_ne!(
        store.get(&team_echo).unwrap(),
        store.get(&team_foxtrot).unwrap(),
        "artifacts must not cross-contaminate"
    );
}

// ── Test 6: body lacking WASM magic bytes is rejected with 400 ────────────────
//
// RED → GREEN: body that doesn't start with `\0asm` returns 400.
//
// Observable: HTTP 400 and nothing stored.

#[tokio::test]
async fn upload_with_non_wasm_body_is_rejected() {
    let (app, registry, store) = app_with_store();
    let (app, token) = register_team(app, "TeamGolf").await;

    let status = post_bots(
        app,
        Some(&format!("Bearer {token}")),
        b"this is definitely not a wasm file".to_vec(),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "non-WASM body must yield 400"
    );
    let team = registry.resolve(&token).unwrap();
    assert!(
        store.get(&team).is_none(),
        "nothing must be stored when the body is not a WASM artifact"
    );
}

// ── Test 7: empty body is rejected with 400 ───────────────────────────────────
//
// RED → GREEN: empty body (missing magic bytes) returns 400.
//
// Observable: HTTP 400 and nothing stored.

#[tokio::test]
async fn upload_with_empty_body_is_rejected() {
    let (app, registry, store) = app_with_store();
    let (app, token) = register_team(app, "TeamHotel").await;

    let status = post_bots(app, Some(&format!("Bearer {token}")), vec![]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "empty body must yield 400");
    let team = registry.resolve(&token).unwrap();
    assert!(
        store.get(&team).is_none(),
        "nothing must be stored for an empty body"
    );
}
