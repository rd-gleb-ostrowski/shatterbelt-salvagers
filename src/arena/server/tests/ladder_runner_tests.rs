//! TDD tests for:
//!   (A) Headless ladder runner control (`/admin/ladder/runner` endpoints)
//!   (B) Recording download (`/admin/recordings/{id}/download`)
//!
//! ## TDD tracer order
//!
//! 1. `get_runner_status_without_auth_returns_401`
//! 2. `get_runner_status_with_auth_fresh_server_returns_not_running`
//! 3. `start_runner_then_status_shows_running`
//! 4. `stop_runner_then_status_shows_not_running`
//! 5. `start_stop_without_auth_return_401`
//! 6. `download_recording_without_auth_returns_401`
//! 7. `download_recording_unknown_id_returns_404`
//! 8. `download_recording_returns_seed_and_meta`

use std::sync::Arc;
use std::time::Duration;

use arena_engine::{Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    auth::TokenRegistry,
    health::{BotHealthStore, DqStore},
    ladder::Ladder,
    observer::ObserverHub,
    recording::{Recording, RecordingMeta, RecordingStore},
    resolver::WsConnectionRegistry,
    routes::{build_router_config, RouterConfig},
    store::{DefaultBotStore, DisabledStore, WasmBotStore},
};
use axum::body::Body;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const EVENT_PASSWORD: &str = "test-event";
const FACILITATOR_PASSWORD: &str = "test-facilitator";
const FAC_HEADER: &str = "Facilitator test-facilitator";

/// Build a minimal test router with short match params so tests run fast.
fn test_app() -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry: TokenRegistry::new(),
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params { max_ticks: 5, ..Params::default() },
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        health_store: BotHealthStore::new(),
        dq_store: DqStore::new(),
        ladder: Ladder::new(),
        disabled_store: DisabledStore::new(),
        default_bot_store: DefaultBotStore::new(),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    })
}

/// Build a test router with a pre-populated recording store.
fn test_app_with_store(recording_store: Arc<RecordingStore>) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry: TokenRegistry::new(),
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params { max_ticks: 5, ..Params::default() },
        observer_hub: ObserverHub::new(),
        recording_store,
        health_store: BotHealthStore::new(),
        dq_store: DqStore::new(),
        ladder: Ladder::new(),
        disabled_store: DisabledStore::new(),
        default_bot_store: DefaultBotStore::new(),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    })
}

async fn send(app: axum::Router, method: Method, uri: &str, auth: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(a) = auth {
        builder = builder.header("authorization", a);
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

// ── Test 1: GET /admin/ladder/runner without auth → 401 ──────────────────────

#[tokio::test]
async fn get_runner_status_without_auth_returns_401() {
    let (status, _) = send(test_app(), Method::GET, "/admin/ladder/runner", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ── Test 2: GET /admin/ladder/runner with auth, fresh server → running:false ──

#[tokio::test]
async fn get_runner_status_with_auth_fresh_server_returns_not_running() {
    let (status, body) = send(test_app(), Method::GET, "/admin/ladder/runner", Some(FAC_HEADER)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["running"], Value::Bool(false));
}

// ── Test 3: POST start → 200, GET → running:true ─────────────────────────────

#[tokio::test]
async fn start_runner_then_status_shows_running() {
    let app = test_app();
    // Start the runner
    let (start_status, _) = send(app.clone(), Method::POST, "/admin/ladder/runner/start", Some(FAC_HEADER)).await;
    assert_eq!(start_status, StatusCode::OK);

    // Status should now report running:true
    let (status, body) = send(app.clone(), Method::GET, "/admin/ladder/runner", Some(FAC_HEADER)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["running"], Value::Bool(true));

    // Cleanup: stop to prevent background task from running forever
    let _ = send(app, Method::POST, "/admin/ladder/runner/stop", Some(FAC_HEADER)).await;
}

// ── Test 4: POST stop after start → 200, GET → running:false ─────────────────

#[tokio::test]
async fn stop_runner_then_status_shows_not_running() {
    let app = test_app();
    // Start first
    let _ = send(app.clone(), Method::POST, "/admin/ladder/runner/start", Some(FAC_HEADER)).await;
    // Then stop
    let (stop_status, _) = send(app.clone(), Method::POST, "/admin/ladder/runner/stop", Some(FAC_HEADER)).await;
    assert_eq!(stop_status, StatusCode::OK);

    // Status should now report running:false
    let (status, body) = send(app, Method::GET, "/admin/ladder/runner", Some(FAC_HEADER)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["running"], Value::Bool(false));
}

// ── Test 5: POST start/stop without auth → 401 ───────────────────────────────

#[tokio::test]
async fn start_stop_without_auth_return_401() {
    let app = test_app();
    let (start_status, _) = send(app.clone(), Method::POST, "/admin/ladder/runner/start", None).await;
    assert_eq!(start_status, StatusCode::UNAUTHORIZED);

    let (stop_status, _) = send(app, Method::POST, "/admin/ladder/runner/stop", None).await;
    assert_eq!(stop_status, StatusCode::UNAUTHORIZED);
}

// ── Test 6: GET /admin/recordings/{id}/download without auth → 401 ───────────

#[tokio::test]
async fn download_recording_without_auth_returns_401() {
    let (status, _) = send(test_app(), Method::GET, "/admin/recordings/any-id/download", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ── Test 7: GET /admin/recordings/{id}/download with auth + unknown id → 404 ──

#[tokio::test]
async fn download_recording_unknown_id_returns_404() {
    let (status, _) = send(
        test_app(),
        Method::GET,
        "/admin/recordings/no-such-id/download",
        Some(FAC_HEADER),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Test 8: download a real recording → 200 with seed + meta ─────────────────

#[tokio::test]
async fn download_recording_returns_seed_and_meta() {
    // Seed the recording store with a known recording
    let store = RecordingStore::new();
    let match_id = "test-dl-match".to_owned();
    let seed = 77_u64;
    let scores = vec![
        ("ship-0".to_owned(), 100.0_f32),
        ("ship-1".to_owned(), 50.0_f32),
    ];
    let params = Params { max_ticks: 5, ..Params::default() };
    let specs = vec![
        ShipSpec {
            id: "ship-0".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.25, params.arena_h * 0.5),
        },
        ShipSpec {
            id: "ship-1".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.75, params.arena_h * 0.5),
        },
    ];
    store.record(Recording {
        match_id: match_id.clone(),
        seed,
        params,
        specs,
        intent_log: vec![],
        meta: RecordingMeta {
            match_id: match_id.clone(),
            seed,
            tick_count: 5,
            winner: Some("ship-0".to_owned()),
            scores: scores.clone(),
        },
    });

    let app = test_app_with_store(store);
    let uri = format!("/admin/recordings/{match_id}/download");
    let (status, body) = send(app, Method::GET, &uri, Some(FAC_HEADER)).await;

    assert_eq!(status, StatusCode::OK, "expected 200, got {status}; body: {body}");

    // The response is now the full Recording (snake_case), not the old metadata-only DTO.
    assert_eq!(body["match_id"], Value::String(match_id));
    assert_eq!(body["seed"], Value::Number(serde_json::Number::from(seed)));
    assert_eq!(body["meta"]["tick_count"], Value::Number(serde_json::Number::from(5_u32)));
    assert_eq!(body["meta"]["winner"], Value::String("ship-0".to_owned()));

    // Full recording also includes specs, params, intent_log.
    assert!(body["specs"].is_array(), "full recording must include specs");
    assert!(body["params"].is_object(), "full recording must include params");
    assert!(body["intent_log"].is_array(), "full recording must include intent_log");

    // Scores in meta: [[shipId, score], ...]
    let scores_val = &body["meta"]["scores"];
    assert!(scores_val.is_array(), "meta.scores must be an array");
    assert_eq!(scores_val.as_array().unwrap().len(), 2);
}

// ── Bonus: start when already running is idempotent (no crash, still running) ─

#[tokio::test]
async fn start_when_already_running_is_idempotent() {
    let app = test_app();
    // Start twice
    let (s1, _) = send(app.clone(), Method::POST, "/admin/ladder/runner/start", Some(FAC_HEADER)).await;
    assert_eq!(s1, StatusCode::OK);
    let (s2, _) = send(app.clone(), Method::POST, "/admin/ladder/runner/start", Some(FAC_HEADER)).await;
    assert_eq!(s2, StatusCode::OK);

    // Still reports running
    let (_, body) = send(app.clone(), Method::GET, "/admin/ladder/runner", Some(FAC_HEADER)).await;
    assert_eq!(body["running"], Value::Bool(true));

    // Cleanup
    let _ = send(app, Method::POST, "/admin/ladder/runner/stop", Some(FAC_HEADER)).await;
}
