//! Integration tests for the WS Bot connect & play endpoint (issue 03).
//!
//! Tests assert **observable PROTOCOL behaviour over the wire** via a real
//! in-process WebSocket client (tokio-tungstenite) connected to an ephemeral
//! localhost TCP port (127.0.0.1:0).  No fixed ports, no real 33ms pacing
//! sleeps — the deadline is injected at 50 ms so the suite runs fast.
//!
//! ## TDD order
//!
//! 1. `ws_welcome_on_connect`            — server sends well-formed `welcome`
//! 2. `ws_valid_token_gets_assigned`     — valid `join` → `assigned` with shipId
//! 3. `ws_invalid_token_rejected`        — bad token → close (no `assigned`)
//! 4. `ws_receives_match_start_and_tick` — after assignment: `matchStart` + `tick`
//! 5. `ws_tick_obs_is_private_view`      — `ships` array has no `aether`/`sigil`
//! 6. `ws_action_before_deadline_is_applied` — sent `turn` changes heading
//! 7. `ws_missed_deadline_persists_previous_action` — heading keeps turning
//! 8. `ws_receives_match_end`            — short match ends with `matchEnd`

use std::time::Duration;

use arena_engine::Params;
use arena_server::{
    auth::TokenRegistry,
    routes::{build_router_config, RouterConfig},
    store::WasmBotStore,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};

// ── Test helpers ──────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "test-secret";
/// Short deadline so tests don't sleep 33 ms per tick.
const TEST_DEADLINE_MS: u64 = 50;
/// Very short match for tests that need to reach matchEnd quickly.
const TEST_MAX_TICKS: u32 = 8;

/// Build an axum router with a short match configuration for testing.
fn test_app(registry: std::sync::Arc<TokenRegistry>) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: "test-facilitator".to_owned(),
        registry,
        wasm_store: WasmBotStore::new(),
        ws_registry: arena_server::resolver::WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(TEST_DEADLINE_MS),
        match_seed: 42,
        match_params: Params { max_ticks: TEST_MAX_TICKS, ..Params::default() },
        observer_hub: arena_server::observer::ObserverHub::new(),
        recording_store: arena_server::recording::RecordingStore::new(),
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
    })
}

/// Bind an ephemeral port, spawn the axum server as a background task, and
/// return the bound address.  The server runs for the life of the test.
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

/// Register a team and return its token.
#[allow(dead_code)]
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

/// Connect a tokio-tungstenite WS client to `ws://<addr>/ws`.
async fn ws_connect(
    addr: std::net::SocketAddr,
) -> tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
> {
    let url = format!("ws://{addr}/ws");
    let (stream, _) = connect_async(&url).await.expect("ws connect");
    stream
}

/// Read the next text frame from the socket, skipping pings.
async fn next_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        match ws.next().await.expect("socket alive").expect("no error") {
            WsMsg::Text(t) => return serde_json::from_str(&t).expect("valid JSON"),
            WsMsg::Ping(_) | WsMsg::Pong(_) => continue,
            other => panic!("expected text, got {:?}", other),
        }
    }
}

/// Send a JSON value as a text frame.
async fn send_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    v: Value,
) {
    ws.send(WsMsg::Text(serde_json::to_string(&v).unwrap().into()))
        .await
        .expect("send");
}

/// Complete the full WS handshake: welcome → join → assigned.
/// Returns (session_id, ship_id).
async fn do_handshake(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    token: &str,
    name: &str,
) -> (String, String) {
    // receive welcome
    let welcome = next_text(ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap().to_owned();

    // send join
    send_json(
        ws,
        serde_json::json!({
            "type": "join",
            "sessionId": session_id,
            "token": token,
            "name": name
        }),
    )
    .await;

    // receive assigned
    let assigned = next_text(ws).await;
    let ship_id = assigned["shipId"].as_str().unwrap().to_owned();

    (session_id, ship_id)
}

// ── Test 1: welcome on connect ────────────────────────────────────────────────

#[tokio::test]
async fn ws_welcome_on_connect() {
    let registry = TokenRegistry::new();
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    let msg = next_text(&mut ws).await;

    assert_eq!(msg["type"], "welcome", "type must be 'welcome'");
    let sid = msg["sessionId"].as_str().expect("sessionId present");
    assert_eq!(sid.len(), 36, "sessionId is a UUID (36 chars)");
    assert!(
        uuid::Uuid::parse_str(sid).is_ok(),
        "sessionId parses as UUID"
    );
    assert_eq!(msg["protocolVersion"], "1");
    assert_eq!(msg["gameType"], "ShatterbeltSalvagers");
}

// ── Test 2: valid token → assigned ───────────────────────────────────────────

#[tokio::test]
async fn ws_valid_token_gets_assigned() {
    let registry = TokenRegistry::new();
    let token = registry.register("alice");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    let welcome = next_text(&mut ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap();

    send_json(
        &mut ws,
        serde_json::json!({
            "type": "join",
            "sessionId": session_id,
            "token": token,
            "name": "alice-bot"
        }),
    )
    .await;

    let assigned = next_text(&mut ws).await;
    assert_eq!(assigned["type"], "assigned", "type must be 'assigned'");
    let ship_id = assigned["shipId"].as_str().expect("shipId present");
    assert!(!ship_id.is_empty(), "shipId not empty");
}

// ── Test 3: invalid token → close (no assigned) ───────────────────────────────

#[tokio::test]
async fn ws_invalid_token_rejected() {
    let registry = TokenRegistry::new();
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    let welcome = next_text(&mut ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap();

    // Send join with a bogus token
    send_json(
        &mut ws,
        serde_json::json!({
            "type": "join",
            "sessionId": session_id,
            "token": "not-a-valid-token",
            "name": "evil-bot"
        }),
    )
    .await;

    // Server must close the connection — we should NOT receive 'assigned'
    loop {
        match ws.next().await {
            None => break, // connection closed cleanly
            Some(Ok(WsMsg::Close(_))) => break,
            Some(Ok(WsMsg::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                assert_ne!(
                    v["type"], "assigned",
                    "must NOT receive assigned after bad token"
                );
            }
            Some(Ok(WsMsg::Ping(_))) | Some(Ok(WsMsg::Pong(_))) => continue,
            Some(Err(_)) => break, // error = closed
            Some(Ok(_)) => {}
        }
    }
}

// ── Test 4: matchStart + tick observation ─────────────────────────────────────

#[tokio::test]
async fn ws_receives_match_start_and_tick() {
    let registry = TokenRegistry::new();
    let token = registry.register("bob");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    do_handshake(&mut ws, &token, "bob-bot").await;

    // Receive matchStart
    let match_start = next_text(&mut ws).await;
    assert_eq!(match_start["type"], "matchStart");

    // Receive first tick observation
    let tick = next_text(&mut ws).await;
    assert_eq!(tick["type"], "tick", "first message after matchStart is tick");
    assert_eq!(tick["tick"], 0, "first tick is tick 0");
    assert!(tick["maxTicks"].as_u64().is_some(), "maxTicks present");
    assert!(tick["seed"].as_u64().is_some(), "seed present");
    assert!(tick["arena"]["width"].as_f64().is_some(), "arena.width present");
    assert!(tick["arena"]["height"].as_f64().is_some(), "arena.height present");

    // self block must be present with required fields
    let self_view = &tick["self"];
    assert!(self_view.is_object(), "self is an object");
    assert!(self_view["id"].as_str().is_some(), "self.id present");
    assert!(self_view["pos"]["x"].as_f64().is_some(), "self.pos.x present");
    assert!(self_view["heading"].as_f64().is_some(), "self.heading present");
    assert!(self_view["hull"]["cur"].as_f64().is_some(), "self.hull.cur present");
    assert!(self_view["aether"]["cur"].as_f64().is_some(), "self.aether present");
}

// ── Test 5: private observation — no aether/sigil in ships array ─────────────

#[tokio::test]
async fn ws_tick_obs_is_private_view() {
    let registry = TokenRegistry::new();
    let token = registry.register("carol");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    do_handshake(&mut ws, &token, "carol-bot").await;

    // Consume matchStart
    let _ = next_text(&mut ws).await;

    // Receive tick 0 observation
    let tick = next_text(&mut ws).await;

    // The "self" block may have aether (it's the bot's own ship)
    assert!(tick["self"]["aether"].is_object(), "own aether present in self");

    // The "ships" array contains OTHER ships only — no aether or sigil
    let ships = tick["ships"].as_array().expect("ships is array");
    for ship in ships {
        assert!(
            ship.get("aether").is_none() || ship["aether"].is_null(),
            "enemy ship must not expose aether (fog rule): got {:?}",
            ship
        );
        assert!(
            ship.get("sigil").is_none() || ship["sigil"].is_null(),
            "enemy ship must not expose sigil (fog rule): got {:?}",
            ship
        );
    }
}

// ── Test 6: action before deadline is applied ────────────────────────────────

#[tokio::test]
async fn ws_action_before_deadline_is_applied() {
    // With seed=42, max_turn=0.15: sending turn=1.0 changes heading by +0.15 rad/tick.
    // Heading at tick 1 should differ from heading at tick 0 by ~0.15 rad.
    let registry = TokenRegistry::new();
    let token = registry.register("dave");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    do_handshake(&mut ws, &token, "dave-bot").await;
    let _ = next_text(&mut ws).await; // matchStart

    // Tick 0 observation
    let obs0 = next_text(&mut ws).await;
    let h0 = obs0["self"]["heading"].as_f64().expect("heading");

    // Send action: full left turn
    send_json(
        &mut ws,
        serde_json::json!({"type": "action", "turn": 1.0, "thrust": 0.0}),
    )
    .await;

    // Tick 1 observation
    let obs1 = next_text(&mut ws).await;
    let h1 = obs1["self"]["heading"].as_f64().expect("heading");

    // Heading must have changed by roughly max_turn = 0.15 rad (positive = CCW)
    let delta = h1 - h0;
    // Normalise to (-pi, pi) to handle wrap-around
    let delta = {
        let mut d = delta;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        d
    };
    assert!(
        (delta - 0.15).abs() < 0.01,
        "heading changed by ~max_turn after turn=1.0 action (delta={delta:.4})"
    );
}

// ── Test 7: missed deadline persists previous action ─────────────────────────

#[tokio::test]
async fn ws_missed_deadline_persists_previous_action() {
    // Tick 0: send turn=1.0 → heading changes by +0.15 rad.
    // Tick 1: send NO action (let deadline pass) → engine persists turn=1.0.
    // Heading at tick 2 should be heading at tick 0 + ~0.30 rad (two full turns).
    let registry = TokenRegistry::new();
    let token = registry.register("eve");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    do_handshake(&mut ws, &token, "eve-bot").await;
    let _ = next_text(&mut ws).await; // matchStart

    // Tick 0: record heading, send turn=1.0
    let obs0 = next_text(&mut ws).await;
    let h0 = obs0["self"]["heading"].as_f64().expect("heading at tick 0");
    send_json(
        &mut ws,
        serde_json::json!({"type": "action", "turn": 1.0, "thrust": 0.0}),
    )
    .await;

    // Tick 1: receive obs, do NOT send any action (miss deadline)
    let obs1 = next_text(&mut ws).await;
    let h1 = obs1["self"]["heading"].as_f64().expect("heading at tick 1");
    // Heading should have changed by ~0.15 from turn applied at tick 0
    let d01 = normalise_angle(h1 - h0);
    assert!(
        (d01 - 0.15).abs() < 0.02,
        "heading changed at tick 1 from tick 0 action (d01={d01:.4})"
    );
    // Wait longer than deadline to ensure it passes without sending action
    tokio::time::sleep(Duration::from_millis(TEST_DEADLINE_MS + 20)).await;

    // Tick 2: receive obs — turn=1.0 should have persisted
    let obs2 = next_text(&mut ws).await;
    let h2 = obs2["self"]["heading"].as_f64().expect("heading at tick 2");
    let d12 = normalise_angle(h2 - h1);
    assert!(
        (d12 - 0.15).abs() < 0.02,
        "heading still changed at tick 2 because turn=1.0 persisted (d12={d12:.4})"
    );
}

fn normalise_angle(mut a: f64) -> f64 {
    while a > std::f64::consts::PI {
        a -= 2.0 * std::f64::consts::PI;
    }
    while a < -std::f64::consts::PI {
        a += 2.0 * std::f64::consts::PI;
    }
    a
}

// ── Test 8: matchEnd received at match end ───────────────────────────────────

#[tokio::test]
async fn ws_receives_match_end() {
    // Play a match of TEST_MAX_TICKS ticks (= 8) with no actions.
    // At the end we must receive a well-formed matchEnd message.
    let registry = TokenRegistry::new();
    let token = registry.register("frank");
    let addr = spawn_server(test_app(registry)).await;
    let mut ws = ws_connect(addr).await;

    do_handshake(&mut ws, &token, "frank-bot").await;
    let _ = next_text(&mut ws).await; // matchStart

    // Drain ticks without sending actions; collect the last message
    let mut last: Option<Value> = None;
    // We expect TEST_MAX_TICKS tick messages followed by matchEnd.
    // Allow a generous number of iterations.
    for _ in 0..=TEST_MAX_TICKS + 5 {
        let msg = next_text(&mut ws).await;
        let msg_type = msg["type"].as_str().unwrap_or("").to_owned();
        if msg_type == "matchEnd" {
            last = Some(msg);
            break;
        }
        // It's a tick — just let the deadline pass (send nothing)
    }

    let match_end = last.expect("received matchEnd before timeout");
    assert_eq!(match_end["type"], "matchEnd");
    assert!(match_end["results"].is_object(), "results present");
    let scores = &match_end["results"]["scores"];
    assert!(scores.is_object(), "results.scores is an object");
    let ticks = match_end["results"]["ticks"].as_u64().expect("ticks");
    assert_eq!(
        ticks, TEST_MAX_TICKS as u64,
        "match ran for exactly max_ticks"
    );
}
