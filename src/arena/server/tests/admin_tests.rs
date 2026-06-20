use std::{sync::Arc, time::Duration};

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
        specs: two_specs(&params),
        teams: teams(),
        wasm_store: WasmBotStore::new(),
        ws_registry: WsConnectionRegistry::new(),
        fuel_per_tick: 10_000_000,
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        tps: 0,
        max_matches: 2,
        pacer_factory: Arc::new(|| Box::new(NoopPacer)),
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
