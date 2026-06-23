use std::{collections::HashMap, sync::Arc, time::Duration};

use arena_engine::{Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    admin::{
        spawn_live_match, ExhibitionConfig, ExhibitionSupervisor, MatchControlHandle, MatchRegistry,
    },
    auth::TokenRegistry,
    observer::ObserverHub,
    pacer::NoopPacer,
    recording::RecordingStore,
    resolver::WsConnectionRegistry,
    routes::{build_router_config, RouterConfig},
    store::WasmBotStore,
};
use axum::body::Body;
use futures_util::future::join_all;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const EVENT_PASSWORD: &str = "test-event";
const FACILITATOR_PASSWORD: &str = "test-facilitator";

fn two_specs(params: &Params) -> Vec<ShipSpec> {
    vec![
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
    ]
}

fn teams() -> Vec<String> {
    vec!["team-a".to_owned(), "team-b".to_owned()]
}

fn test_app_with_params(
    recording_store: Arc<RecordingStore>,
    observer_hub: ObserverHub,
    params: Params,
) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry: TokenRegistry::new(),
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: params,
        observer_hub,
        recording_store,
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
        ladder: arena_server::ladder::Ladder::new(),
        disabled_store: arena_server::store::DisabledStore::new(),
        default_bot_store: arena_server::store::DefaultBotStore::new(),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    })
}

fn test_app(recording_store: Arc<RecordingStore>, observer_hub: ObserverHub) -> axum::Router {
    test_app_with_params(
        recording_store,
        observer_hub,
        Params {
            max_ticks: 5,
            ..Params::default()
        },
    )
}

async fn request_json(
    app: axum::Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<Value>,
) -> http::Response<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(auth) = auth {
        builder = builder.header("authorization", auth);
    }
    let body = match body {
        Some(body) => {
            builder = builder.header("content-type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    app.oneshot(builder.body(body).unwrap()).await.unwrap()
}

async fn register_teams(app: axum::Router, teams: &[String]) -> HashMap<String, String> {
    join_all(teams.iter().map(async |team| {
        let r = request_json(
            app.clone(),
            Method::POST,
            "/register",
            None,
            Some(serde_json::json!({ "password": EVENT_PASSWORD, "team": team })),
        )
        .await;
        let token = response_json(r).await.get("token").unwrap().to_string();
        (team.clone(), token)
    }))
    .await
    .into_iter()
    .collect()
}

async fn post_admin_matches(
    app: axum::Router,
    auth: Option<&str>,
    body: Value,
) -> http::Response<Body> {
    request_json(app, Method::POST, "/admin/matches", auth, Some(body)).await
}

async fn response_json(resp: http::Response<Body>) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn admin_endpoint_rejects_wrong_facilitator_password() {
    let app = test_app(RecordingStore::new(), ObserverHub::new());

    let resp = post_admin_matches(
        app,
        Some("Facilitator wrong-password"),
        serde_json::json!({ "mode": "headless" }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_endpoint_rejects_absent_facilitator_password() {
    let app = test_app(RecordingStore::new(), ObserverHub::new());

    let resp = post_admin_matches(app, None, serde_json::json!({ "mode": "headless" })).await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn start_headless_match_returns_match_id() {
    let recording_store = RecordingStore::new();
    let app = test_app(recording_store.clone(), ObserverHub::new());
    let teams = teams();
    register_teams(app.clone(), &teams).await;
    let resp = post_admin_matches(
        app,
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        serde_json::json!({ "mode": "headless", "seed": 1, "maxTicks": 5 }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let match_id = json["matchId"].as_str().expect("matchId string");
    assert!(
        recording_store.get(match_id).is_some(),
        "headless match is recorded"
    );
}

#[tokio::test]
async fn start_live_match_returns_match_id_and_feeds_observer() {
    let recording_store = RecordingStore::new();
    let observer_hub = ObserverHub::new();
    let mut observer_rx = observer_hub.subscribe();
    let app = test_app(recording_store, observer_hub);
    let teams = teams();
    register_teams(app.clone(), &teams).await;
    let resp = post_admin_matches(
        app,
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        serde_json::json!({ "mode": "live", "seed": 1, "maxTicks": 5, "tps": 0 }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert!(json["matchId"].as_str().is_some(), "matchId string");
    let frame = tokio::time::timeout(Duration::from_millis(500), observer_rx.recv())
        .await
        .expect("observer frame")
        .expect("broadcast frame");
    assert!(frame.contains("godView"));
}

#[tokio::test]
async fn pause_sets_paused_state_on_handle() {
    let params = Params {
        max_ticks: 20,
        ..Params::default()
    };
    let handle = MatchControlHandle::new(0);
    handle.pause();
    assert!(handle.is_paused());

    let join = spawn_live_match(
        7,
        params.clone(),
        two_specs(&params),
        teams(),
        WasmBotStore::new(),
        WsConnectionRegistry::new(),
        10_000_000,
        ObserverHub::new(),
        RecordingStore::new(),
        handle.clone(),
        Box::new(NoopPacer),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(
        handle.tick_count(),
        0,
        "ticks must not advance while paused"
    );
    handle.resume();
    let (_, outcome) = tokio::time::timeout(Duration::from_secs(1), join)
        .await
        .expect("match finishes")
        .expect("join succeeds");
    assert_eq!(outcome.ticks, 20);
    assert!(handle.tick_count() > 0);
}

#[tokio::test]
async fn resume_unpauses_match() {
    let handle = MatchControlHandle::new(30);
    handle.pause();
    handle.resume();
    assert!(!handle.is_paused());
}

#[tokio::test]
async fn set_tps_changes_handle_tps_value() {
    let handle = MatchControlHandle::new(30);
    handle.set_tps(60);
    assert_eq!(handle.tps(), 60);
}

#[tokio::test]
async fn abort_stops_live_match() {
    let recording_store = RecordingStore::new();
    let observer_hub = ObserverHub::new();
    let app = test_app_with_params(
        recording_store,
        observer_hub,
        Params {
            max_ticks: 100,
            ..Params::default()
        },
    );
    let teams = teams();
    register_teams(app.clone(), &teams).await;

    let start = post_admin_matches(
        app.clone(),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        serde_json::json!({ "mode": "live", "seed": 1, "maxTicks": 100, "tps": 1 }),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let json = response_json(start).await;
    let match_id = json["matchId"].as_str().expect("matchId").to_owned();

    let delete = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/admin/matches/{match_id}"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;
    assert_eq!(delete.status(), StatusCode::OK);

    let pause_after_delete = request_json(
        app,
        Method::POST,
        &format!("/admin/matches/{match_id}/pause"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;
    assert_eq!(pause_after_delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn exhibition_supervisor_restarts_match_on_completion() {
    let params = Params {
        max_ticks: 5,
        ..Params::default()
    };
    let supervisor = ExhibitionSupervisor::new();
    supervisor.start(ExhibitionConfig {
        seed: 1,
        params: params.clone(),
        teams: teams(),
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        registry: TokenRegistry::new(),
        fuel_per_tick: 10_000_000,
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        tps: 0,
        max_matches: 2,
        pacer_factory: Arc::new(|_| Box::new(NoopPacer)),
    });

    tokio::time::timeout(Duration::from_millis(500), supervisor.join())
        .await
        .expect("supervisor finishes");
    assert_eq!(supervisor.match_count(), 2);
}

#[tokio::test]
async fn event_password_does_not_grant_admin_access() {
    let registry = MatchRegistry::new();
    let handle = MatchControlHandle::new(30);
    registry.register("match".to_owned(), handle.clone());
    assert!(registry.get("match").is_some());
    handle.abort();
    assert!(handle.is_aborted());
    registry.remove("match");
    assert!(registry.get("match").is_none());

    let app = test_app(RecordingStore::new(), ObserverHub::new());
    let resp = post_admin_matches(
        app,
        Some(&format!("Bearer {EVENT_PASSWORD}")),
        serde_json::json!({ "mode": "headless" }),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════════
// B2b — full download + import tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Run a headless match via the HTTP API and return the match_id.
async fn run_headless_match(app: axum::Router) -> String {
    let teams = teams();
    register_teams(app.clone(), &teams).await;
    let resp = post_admin_matches(
        app,
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        serde_json::json!({ "mode": "headless", "seed": 55, "maxTicks": 10 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "headless match must start");
    let json = response_json(resp).await;
    json["matchId"].as_str().expect("matchId string").to_owned()
}

// ── (3) GET /admin/recordings/{id}/download returns full Recording JSON ───────

#[tokio::test]
async fn download_returns_full_recording_json() {
    let store = RecordingStore::new();
    let app = test_app(store.clone(), ObserverHub::new());

    let match_id = run_headless_match(app.clone()).await;

    let resp = request_json(
        app,
        Method::GET,
        &format!("/admin/recordings/{match_id}/download"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK, "download must return 200");
    let body = response_json(resp).await;

    // Must contain all Recording fields (not just metadata).
    assert_eq!(body["match_id"], match_id, "match_id field must be present");
    assert!(body["seed"].is_number(), "seed must be present");
    assert!(body["params"].is_object(), "params must be present");
    assert!(body["specs"].is_array(), "specs must be present");
    assert!(body["intent_log"].is_array(), "intent_log must be present");
    assert!(body["meta"].is_object(), "meta must be present");
    assert!(
        body["meta"]["tick_count"].is_number(),
        "meta.tick_count must be present"
    );
}

#[tokio::test]
async fn download_returns_404_for_unknown_id() {
    let app = test_app(RecordingStore::new(), ObserverHub::new());

    let resp = request_json(
        app,
        Method::GET,
        "/admin/recordings/nonexistent-id/download",
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_returns_401_without_auth() {
    let store = RecordingStore::new();
    let app = test_app(store.clone(), ObserverHub::new());

    let match_id = run_headless_match(app.clone()).await;

    let resp = request_json(
        app,
        Method::GET,
        &format!("/admin/recordings/{match_id}/download"),
        None,
        None,
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── (4) POST /admin/recordings/import stores recording + appears in listing ───

#[tokio::test]
async fn import_stores_recording_and_appears_in_listing() {
    // Step 1: run a match and download the full artifact.
    let source_store = RecordingStore::new();
    let source_app = test_app(source_store.clone(), ObserverHub::new());
    let match_id = run_headless_match(source_app.clone()).await;

    let dl_resp = request_json(
        source_app,
        Method::GET,
        &format!("/admin/recordings/{match_id}/download"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;
    assert_eq!(dl_resp.status(), StatusCode::OK);
    let recording_json = response_json(dl_resp).await;

    // Step 2: import into a FRESH, empty store.
    let fresh_store = RecordingStore::new();
    let fresh_app = test_app(fresh_store.clone(), ObserverHub::new());

    let import_resp = request_json(
        fresh_app.clone(),
        Method::POST,
        "/admin/recordings/import",
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        Some(recording_json),
    )
    .await;
    assert_eq!(
        import_resp.status(),
        StatusCode::OK,
        "import must return 200"
    );

    // Step 3: verify it appears in GET /recordings.
    let list_resp = request_json(fresh_app, Method::GET, "/recordings", None, None).await;
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list = response_json(list_resp).await;
    let ids: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|item| item["matchId"].as_str())
        .collect();
    assert!(
        ids.contains(&match_id.as_str()),
        "imported match_id must be in GET /recordings"
    );

    // Step 4: verify store.get() returns it.
    assert!(
        fresh_store.get(&match_id).is_some(),
        "fresh_store.get() must find the imported recording"
    );
}

// ── (5) import → recording is replayable ─────────────────────────────────────

#[tokio::test]
async fn import_makes_recording_replayable() {
    // Download artifact from a source app.
    let source_app = test_app(RecordingStore::new(), ObserverHub::new());
    let match_id = run_headless_match(source_app.clone()).await;

    let dl_resp = request_json(
        source_app,
        Method::GET,
        &format!("/admin/recordings/{match_id}/download"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;
    let recording_json = response_json(dl_resp).await;

    // Import into a fresh app.
    let fresh_app = test_app(RecordingStore::new(), ObserverHub::new());
    let import_resp = request_json(
        fresh_app.clone(),
        Method::POST,
        "/admin/recordings/import",
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        Some(recording_json),
    )
    .await;
    assert_eq!(import_resp.status(), StatusCode::OK);

    // Replay the imported recording.
    let replay_resp = request_json(
        fresh_app,
        Method::POST,
        &format!("/recordings/{match_id}/replay"),
        None,
        None,
    )
    .await;
    assert_eq!(
        replay_resp.status(),
        StatusCode::OK,
        "replay of imported recording must succeed"
    );
}

// ── (6) import malformed body → 400; no auth → 401 ────────────────────────────

#[tokio::test]
async fn import_malformed_body_returns_400() {
    let app = test_app(RecordingStore::new(), ObserverHub::new());

    let resp = request_json(
        app,
        Method::POST,
        "/admin/recordings/import",
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        Some(serde_json::json!({"not": "a recording"})),
    )
    .await;

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "malformed body must return 400"
    );
}

#[tokio::test]
async fn import_without_auth_returns_401() {
    let app = test_app(RecordingStore::new(), ObserverHub::new());

    let resp = request_json(
        app,
        Method::POST,
        "/admin/recordings/import",
        None,
        Some(serde_json::json!({})),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── (7) download → import → fresh store → list + replay round-trip ───────────

#[tokio::test]
async fn download_import_replay_round_trip() {
    // Phase A: run a real match in source app, download the full artifact.
    let source_store = RecordingStore::new();
    let source_app = test_app(source_store, ObserverHub::new());
    let match_id = run_headless_match(source_app.clone()).await;

    let dl_resp = request_json(
        source_app,
        Method::GET,
        &format!("/admin/recordings/{match_id}/download"),
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        None,
    )
    .await;
    assert_eq!(dl_resp.status(), StatusCode::OK);
    let artifact = response_json(dl_resp).await;

    // Sanity-check the artifact shape before round-tripping it.
    assert!(artifact["intent_log"].is_array());
    assert!(
        !artifact["intent_log"].as_array().unwrap().is_empty(),
        "intent_log must not be empty"
    );

    // Phase B: import the artifact into a COMPLETELY FRESH app (simulates
    // a different server instance receiving the artifact via HTTP).
    let fresh_hub = ObserverHub::new();
    let fresh_app = test_app(RecordingStore::new(), fresh_hub.clone());

    let import_resp = request_json(
        fresh_app.clone(),
        Method::POST,
        "/admin/recordings/import",
        Some(&format!("Facilitator {FACILITATOR_PASSWORD}")),
        Some(artifact),
    )
    .await;
    assert_eq!(
        import_resp.status(),
        StatusCode::OK,
        "import into fresh server must succeed"
    );

    // Phase C: verify listing.
    let list_resp = request_json(fresh_app.clone(), Method::GET, "/recordings", None, None).await;
    let list = response_json(list_resp).await;
    let listed_ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["matchId"].as_str())
        .collect();
    assert!(
        listed_ids.contains(&match_id.as_str()),
        "artifact must be listed after import"
    );

    // Phase D: replay must succeed (proves the artifact is genuinely portable).
    let replay_resp = request_json(
        fresh_app,
        Method::POST,
        &format!("/recordings/{match_id}/replay"),
        None,
        None,
    )
    .await;
    assert_eq!(
        replay_resp.status(),
        StatusCode::OK,
        "round-tripped artifact must be replayable"
    );
}
