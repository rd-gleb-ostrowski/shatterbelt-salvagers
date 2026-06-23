//! Integration tests for bot health & moderation (issue 12).
//!
//! ## TDD slices (RED → GREEN, in order)
//!
//! 1. `get_admin_bots_requires_facilitator_auth`          — 401 without password
//! 2. `get_admin_bots_returns_health_list_after_headless` — 200 + list after a headless match
//! 3. `wasm_fuel_exhaustion_shows_skipped_ticks_and_crashes` — fuel-bomb WASM bot metrics
//! 4. `wasm_log_bytes_captured_in_health`                 — log import bytes in recentLogs
//! 5. `ws_missed_deadlines_increment_skipped_ticks`       — inline WS health tracking
//! 6. `kick_returns_ok_and_marks_connected_false`         — kick endpoint 200 + health
//! 7. `kick_excludes_team_from_future_resolution`         — resolver uses Default after kick
//! 8. `kick_requires_facilitator_auth`                    — kick 401 without password

use std::{sync::Arc, time::Duration};

use arena_engine::{Params, ShipClass, ShipSpec, Vec2};
use arena_server::runner::BotDriver;
use arena_server::{
    auth::TokenRegistry,
    health::{BotHealthStore, DqStore},
    observer::ObserverHub,
    recording::RecordingStore,
    resolver::{ConnectionResolver, Slot, WsConnectionRegistry},
    routes::{build_router_config, RouterConfig},
    store::WasmBotStore,
};
use axum::body::Body;
use futures_util::future::join_all;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

// ── Constants ─────────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "test-event";
const FACILITATOR: &str = "Facilitator test-facilitator";
const WRONG_AUTH: &str = "Facilitator wrong-password";

// ── WAT fixtures (same pattern as wasm_host_tests) ───────────────────────────

/// A bot that always writes `{"thrust":1.0}` — baseline.
const CONST_ACTION_WAT: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 256) "{\"thrust\":1.0}")
  (func (export "alloc") (param $len i32) (result i32)
    i32.const 512
  )
  (func (export "init") (param $ptr i32) (param $len i32))
  (func (export "tick") (param $ptr i32) (param $len i32) (result i64)
    i64.const 256
    i64.const 32
    i64.shl
    i64.const 14
    i64.or
  )
)
"#;

/// Spins in an infinite loop — exhausts any finite fuel budget each tick.
const FUEL_BOMB_WAT: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "alloc") (param $len i32) (result i32)
    i32.const 0
  )
  (func (export "init") (param $ptr i32) (param $len i32))
  (func (export "tick") (param $ptr i32) (param $len i32) (result i64)
    (loop $spin (br $spin))
    i64.const 0
  )
)
"#;

/// Calls `log` during `tick` with the string `"hello"` (5 bytes) and returns
/// a valid action JSON.
const LOG_BOT_WAT: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0)   "hello")             ;; 5 bytes at offset 0
  (data (i32.const 256) "{\"thrust\":0.0}")  ;; action JSON at 256
  (func (export "alloc") (param $len i32) (result i32)
    i32.const 512
  )
  (func (export "init") (param $ptr i32) (param $len i32))
  (func (export "tick") (param $ptr i32) (param $len i32) (result i64)
    i32.const 0
    i32.const 5
    call $log
    i64.const 256
    i64.const 32
    i64.shl
    i64.const 14
    i64.or
  )
)
"#;

fn wat_to_wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT assembly failed")
}

// ── Test app helpers ──────────────────────────────────────────────────────────

/// Build a test router that shares the given health/DQ stores with the test.
fn test_app(
    health_store: Arc<BotHealthStore>,
    dq_store: Arc<DqStore>,
    wasm_store: Arc<WasmBotStore>,
    params: Params,
) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: "test-facilitator".to_owned(),
        registry: TokenRegistry::new(),
        wasm_store,
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(5),
        match_seed: 42,
        match_params: params,
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        health_store,
        dq_store,
        ladder: arena_server::ladder::Ladder::new(),
        disabled_store: arena_server::store::DisabledStore::new(),
        default_bot_store: arena_server::store::DefaultBotStore::new(),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    })
}

fn short_params() -> Params {
    Params { max_ticks: 10, ..Params::default() }
}

async fn http(
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
        Some(b) => {
            builder = builder.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    app.oneshot(builder.body(body).unwrap()).await.unwrap()
}

async fn json_body(resp: http::Response<Body>) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn register_and_upload_wasm(
    app: axum::Router,
    team: &str,
    wasm_bytes: Vec<u8>,
) -> axum::Router {
    // Register
    let (app, token) = {
        let token_resp = http(
            app.clone(),
            Method::POST,
            "/register",
            None,
            Some(serde_json::json!({ "password": EVENT_PASSWORD, "team": team })),
        )
        .await;
        let token = json_body(token_resp).await["token"]
            .as_str()
            .unwrap()
            .to_owned();
        (app, token)
    };
    // Upload WASM
    let req = Request::builder()
        .method("POST")
        .uri("/bots")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(wasm_bytes))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();
    app
}

async fn register_teams(app: axum::Router) {
    join_all(["health-team-1", "health-team-2"].map(async |t| {
        http(
            app.clone(),
            Method::POST,
            "/register",
            None,
            Some(serde_json::json!({ "password": EVENT_PASSWORD, "team": t })),
        )
        .await;
    }))
    .await;
}

async fn start_headless_match(app: axum::Router) -> http::Response<Body> {
    http(
        app,
        Method::POST,
        "/admin/matches",
        Some(FACILITATOR),
        Some(serde_json::json!({ "mode": "headless", "seed": 1, "maxTicks": 10 })),
    )
    .await
}

async fn get_bots(app: axum::Router, auth: Option<&str>) -> http::Response<Body> {
    http(app, Method::GET, "/admin/bots", auth, None).await
}

async fn kick_bot(app: axum::Router, team: &str, auth: Option<&str>) -> http::Response<Body> {
    http(
        app,
        Method::POST,
        &format!("/admin/bots/{team}/kick"),
        auth,
        None,
    )
    .await
}

// ── Slice 1: auth gating ─────────────────────────────────────────────────────

/// `GET /admin/bots` without the facilitator password → 401.
#[tokio::test]
async fn get_admin_bots_requires_facilitator_auth() {
    let app = test_app(
        BotHealthStore::new(),
        DqStore::new(),
        WasmBotStore::new(),
        short_params(),
    );

    let resp = get_bots(app.clone(), None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = get_bots(app, Some(WRONG_AUTH)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Slice 2: basic health list ────────────────────────────────────────────────

/// After a short headless match, `GET /admin/bots` returns a non-empty list
/// containing each team with at minimum `kind`, `connected`, and `team` fields.
#[tokio::test]
async fn get_admin_bots_returns_health_list_after_headless() {
    let health_store = BotHealthStore::new();
    let wasm_store = WasmBotStore::new();
    let app = test_app(
        health_store.clone(),
        DqStore::new(),
        wasm_store.clone(),
        short_params(),
    );

    // Start a headless match so the resolver populates health entries.
    register_teams(app.clone()).await;
    let match_resp = start_headless_match(app.clone()).await;
    assert_eq!(match_resp.status(), StatusCode::OK);

    // Now inspect health.
    let resp = get_bots(app, Some(FACILITATOR)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bots = json_body(resp).await;
    let arr = bots.as_array().expect("response is a JSON array");
    // Two teams should be present (team-a, team-b are the defaults).
    assert!(arr.len() >= 2, "expected at least 2 bot entries, got {}", arr.len());
    for entry in arr {
        assert!(entry["team"].is_string(), "entry missing team field");
        assert!(entry["kind"].is_string(), "entry missing kind field");
        assert!(entry["connected"].is_boolean(), "entry missing connected field");
        assert!(entry["skippedTicks"].is_number(), "entry missing skippedTicks");
        assert!(entry["crashes"].is_number(), "entry missing crashes");
        assert!(entry["recentLogs"].is_string(), "entry missing recentLogs");
    }
}

// ── Slice 3: WASM fuel exhaustion → skipped ticks + crashes ──────────────────

/// A WASM bot that exhausts its fuel budget every tick has its
/// `skippedTicks` and `crashes` fields reflected in `GET /admin/bots`.
#[tokio::test]
async fn wasm_fuel_exhaustion_shows_skipped_ticks_and_crashes() {
    let health_store = BotHealthStore::new();
    let wasm_store = WasmBotStore::new();
    let app = test_app(
        health_store.clone(),
        DqStore::new(),
        wasm_store.clone(),
        short_params(),
    );

    register_teams(app.clone()).await;
    // Register team-a with the fuel-bomb WASM bot.
    let app =
        register_and_upload_wasm(app, "team-a", wat_to_wasm(FUEL_BOMB_WAT)).await;

    // Start a headless match — fuel-bomb bot will exhaust fuel every tick.
    // Use very small fuel budget to guarantee exhaustion.
    let resp = http(
        app.clone(),
        Method::POST,
        "/admin/matches",
        Some(FACILITATOR),
        Some(serde_json::json!({ "mode": "headless", "seed": 1, "maxTicks": 5 })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Inspect health.
    let bots_resp = get_bots(app, Some(FACILITATOR)).await;
    assert_eq!(bots_resp.status(), StatusCode::OK);
    let bots = json_body(bots_resp).await;
    let arr = bots.as_array().unwrap();

    let team_a = arr
        .iter()
        .find(|b| b["team"] == "team-a")
        .expect("team-a should have a health entry");

    assert_eq!(team_a["kind"], "wasm", "driver kind should be wasm");
    assert!(
        team_a["skippedTicks"].as_u64().unwrap() > 0,
        "fuel-bomb WASM bot should have skipped ticks; got: {team_a}"
    );
    assert!(
        team_a["crashes"].as_u64().unwrap() > 0,
        "fuel-bomb WASM bot should have non-zero crashes; got: {team_a}"
    );
}

// ── Slice 4: WASM log bytes ───────────────────────────────────────────────────

/// WASM `log` import bytes are captured in `recentLogs` on `GET /admin/bots`.
#[tokio::test]
async fn wasm_log_bytes_captured_in_health() {
    let health_store = BotHealthStore::new();
    let wasm_store = WasmBotStore::new();
    let app = test_app(
        health_store.clone(),
        DqStore::new(),
        wasm_store.clone(),
        short_params(),
    );

    register_teams(app.clone()).await;
    // Register team-a with the log bot.
    let app = register_and_upload_wasm(app, "team-a", wat_to_wasm(LOG_BOT_WAT)).await;

    let resp = start_headless_match(app.clone()).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bots_resp = get_bots(app, Some(FACILITATOR)).await;
    let bots = json_body(bots_resp).await;
    let arr = bots.as_array().unwrap();

    let team_a = arr
        .iter()
        .find(|b| b["team"] == "team-a")
        .expect("team-a health entry");

    let logs = team_a["recentLogs"].as_str().unwrap();
    assert!(
        logs.contains("hello"),
        "log output 'hello' should appear in recentLogs; got: '{logs}'"
    );
}

// ── Slice 5: WS skipped-ticks via inline handler ─────────────────────────────

/// A WS bot that misses every deadline has `skippedTicks > 0` in health.
///
/// Uses the full TCP-server path (same as ws_tests.rs) so the inline
/// `handle_ws_bot` health-tracking code is exercised.
#[tokio::test]
async fn ws_missed_deadlines_increment_skipped_ticks() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};

    let health_store = BotHealthStore::new();
    let app = test_app(
        health_store.clone(),
        DqStore::new(),
        WasmBotStore::new(),
        Params { max_ticks: 5, ..Params::default() },
    );

    // Spawn a real TCP server.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // Register a team via HTTP.
    let client = reqwest::Client::new();
    let token = client
        .post(format!("http://{addr}/register"))
        .json(&serde_json::json!({ "password": EVENT_PASSWORD, "team": "ws-team" }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();

    // Connect WS bot but never respond to tick messages.
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws")).await.unwrap();

    // Read welcome.
    let welcome_txt = loop {
        match ws.next().await.unwrap().unwrap() {
            WsMsg::Text(t) => break t,
            _ => continue,
        }
    };
    let welcome: Value = serde_json::from_str(&welcome_txt).unwrap();
    let session_id = welcome["sessionId"].as_str().unwrap().to_owned();

    // Send join.
    ws.send(WsMsg::Text(
        serde_json::to_string(&serde_json::json!({
            "sessionId": session_id,
            "token": token,
            "name": "silent-bot"
        }))
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();

    // Wait for assigned (bot is now registered but idle).
    loop {
        match ws.next().await.unwrap().unwrap() {
            WsMsg::Text(t) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                if v["type"] == "assigned" {
                    break;
                }
            }
            _ => continue,
        }
    }

    // Brief delay so the server completes registration.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Start a live match that includes this team so ticks start flowing.
    let start_resp = client
        .post(format!("http://{addr}/admin/matches"))
        .header("Authorization", "Facilitator test-facilitator")
        .json(&serde_json::json!({ "mode": "live", "maxTicks": 5, "teams": ["ws-team", "team-b"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(start_resp.status(), 200, "match start must succeed");

    // Drain all messages (matchStart, ticks, matchEnd) without responding.
    // Bound: at most 5 ticks + 3 control messages = 8 messages max.
    let mut msg_count = 0_u32;
    loop {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(WsMsg::Text(t)))) => {
                msg_count += 1;
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                if v["type"] == "matchEnd" || msg_count >= 20 {
                    break;
                }
            }
            _ => break,
        }
    }

    // Give the server a moment to update the health entry.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Check health.
    let health_snap = health_store.list_snapshots();
    let ws_team = health_snap
        .iter()
        .find(|s| s.team == "ws-team")
        .expect("ws-team health entry should exist");

    assert_eq!(ws_team.kind, "ws");
    assert!(
        ws_team.skipped_ticks > 0,
        "silent WS bot should have skipped ticks, got {}",
        ws_team.skipped_ticks
    );
}

// ── Slice 6: kick → connected=false ──────────────────────────────────────────

/// `POST /admin/bots/{team}/kick` returns 200 and the team's health entry
/// transitions to `connected=false`.
#[tokio::test]
async fn kick_returns_ok_and_marks_connected_false() {
    let health_store = BotHealthStore::new();
    let dq_store = DqStore::new();
    let app = test_app(
        health_store.clone(),
        dq_store.clone(),
        WasmBotStore::new(),
        short_params(),
    );

    // Run a headless match to populate health entries.
    register_teams(app.clone()).await;
    let resp = start_headless_match(app.clone()).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify team-a exists and is connected.
    let entry_before = health_store.get("health-team-1").expect("health-team-1 entry present");
    assert!(entry_before.snapshot().connected, "health-team-1 should be connected before kick");

    // Kick team-a.
    let kick_resp = kick_bot(app.clone(), "health-team-1", Some(FACILITATOR)).await;
    assert_eq!(kick_resp.status(), StatusCode::OK, "kick should return 200");

    // Check health shows connected=false.
    let bots_resp = get_bots(app, Some(FACILITATOR)).await;
    let bots = json_body(bots_resp).await;
    let arr = bots.as_array().unwrap();
    let health_team_1 = arr
        .iter()
        .find(|b| b["team"] == "health-team-1")
        .expect("health-team-1 health entry");
    assert_eq!(
        health_team_1["connected"], false,
        "health-team-1 should be disconnected after kick; got: {health_team_1}"
    );

    // DqStore must contain team-a.
    assert!(
        dq_store.is_disqualified("health-team-1"),
        "health-team-1 should be in DqStore after kick"
    );
}

// ── Slice 7: kicked team excluded from future resolution ─────────────────────

/// After a kick, a subsequent match resolution assigns the Default Bot to the
/// DQ'd team (kind = "default") instead of the previously-registered WS/WASM bot.
#[tokio::test]
async fn kick_excludes_team_from_future_resolution() {
    use arena_engine::Engine;
    use arena_server::ws::obs_to_tick_json;

    let health_store = BotHealthStore::new();
    let dq_store = DqStore::new();
    let wasm_store = WasmBotStore::new();

    // Store a WASM bot for team-a so the resolver would normally pick it.
    wasm_store.store("team-a", wat_to_wasm(CONST_ACTION_WAT));

    // Without kick: resolver should give WASM driver for team-a.
    {
        let resolver = ConnectionResolver::new(
            WsConnectionRegistry::new(),
            Arc::clone(&wasm_store),
            10_000_000,
        )
        .with_moderation(Arc::clone(&dq_store), Arc::clone(&health_store));

        let params = short_params();
        let specs = vec![ShipSpec {
            id: "ship-0".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.5, params.arena_h * 0.5),
        }];
        let engine0 = Engine::new(42, params.clone(), specs.clone());
        let slots = vec![Slot {
            team: "team-a".to_owned(),
            tick0_obs_json: engine0
                .observation(&"ship-0".to_owned())
                .map(|obs| obs_to_tick_json(0, &obs))
                .unwrap_or_default(),
        }];

        let drivers = resolver.resolve(&slots, &params);
        assert_eq!(
            drivers[0].kind(),
            "wasm",
            "before kick, team-a should use wasm driver"
        );
    }

    // Disqualify team-a.
    dq_store.disqualify("team-a");

    // After kick: resolver should give Default Bot for team-a.
    {
        // Re-upload WASM bot (still present in store).
        wasm_store.store("team-a", wat_to_wasm(CONST_ACTION_WAT));

        let resolver = ConnectionResolver::new(
            WsConnectionRegistry::new(),
            Arc::clone(&wasm_store),
            10_000_000,
        )
        .with_moderation(Arc::clone(&dq_store), Arc::clone(&health_store));

        let params = short_params();
        let specs = vec![ShipSpec {
            id: "ship-0".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.5, params.arena_h * 0.5),
        }];
        let engine0 = Engine::new(42, params.clone(), specs.clone());
        let slots = vec![Slot {
            team: "team-a".to_owned(),
            tick0_obs_json: engine0
                .observation(&"ship-0".to_owned())
                .map(|obs| obs_to_tick_json(0, &obs))
                .unwrap_or_default(),
        }];

        let drivers = resolver.resolve(&slots, &params);
        // ExclusionDriver wraps the inner driver; kind() delegates to inner.
        // After DQ, inner is DefaultBotDriver → kind() == "default".
        assert_eq!(
            drivers[0].kind(),
            "default",
            "after kick, team-a should fall back to default driver"
        );
    }
}

// ── Slice 8: kick auth gating ─────────────────────────────────────────────────

/// `POST /admin/bots/{team}/kick` without the facilitator password → 401.
#[tokio::test]
async fn kick_requires_facilitator_auth() {
    let app = test_app(
        BotHealthStore::new(),
        DqStore::new(),
        WasmBotStore::new(),
        short_params(),
    );

    let resp = kick_bot(app.clone(), "team-a", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = kick_bot(app, "team-a", Some(WRONG_AUTH)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Slice 9: ExclusionDriver mid-match exclusion ──────────────────────────────

/// After disqualification, the `ExclusionDriver` wrapper returns `None` from
/// `decide` so the ship's intents stop being applied (engine uses previous
/// intent / Default behaviour).
///
/// This is tested at the unit level (MatchRunner + ExclusionDriver directly)
/// so no WS socket is needed.
#[tokio::test]
async fn exclusion_driver_returns_none_after_kick() {
    use arena_engine::Intent;
    use arena_server::health::ExclusionDriver;
    // A driver that always returns Some(Intent::default()).
    struct AlwaysDriver;
    impl BotDriver for AlwaysDriver {
        fn decide(&mut self, _tick: u32, _obs: &arena_engine::Observation) -> Option<Intent> {
            Some(Intent::default())
        }
        fn kind(&self) -> &'static str {
            "always"
        }
    }

    let dq = DqStore::new();
    let health_store = BotHealthStore::new();
    let health = health_store.register(arena_server::health::BotHealthEntry::new("t", "ws"));

    let mut driver = ExclusionDriver::new(
        "team-x",
        Box::new(AlwaysDriver),
        Arc::clone(&dq),
        Some(health),
    );

    // Build a minimal observation using the engine.
    let params = short_params();
    let specs = vec![ShipSpec {
        id: "s0".to_owned(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::new(params.arena_w * 0.5, params.arena_h * 0.5),
    }];
    let engine = arena_engine::Engine::new(42, params.clone(), specs);
    let obs = engine.observation(&"s0".to_owned()).unwrap();

    // Before kick: should produce Some.
    assert!(
        driver.decide(0, &obs).is_some(),
        "driver should return Some before kick"
    );

    // Kick: disqualify team-x.
    dq.disqualify("team-x");

    // After kick: should return None.
    assert!(
        driver.decide(1, &obs).is_none(),
        "driver should return None after kick"
    );

    // Health entry should show connected=false after the first excluded tick.
    let snap = health_store.list_snapshots();
    let entry = snap.iter().find(|s| s.team == "t").unwrap();
    assert!(!entry.connected, "health entry should show connected=false after exclusion");
}
