//! Integration tests for the WS Bot persistent-connection model (ADR-0001 v2).
//!
//! Tests assert **observable PROTOCOL behaviour over the wire** via a real
//! in-process WebSocket client (tokio-tungstenite) connected to an ephemeral
//! localhost TCP port (127.0.0.1:0).  No fixed ports.  Deadlines and match
//! durations are kept tiny so the suite runs fast without real pacing sleeps.
//!
//! ## Test inventory
//!
//! 1. `ws_welcome_on_connect`             — server sends well-formed `welcome`
//! 2. `ws_valid_token_gets_assigned`      — valid `join` → `assigned` with shipId = team name
//! 3. `ws_invalid_token_rejected`         — bad token → close (no `assigned`)
//! 4. `ws_bot_stays_registered_after_handshake` — registry.has(team) after assigned
//! 5. `ws_receives_match_start_and_tick`  — match started externally → matchStart + tick
//! 6. `ws_tick_obs_is_private_view`       — `ships` array has no `aether`/`sigil`
//! 7. `ws_action_before_deadline_applied` — sent `turn` changes heading
//! 8. `ws_missed_deadline_persists_action` — no action → engine keeps previous intent
//! 9. `ws_receives_match_end`             — match ends with `matchEnd`
//! 10. `ws_bot_still_registered_after_match` — registry.has(team) AFTER match completes
//! 11. `ws_two_sequential_matches_reuse_connection` — TWO successive matches on same socket
//! 12. `ws_match_start_end_envelopes_delivered` — matchStart / matchEnd are delivered

use std::sync::Arc;
use std::time::Duration;

use arena_engine::Params;
use arena_server::{
    auth::TokenRegistry,
    resolver::WsConnectionRegistry,
    routes::{AppState, build_router_config, RouterConfig},
    store::WasmBotStore,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};

// ── Constants ─────────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "test-secret";
const FACILITATOR_PASSWORD: &str = "test-facilitator";
/// Short deadline: bots reply quickly in tests; this also governs how long
/// WsBotDriver::decide waits before declaring a miss.
const TEST_DEADLINE_MS: u64 = 50;
/// Very short match so tests that wait for matchEnd finish quickly.
const TEST_MAX_TICKS: u32 = 8;

// ── App builder ───────────────────────────────────────────────────────────────

/// Build a test router with a shared, inspectable ws_registry.
fn test_app_with_registry(
    registry: Arc<TokenRegistry>,
    ws_registry: Arc<WsConnectionRegistry>,
) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry,
        wasm_store: WasmBotStore::new(),
        ws_registry,
        tick_deadline: Duration::from_millis(TEST_DEADLINE_MS),
        match_seed: 42,
        match_params: Params { max_ticks: TEST_MAX_TICKS, ..Params::default() },
        observer_hub: arena_server::observer::ObserverHub::new(),
        recording_store: arena_server::recording::RecordingStore::new(),
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
        ladder: arena_server::ladder::Ladder::new(),
        disabled_store: arena_server::store::DisabledStore::new(),
        default_bot_store: arena_server::store::DefaultBotStore::new(),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    })
}

fn test_app(registry: Arc<TokenRegistry>) -> axum::Router {
    test_app_with_registry(registry, WsConnectionRegistry::new())
}

/// Bind an ephemeral port, spawn the axum server, return the bound address.
async fn spawn_server(app: axum::Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

/// Spawn server returning both the socket addr AND the shared AppState
/// (so tests can inspect the registry directly).
async fn spawn_server_with_state(
    registry: Arc<TokenRegistry>,
    ws_registry: Arc<WsConnectionRegistry>,
) -> std::net::SocketAddr {
    let app = test_app_with_registry(Arc::clone(&registry), Arc::clone(&ws_registry));
    spawn_server(app).await
}

/// Register a team via HTTP; return the token.
async fn register_team(addr: std::net::SocketAddr, team: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/register"))
        .json(&serde_json::json!({ "password": EVENT_PASSWORD, "team": team }))
        .send()
        .await
        .expect("register request");
    resp.json::<Value>()
        .await
        .expect("register response JSON")["token"]
        .as_str()
        .expect("token string")
        .to_owned()
}

/// Start a live match via the admin API; return the match_id.
async fn start_live_match(addr: std::net::SocketAddr, teams: Option<Vec<&str>>) -> String {
    let client = reqwest::Client::new();
    let body = if let Some(t) = teams {
        serde_json::json!({"mode": "live", "maxTicks": TEST_MAX_TICKS, "teams": t})
    } else {
        serde_json::json!({"mode": "live", "maxTicks": TEST_MAX_TICKS})
    };
    let resp = client
        .post(format!("http://{addr}/admin/matches"))
        .header("Authorization", format!("Facilitator {FACILITATOR_PASSWORD}"))
        .json(&body)
        .send()
        .await
        .expect("start match");
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["matchId"]
        .as_str()
        .unwrap()
        .to_owned()
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Connect a tokio-tungstenite WS client to `ws://<addr>/ws`.
async fn ws_connect(addr: std::net::SocketAddr) -> WsStream {
    let (stream, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("ws connect");
    stream
}

/// Read the next text frame, skipping pings/pongs.
async fn next_text(ws: &mut WsStream) -> Value {
    loop {
        match ws.next().await.expect("socket alive").expect("no error") {
            WsMsg::Text(t) => return serde_json::from_str(&t).expect("valid JSON"),
            WsMsg::Ping(_) | WsMsg::Pong(_) => continue,
            other => panic!("expected text, got {:?}", other),
        }
    }
}

/// Read the next text frame with a timeout; returns None on timeout/close.
#[allow(dead_code)]
async fn next_text_timeout(ws: &mut WsStream, ms: u64) -> Option<Value> {
    let fut = async {
        loop {
            match ws.next().await?.ok()? {
                WsMsg::Text(t) => return serde_json::from_str(&t).ok(),
                WsMsg::Ping(_) | WsMsg::Pong(_) => continue,
                WsMsg::Close(_) => return None,
                _ => continue,
            }
        }
    };
    timeout(Duration::from_millis(ms), fut).await.ok().flatten()
}

/// Send a JSON value as a text frame.
async fn send_json(ws: &mut WsStream, v: Value) {
    ws.send(WsMsg::Text(serde_json::to_string(&v).unwrap().into()))
        .await
        .expect("send");
}

/// Perform the full handshake (welcome → join → assigned).
/// Returns (session_id, assigned_ship_id).
async fn do_handshake(ws: &mut WsStream, token: &str, name: &str) -> (String, String) {
    let welcome = next_text(ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap().to_owned();
    send_json(
        ws,
        serde_json::json!({
            "type": "join",
            "sessionId": session_id,
            "token": token,
            "name": name,
        }),
    )
    .await;
    let assigned = next_text(ws).await;
    let ship_id = assigned["shipId"].as_str().unwrap().to_owned();
    (session_id, ship_id)
}

/// Drain messages from `ws` until we see one with the given `type_`,
/// returning it.  Passes all others through a predicate for side-effects.
/// Times out after `timeout_ms`.
async fn wait_for_type(ws: &mut WsStream, type_: &str, timeout_ms: u64) -> Option<Value> {
    let deadline = Duration::from_millis(timeout_ms);
    let fut = async {
        loop {
            let msg = next_text(ws).await;
            if msg["type"].as_str() == Some(type_) {
                return Some(msg);
            }
        }
    };
    timeout(deadline, fut).await.ok().flatten()
}

// ── Test helpers ──────────────────────────────────────────────────────────────

fn normalise_angle(mut a: f64) -> f64 {
    while a > std::f64::consts::PI {
        a -= 2.0 * std::f64::consts::PI;
    }
    while a < -std::f64::consts::PI {
        a += 2.0 * std::f64::consts::PI;
    }
    a
}

// ── Test 1: welcome on connect ────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_welcome_on_connect() {
    let registry = TokenRegistry::new();
    let addr = spawn_server(test_app(Arc::clone(&registry))).await;
    let mut ws = ws_connect(addr).await;

    let msg = next_text(&mut ws).await;

    assert_eq!(msg["type"], "welcome");
    let sid = msg["sessionId"].as_str().expect("sessionId present");
    assert_eq!(sid.len(), 36, "sessionId is a UUID (36 chars)");
    assert!(uuid::Uuid::parse_str(sid).is_ok(), "sessionId parses as UUID");
    assert_eq!(msg["protocolVersion"], "1");
    assert_eq!(msg["gameType"], "ShatterbeltSalvagers");
}

// ── Test 2: valid token → assigned ───────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_valid_token_gets_assigned() {
    let registry = TokenRegistry::new();
    let token = registry.register("alice");
    let addr = spawn_server(test_app(Arc::clone(&registry))).await;
    let mut ws = ws_connect(addr).await;

    let welcome = next_text(&mut ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap();
    send_json(
        &mut ws,
        serde_json::json!({ "type": "join", "sessionId": session_id, "token": token, "name": "alice-bot" }),
    )
    .await;

    let assigned = next_text(&mut ws).await;
    assert_eq!(assigned["type"], "assigned");
    // In the persistent model, shipId at handshake time is the team's stable
    // identity (the team name registered via POST /register).
    let ship_id = assigned["shipId"].as_str().expect("shipId present");
    assert!(!ship_id.is_empty(), "shipId not empty");
    assert_eq!(ship_id, "alice", "shipId equals the team name");
}

// ── Test 3: invalid token → close (no assigned) ───────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_invalid_token_rejected() {
    let registry = TokenRegistry::new();
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    let welcome = next_text(&mut ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap();
    send_json(
        &mut ws,
        serde_json::json!({ "type": "join", "sessionId": session_id, "token": "bad-token", "name": "evil" }),
    )
    .await;

    loop {
        match ws.next().await {
            None => break,
            Some(Ok(WsMsg::Close(_))) => break,
            Some(Ok(WsMsg::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                assert_ne!(v["type"], "assigned", "must NOT receive assigned after bad token");
            }
            Some(Ok(WsMsg::Ping(_))) | Some(Ok(WsMsg::Pong(_))) => continue,
            Some(Err(_)) => break,
            Some(Ok(_)) => {}
        }
    }
}

// ── Test 4: bot stays registered after handshake ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_bot_stays_registered_after_handshake() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("team-reg");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "reg-bot").await;

    // Give the server a moment to register the session after sending assigned.
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(
        ws_registry.has("team-reg"),
        "ws_registry must contain 'team-reg' after successful handshake"
    );

    // connected_teams must include the team.
    assert!(
        ws_registry.connected_teams().contains(&"team-reg".to_owned()),
        "connected_teams must include 'team-reg'"
    );

    // Drop the socket to trigger disconnect cleanup.
    drop(ws);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        !ws_registry.has("team-reg"),
        "ws_registry must remove 'team-reg' after disconnect"
    );
}

// ── Test 5: matchStart + tick after match is started externally ───────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_receives_match_start_and_tick() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("bot-a");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "bot-a").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Start a live match that includes this team.
    start_live_match(addr, Some(vec!["bot-a", "team-b"])).await;

    // Should receive matchStart.
    let match_start = timeout(
        Duration::from_millis(500),
        wait_for_type(&mut ws, "matchStart", 500),
    )
    .await
    .expect("matchStart timeout")
    .expect("received matchStart");
    assert_eq!(match_start["type"], "matchStart");

    // Should receive first tick observation.
    let tick = timeout(
        Duration::from_millis(500),
        next_text(&mut ws),
    )
    .await
    .expect("tick timeout");
    assert_eq!(tick["type"], "tick");
    assert_eq!(tick["tick"], 0, "first tick is tick 0");
    assert!(tick["maxTicks"].as_u64().is_some(), "maxTicks present");
    assert!(tick["seed"].as_u64().is_some(), "seed present");
    assert!(tick["arena"]["width"].as_f64().is_some(), "arena.width present");
    assert!(tick["self"]["id"].as_str().is_some(), "self.id present");
    assert!(tick["self"]["pos"]["x"].as_f64().is_some(), "self.pos.x present");
    assert!(tick["self"]["aether"]["cur"].as_f64().is_some(), "self.aether present");
}

// ── Test 6: private observation — no aether/sigil in ships array ─────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_tick_obs_is_private_view() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("carol");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "carol-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    start_live_match(addr, Some(vec!["carol", "team-b"])).await;

    // Consume matchStart.
    let _ = wait_for_type(&mut ws, "matchStart", 500)
        .await
        .expect("matchStart");

    // Get first tick.
    let tick = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("tick timeout");

    // Own aether must be present in "self".
    assert!(tick["self"]["aether"].is_object(), "own aether present in self");

    // Enemy ships must NOT expose aether or sigil.
    let ships = tick["ships"].as_array().expect("ships is array");
    for ship in ships {
        assert!(
            ship.get("aether").is_none() || ship["aether"].is_null(),
            "enemy ship must not expose aether: {:?}",
            ship
        );
        assert!(
            ship.get("sigil").is_none() || ship["sigil"].is_null(),
            "enemy ship must not expose sigil: {:?}",
            ship
        );
    }
}

// ── Test 7: action before deadline is applied ────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_action_before_deadline_applied() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("dave");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "dave-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    start_live_match(addr, Some(vec!["dave", "team-b"])).await;
    let _ = wait_for_type(&mut ws, "matchStart", 500).await.expect("matchStart");

    // Tick 0: record heading, send turn=1.0.
    let obs0 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("obs0 timeout");
    let h0 = obs0["self"]["heading"].as_f64().expect("heading at tick 0");
    send_json(&mut ws, serde_json::json!({"type": "action", "turn": 1.0, "thrust": 0.0})).await;

    // Tick 1: heading must have changed by ~max_turn = 0.15 rad.
    let obs1 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("obs1 timeout");
    let h1 = obs1["self"]["heading"].as_f64().expect("heading at tick 1");
    let delta = normalise_angle(h1 - h0);
    assert!(
        (delta - 0.15).abs() < 0.02,
        "heading changed by ~max_turn after turn=1.0 (delta={delta:.4})"
    );
}

// ── Test 8: missed deadline persists previous action ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_missed_deadline_persists_action() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("eve");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "eve-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    start_live_match(addr, Some(vec!["eve", "team-b"])).await;
    let _ = wait_for_type(&mut ws, "matchStart", 500).await.expect("matchStart");

    // Tick 0: send turn=1.0.
    let obs0 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("obs0");
    let h0 = obs0["self"]["heading"].as_f64().unwrap();
    send_json(&mut ws, serde_json::json!({"type": "action", "turn": 1.0, "thrust": 0.0})).await;

    // Tick 1: receive obs, do NOT send action — let deadline expire.
    let obs1 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("obs1");
    let h1 = obs1["self"]["heading"].as_f64().unwrap();
    let d01 = normalise_angle(h1 - h0);
    assert!((d01 - 0.15).abs() < 0.02, "tick1 changed by ~0.15 (d01={d01:.4})");

    // Wait longer than deadline so the server declares a miss.
    tokio::time::sleep(Duration::from_millis(TEST_DEADLINE_MS + 30)).await;

    // Tick 2: turn=1.0 should have persisted → another ~0.15 rad change.
    let obs2 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("obs2");
    let h2 = obs2["self"]["heading"].as_f64().unwrap();
    let d12 = normalise_angle(h2 - h1);
    assert!(
        (d12 - 0.15).abs() < 0.02,
        "heading still changed at tick 2 because turn=1.0 persisted (d12={d12:.4})"
    );
}

// ── Test 9: matchEnd received at end of match ────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_receives_match_end() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("frank");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "frank-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    start_live_match(addr, Some(vec!["frank", "team-b"])).await;

    // Drain messages until we see matchEnd (or timeout).
    let match_end = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchEnd", 5000),
    )
    .await
    .expect("matchEnd outer timeout")
    .expect("received matchEnd");

    assert_eq!(match_end["type"], "matchEnd");
    assert!(match_end["results"].is_object(), "results present");
    assert!(match_end["results"]["scores"].is_object(), "results.scores present");
    let ticks = match_end["results"]["ticks"].as_u64().expect("ticks");
    assert_eq!(ticks, TEST_MAX_TICKS as u64, "match ran for exactly max_ticks");
}

// ── Test 10: bot still registered after match completes ──────────────────────
//
// The key regression: the bot should REMAIN in the registry after the match
// so it can play subsequent matches on the same connection.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_bot_still_registered_after_match() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("grace");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "grace-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Verify registered before match.
    assert!(ws_registry.has("grace"), "registered before match");

    start_live_match(addr, Some(vec!["grace", "team-b"])).await;

    // Wait for matchEnd.
    let _ = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchEnd", 5000),
    )
    .await
    .expect("matchEnd")
    .expect("received matchEnd");

    // Allow brief cleanup.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Bot must STILL be registered — this is the core regression test.
    assert!(
        ws_registry.has("grace"),
        "bot must remain registered after match (persistent connection model)"
    );
}

// ── Test 11: two sequential matches reuse the same WS connection ──────────────
//
// THE regression test for the reported bug:
// - Connect once and stay connected.
// - Match 1 completes → matchEnd received.
// - Match 2 starts on the SAME socket → matchStart + tick + matchEnd received.
// - Registry still has the bot after both matches.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_two_sequential_matches_reuse_connection() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("henry");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "henry-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(ws_registry.has("henry"), "registered before match 1");

    // ── 1 Match ─────────────────────────────
    start_live_match(addr, Some(vec!["henry", "team-b"])).await;

    let ms1 = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchStart", 5000),
    )
    .await
    .expect("match1 matchStart timeout")
    .expect("match1 matchStart");
    assert_eq!(ms1["type"], "matchStart", "match 1 matchStart");

    // Wait for match 1 matchEnd.
    let me1 = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchEnd", 5000),
    )
    .await
    .expect("match1 matchEnd timeout")
    .expect("match1 matchEnd");
    assert_eq!(me1["type"], "matchEnd", "match 1 matchEnd");
    assert!(
        ws_registry.has("henry"),
        "bot must still be registered after match 1 (reuse)"
    );

    // Brief pause to let server settle between matches.
    tokio::time::sleep(Duration::from_millis(30)).await;

    // ── Match 2 — same socket, same registration ──────────────────────────────
    start_live_match(addr, Some(vec!["henry", "team-b"])).await;

    let ms2 = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchStart", 5000),
    )
    .await
    .expect("match2 matchStart timeout")
    .expect("match2 matchStart");
    assert_eq!(ms2["type"], "matchStart", "match 2 matchStart arrived on same socket");

    // Receive at least one tick in match 2, proving the socket is still driving.
    let tick2 = timeout(Duration::from_millis(500), next_text(&mut ws))
        .await
        .expect("match2 tick timeout");
    assert_eq!(tick2["type"], "tick", "tick received in match 2 on same socket");

    // Wait for match 2 matchEnd.
    let me2 = timeout(
        Duration::from_secs(5),
        wait_for_type(&mut ws, "matchEnd", 5000),
    )
    .await
    .expect("match2 matchEnd timeout")
    .expect("match2 matchEnd");
    assert_eq!(me2["type"], "matchEnd", "match 2 matchEnd");

    // Bot still registered after TWO matches.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        ws_registry.has("henry"),
        "bot must still be registered after two sequential matches"
    );
}

// ── Test 12: matchStart/matchEnd envelopes delivered around each match ────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_match_start_end_envelopes_delivered() {
    let token_registry = Arc::new(TokenRegistry::new());
    let token = token_registry.register("iris");
    let ws_registry = WsConnectionRegistry::new();
    let addr = spawn_server_with_state(
        Arc::clone(&token_registry),
        Arc::clone(&ws_registry),
    )
    .await;

    let mut ws = ws_connect(addr).await;
    do_handshake(&mut ws, &token, "iris-bot").await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    start_live_match(addr, Some(vec!["iris", "team-b"])).await;

    // Collect all message types until matchEnd (or timeout).
    let mut types_seen: Vec<String> = Vec::new();
    let result = timeout(Duration::from_secs(5), async {
        loop {
            let msg = next_text(&mut ws).await;
            let t = msg["type"].as_str().unwrap_or("").to_owned();
            let done = t == "matchEnd";
            types_seen.push(t);
            if done {
                break;
            }
        }
    })
    .await;
    assert!(result.is_ok(), "should receive matchEnd within 5 s");

    assert!(
        types_seen.contains(&"matchStart".to_owned()),
        "matchStart must be delivered (saw: {types_seen:?})"
    );
    assert!(
        types_seen.contains(&"tick".to_owned()),
        "at least one tick must be delivered (saw: {types_seen:?})"
    );
    assert!(
        types_seen.contains(&"matchEnd".to_owned()),
        "matchEnd must be delivered (saw: {types_seen:?})"
    );

    // Verify message ordering: matchStart before any tick before matchEnd.
    let ms_idx = types_seen.iter().position(|t| t == "matchStart").unwrap();
    let tick_idx = types_seen.iter().position(|t| t == "tick").unwrap();
    let me_idx = types_seen.iter().position(|t| t == "matchEnd").unwrap();
    assert!(ms_idx < tick_idx, "matchStart must come before first tick");
    assert!(tick_idx < me_idx, "tick must come before matchEnd");
}
