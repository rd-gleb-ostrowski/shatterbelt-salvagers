//! Tests for the Observer god-mode stream (issue 07).
//!
//! ## TDD tracer order
//!
//! 1. `observer_hub_publishes_one_frame_per_tick`
//!    ObserverHub delivers exactly one frame per tick step via subscribe().
//!
//! 2. `observer_frame_is_full_god_view_not_fog_filtered`
//!    Received frame contains bot-hidden fields: all ships expose `aether` and
//!    `sigil`; the frame is not fog-filtered.
//!
//! 3. `observer_frame_tick_advances_with_match`
//!    The `tick` field in successive frames increments with the match.
//!
//! 4. `viewer_ws_receives_god_view_frames`
//!    End-to-end: a Viewer WS client connecting to `/observe` receives
//!    `"godView"` frames while a bot match runs on the same server.
//!
//! 5. `bot_does_not_receive_god_frames`
//!    A bot's WS socket receives only `tick`/`matchStart`/`matchEnd`
//!    messages — never `"godView"`.
//!
//! 6. `viewer_sending_bytes_is_ignored`
//!    A Viewer sending arbitrary bytes does not crash the server or affect
//!    the match; the bot still completes normally.

use std::time::Duration;

use arena_engine::{Engine, Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    auth::TokenRegistry,
    observer::ObserverHub,
    routes::{RouterConfig, build_router_config},
    store::WasmBotStore,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};

// ── Constants ─────────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "obs-secret";
const TEST_DEADLINE_MS: u64 = 50;
const TEST_MAX_TICKS: u32 = 5;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a test router with a short match and an exposed hub clone.
fn test_app_with_hub(
    registry: std::sync::Arc<TokenRegistry>,
) -> (axum::Router, ObserverHub) {
    let hub = ObserverHub::new();
    let app = build_router_config(RouterConfig {
        event_password: EVENT_PASSWORD.to_owned(),
        facilitator_password: "test-facilitator".to_owned(),
        registry,
        wasm_store: WasmBotStore::new(),
        ws_registry: arena_server::resolver::WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(TEST_DEADLINE_MS),
        match_seed: 42,
        match_params: Params { max_ticks: TEST_MAX_TICKS, ..Params::default() },
        observer_hub: hub.clone(),
        recording_store: arena_server::recording::RecordingStore::new(),
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
        ladder: arena_server::ladder::Ladder::new(),
    });
    (app, hub)
}

/// Bind an ephemeral port, spawn the server, return the bound address.
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
        .expect("register");
    resp.json::<Value>().await.expect("JSON")["token"]
        .as_str()
        .expect("token")
        .to_owned()
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Connect to ws://<addr>/<path>.
async fn ws_connect(addr: std::net::SocketAddr, path: &str) -> WsStream {
    let url = format!("ws://{addr}{path}");
    let (stream, _) = connect_async(&url).await.expect("ws connect");
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

/// Try to receive the next text frame with a timeout; returns None on timeout.
async fn try_next_text(ws: &mut WsStream, ms: u64) -> Option<Value> {
    tokio::time::timeout(Duration::from_millis(ms), next_text(ws))
        .await
        .ok()
}

/// Send a JSON value as a text frame.
async fn send_json(ws: &mut WsStream, v: Value) {
    ws.send(WsMsg::Text(serde_json::to_string(&v).unwrap().into()))
        .await
        .expect("send");
}

/// Full WS bot handshake: welcome → join → assigned → matchStart.
async fn bot_handshake(ws: &mut WsStream, token: &str) {
    // welcome
    let welcome = next_text(ws).await;
    let session_id = welcome["sessionId"].as_str().unwrap().to_owned();
    // join
    send_json(
        ws,
        serde_json::json!({
            "type": "join",
            "sessionId": session_id,
            "token": token,
            "name": "obs-test-bot"
        }),
    )
    .await;
    // assigned
    let _ = next_text(ws).await;
    // matchStart
    let _ = next_text(ws).await;
}

// ── Test 1: hub delivers one frame per tick ───────────────────────────────────

/// RED→GREEN: subscribing to the hub and stepping the engine N times delivers
/// exactly N frames.
#[tokio::test]
async fn observer_hub_publishes_one_frame_per_tick() {
    let hub = ObserverHub::new();
    let mut rx = hub.subscribe();

    let params = Params { max_ticks: 3, ..Params::default() };
    let specs = vec![
        ShipSpec {
            id: "s0".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.25, params.arena_h * 0.5),
        },
        ShipSpec {
            id: "s1".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.75, params.arena_h * 0.5),
        },
    ];
    let mut engine = Engine::new(1, params, specs);

    // Step twice, publishing god view after each step (mirroring the tick loop).
    engine.step(vec![]);
    hub.publish_god_view(&engine.god_view());

    engine.step(vec![]);
    hub.publish_god_view(&engine.god_view());

    // Should receive exactly 2 frames, no more.
    let f1 = rx.try_recv().expect("frame 1");
    let f2 = rx.try_recv().expect("frame 2");
    assert!(rx.try_recv().is_err(), "no third frame");

    let v1: Value = serde_json::from_str(&f1).expect("frame 1 is JSON");
    let v2: Value = serde_json::from_str(&f2).expect("frame 2 is JSON");
    assert_eq!(v1["type"], "godView", "frame 1 type");
    assert_eq!(v2["type"], "godView", "frame 2 type");
}

// ── Test 2: frame is full god-view, not fog-filtered ─────────────────────────

/// RED→GREEN: a god-view frame exposes `aether` and `sigil` on every ship —
/// fields that are hidden from bots in per-bot `Observation`s.
#[tokio::test]
async fn observer_frame_is_full_god_view_not_fog_filtered() {
    let hub = ObserverHub::new();
    let mut rx = hub.subscribe();

    let params = Params { max_ticks: 3, ..Params::default() };
    let specs = vec![
        ShipSpec {
            id: "s0".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.25, params.arena_h * 0.5),
        },
        ShipSpec {
            id: "s1".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.75, params.arena_h * 0.5),
        },
    ];
    let mut engine = Engine::new(42, params, specs);

    engine.step(vec![]);
    hub.publish_god_view(&engine.god_view());

    let frame_str = rx.try_recv().expect("one frame");
    let frame: Value = serde_json::from_str(&frame_str).expect("valid JSON");

    assert_eq!(frame["type"], "godView");

    let ships = frame["ships"].as_array().expect("ships is array");
    assert!(!ships.is_empty(), "ships array is non-empty");

    for ship in ships {
        // aether MUST be present — it is hidden in per-bot OtherShipView
        assert!(
            ship["aether"].is_object(),
            "god-view ship must expose aether (bot-hidden field): ship={ship:?}"
        );
        assert!(
            ship["aether"]["cur"].as_f64().is_some(),
            "aether.cur must be numeric: ship={ship:?}"
        );

        // sigil MUST be a field (null when no sigil held, or a string)
        assert!(
            ship.get("sigil").is_some(),
            "god-view ship must expose sigil key (bot-hidden field): ship={ship:?}"
        );

        // headings etc. sanity check
        assert!(ship["pos"].is_object(), "pos present");
        assert!(ship["hull"].is_object(), "hull present");
    }

    // The god view also exposes fields absent from bot observations:
    // scores, arena, tick, maxTicks, seed.
    assert!(frame["scores"].is_object(), "scores present");
    assert!(frame["arena"]["width"].as_f64().is_some(), "arena.width present");
    assert!(frame["tick"].as_u64().is_some(), "tick present");
    assert!(frame["maxTicks"].as_u64().is_some(), "maxTicks present");
    assert!(frame["seed"].as_u64().is_some(), "seed present");
}

// ── Test 3: tick advances with the match ──────────────────────────────────────

/// RED→GREEN: successive god-view frames report an incrementing `tick` value.
#[tokio::test]
async fn observer_frame_tick_advances_with_match() {
    let hub = ObserverHub::new();
    let mut rx = hub.subscribe();

    let params = Params { max_ticks: 5, ..Params::default() };
    let specs = vec![
        ShipSpec {
            id: "s0".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.25, params.arena_h * 0.5),
        },
        ShipSpec {
            id: "s1".into(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.75, params.arena_h * 0.5),
        },
    ];
    let mut engine = Engine::new(7, params, specs);

    let n = 4u32;
    for _ in 0..n {
        engine.step(vec![]);
        hub.publish_god_view(&engine.god_view());
    }

    let mut ticks = Vec::new();
    for _ in 0..n {
        let f: Value = serde_json::from_str(&rx.try_recv().expect("frame")).unwrap();
        ticks.push(f["tick"].as_u64().expect("tick field") as u32);
    }

    // Ticks should be strictly increasing: 1, 2, 3, 4
    // (tick is engine.tick() after step, so starts at 1 after first step)
    for i in 1..ticks.len() {
        assert!(
            ticks[i] > ticks[i - 1],
            "tick must increase: ticks[{i}]={} vs ticks[{}]={}",
            ticks[i],
            i - 1,
            ticks[i - 1]
        );
    }
    // First tick after first step should be 1
    assert_eq!(ticks[0], 1, "tick after first step is 1");
}

// ── Test 4: Viewer WS receives godView frames (end-to-end) ───────────────────

/// RED→GREEN: a Viewer WS client connecting to `/observe` receives `"godView"`
/// frames while a bot match runs on the same server.
#[tokio::test]
async fn viewer_ws_receives_god_view_frames() {
    let registry = TokenRegistry::new();
    let token = registry.register("viewer-test-bot");
    let (app, _hub) = test_app_with_hub(registry);
    let addr = spawn_server(app).await;

    // Connect Viewer BEFORE the bot so it subscribes before frames start.
    let mut viewer = ws_connect(addr, "/observe").await;

    // Connect bot and play through the match.
    let mut bot = ws_connect(addr, "/ws").await;
    bot_handshake(&mut bot, &token).await;

    // Drain the bot's ticks (to drive the match forward) while viewer collects frames.
    // Play through all TEST_MAX_TICKS ticks by receiving bot observations.
    let mut god_frames: Vec<Value> = Vec::new();

    // We race: bot draining vs viewer collecting. Use tokio tasks.
    let bot_task = tokio::spawn(async move {
        for _ in 0..TEST_MAX_TICKS + 5 {
            let msg = tokio::time::timeout(
                Duration::from_millis(TEST_DEADLINE_MS * 4),
                next_text(&mut bot),
            )
            .await;
            match msg {
                Ok(m) if m["type"] == "matchEnd" => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    // Collect god-view frames (with a deadline so test doesn't hang).
    let deadline = Duration::from_millis((TEST_MAX_TICKS as u64 + 5) * TEST_DEADLINE_MS * 6);
    let start = tokio::time::Instant::now();
    while start.elapsed() < deadline && god_frames.len() < TEST_MAX_TICKS as usize {
        if let Some(frame) =
            try_next_text(&mut viewer, TEST_DEADLINE_MS * 4).await
        {
            if frame["type"] == "godView" {
                god_frames.push(frame);
            }
        }
    }

    bot_task.await.expect("bot task");

    assert!(
        !god_frames.is_empty(),
        "Viewer must receive at least one godView frame"
    );

    // All frames must be well-formed godView frames.
    for (i, frame) in god_frames.iter().enumerate() {
        assert_eq!(frame["type"], "godView", "frame {i} type must be godView");
        assert!(frame["tick"].as_u64().is_some(), "frame {i} tick present");
        let ships = frame["ships"].as_array().expect("ships array in frame {i}");
        assert!(!ships.is_empty(), "frame {i} ships non-empty");
        // Viewer frames include bot-hidden aether
        assert!(
            ships[0]["aether"].is_object(),
            "frame {i} ship[0] exposes aether"
        );
    }
}

// ── Test 5: bot does not receive godView frames ───────────────────────────────

/// RED→GREEN: a WS bot's socket receives only `tick`, `matchStart`, and
/// `matchEnd` messages — never a `"godView"` frame.
#[tokio::test]
async fn bot_does_not_receive_god_frames() {
    let registry = TokenRegistry::new();
    let token = registry.register("privacy-bot");
    let (app, _hub) = test_app_with_hub(registry);
    let addr = spawn_server(app).await;

    let mut bot = ws_connect(addr, "/ws").await;
    bot_handshake(&mut bot, &token).await;

    // Collect all messages the bot receives until matchEnd.
    let deadline = Duration::from_millis((TEST_MAX_TICKS as u64 + 5) * TEST_DEADLINE_MS * 6);
    let start = tokio::time::Instant::now();
    let mut received_types: Vec<String> = Vec::new();

    while start.elapsed() < deadline {
        match tokio::time::timeout(Duration::from_millis(TEST_DEADLINE_MS * 4), next_text(&mut bot))
            .await
        {
            Ok(msg) => {
                let t = msg["type"].as_str().unwrap_or("unknown").to_owned();
                let is_end = t == "matchEnd";
                received_types.push(t);
                if is_end {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    // The bot must have received at least one tick and no godView frames.
    assert!(
        received_types.contains(&"tick".to_owned()),
        "bot must receive tick messages; got: {received_types:?}"
    );
    assert!(
        !received_types.contains(&"godView".to_owned()),
        "bot must NEVER receive godView frames; got: {received_types:?}"
    );
}

// ── Test 6: viewer sending bytes is ignored ───────────────────────────────────

/// RED→GREEN: a Viewer sending arbitrary bytes does not crash the server or
/// affect match progression; the bot still receives `matchEnd`.
#[tokio::test]
async fn viewer_sending_bytes_is_ignored() {
    let registry = TokenRegistry::new();
    let token = registry.register("ignored-sender-bot");
    let (app, _hub) = test_app_with_hub(registry);
    let addr = spawn_server(app).await;

    // Connect viewer first.
    let mut viewer = ws_connect(addr, "/observe").await;

    // Connect bot.
    let mut bot = ws_connect(addr, "/ws").await;
    bot_handshake(&mut bot, &token).await;

    // Viewer spams the server with arbitrary data.
    let spam_task = tokio::spawn(async move {
        for _ in 0..10u8 {
            let _ = viewer
                .send(WsMsg::Text(
                    r#"{"type":"hack","payload":"ignored"}"#.into(),
                ))
                .await;
            let _ = viewer.send(WsMsg::Binary(vec![0xFF, 0xAB, 0x00].into())).await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Bot plays through the full match normally.
    let deadline = Duration::from_millis((TEST_MAX_TICKS as u64 + 5) * TEST_DEADLINE_MS * 8);
    let start = tokio::time::Instant::now();
    let mut got_match_end = false;

    while start.elapsed() < deadline {
        match tokio::time::timeout(
            Duration::from_millis(TEST_DEADLINE_MS * 4),
            next_text(&mut bot),
        )
        .await
        {
            Ok(msg) if msg["type"] == "matchEnd" => {
                got_match_end = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    spam_task.await.expect("spam task");

    assert!(
        got_match_end,
        "bot must receive matchEnd even when Viewer sends unsolicited data"
    );
}
