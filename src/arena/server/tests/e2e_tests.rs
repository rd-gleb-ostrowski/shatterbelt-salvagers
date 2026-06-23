//! End-to-end integration tests for the Arena server.
//!
//! These tests boot the **real** axum server on ephemeral TCP ports, connect
//! real WebSocket clients (tokio-tungstenite), and drive real HTTP requests
//! (reqwest).  They guard the integration wiring that unit/oneshot tests do
//! not cover.
//!
//! ## Tests
//!
//! - [`e2e_spectator_watches_live_match`] — proves a spectator can watch a
//!   live match via `GET /observe` and receive `godView` frames.
//! - [`e2e_recordings_survive_restart`] — proves that recordings written to
//!   disk by server #1 are listed and replayable by server #2 booted from
//!   the same directory (simulating a server restart).
//!
//! All waiting is bounded with `tokio::time::timeout`; no test can hang.

use std::path::PathBuf;
use std::time::Duration;

use arena_server::recording::RecordingStore;
use arena_server::routes::{build_app, build_app_with_store};
use futures_util::future::join_all;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};

// ── Constants ─────────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "e2e-event";
const FACILITATOR_PASSWORD: &str = "e2e-facilitator";

// ── Type alias ────────────────────────────────────────────────────────────────

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

// ── Server helpers ────────────────────────────────────────────────────────────

/// Bind an ephemeral port, spawn the server with `build_app` (in-memory
/// recording store), and return `(base_url, port, join_handle)`.
async fn start_api_server() -> (String, u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind");
    let port = listener.local_addr().unwrap().port();
    let router = build_app(
        EVENT_PASSWORD.to_owned(),
        FACILITATOR_PASSWORD.to_owned(),
        None,
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    (format!("http://127.0.0.1:{port}"), port, handle)
}

/// Bind an ephemeral port, spawn the server with `build_app_with_store` using
/// a disk-backed [`RecordingStore`] rooted at `dir`, and return
/// `(base_url, join_handle)`.
async fn start_server_with_store(dir: PathBuf) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind");
    let port = listener.local_addr().unwrap().port();
    let store = RecordingStore::with_dir(dir);
    let router = build_app_with_store(
        EVENT_PASSWORD.to_owned(),
        FACILITATOR_PASSWORD.to_owned(),
        None,
        store,
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// Build a reqwest client with a generous per-request timeout.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

/// Collect up to `n` `godView` frames from a WS stream, skipping pings/pongs
/// and non-godView text messages.  Returns as soon as `n` frames arrive or
/// the stream closes.
async fn collect_god_view_frames(ws: &mut WsStream, n: usize) -> Vec<Value> {
    let mut frames = Vec::new();
    while frames.len() < n {
        match ws.next().await {
            Some(Ok(WsMsg::Text(t))) => {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if v["type"] == "godView" {
                        frames.push(v);
                    }
                }
            }
            Some(Ok(WsMsg::Ping(_))) | Some(Ok(WsMsg::Pong(_))) => continue,
            _ => break, // stream closed or error
        }
    }
    frames
}

// ── E2E-1: Spectator watches a live match ────────────────────────────────────

/// E2E-1 — A spectator connected to `/observe` receives `godView` frames while
/// a live match is running.
///
/// ## What is asserted
///
/// 1. `POST /admin/matches {mode:"live", seed:42}` returns **200** with a
///    `matchId` in the response.
/// 2. At least **5 `godView` frames** arrive on the `/observe` WS within 5 s.
/// 3. Every frame has `type == "godView"`, a `ships` array, and an `events`
///    array.
/// 4. `tick` strictly increases across the collected frames.
///
/// ## Hang prevention
///
/// Uses `flavor = "multi_thread"` so the spawn_blocking match loop and the
/// async WS forwarder run truly in parallel (no single-thread starvation).
/// The WS connection and the frame-collection loop are both wrapped in
/// `tokio::time::timeout`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_spectator_watches_live_match() {
    let (base_url, port, _handle) = start_api_server().await;

    // Subscribe to /observe BEFORE starting the match so the broadcast
    // receiver is live when the first frames are published.
    let ws_url = format!("ws://127.0.0.1:{port}/observe");
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(5), connect_async(&ws_url))
        .await
        .expect("WS connect timed out")
        .expect("WS handshake failed");

    // Start a live match with a short cap (60 ticks ≈ 2 s).
    let c = http_client();
    register_teams(&c, &base_url).await;

    let resp = c
        .post(format!("{base_url}/admin/matches"))
        .header(
            "Authorization",
            format!("Facilitator {FACILITATOR_PASSWORD}"),
        )
        .json(&serde_json::json!({"mode": "live", "seed": 42, "maxTicks": 60}))
        .send()
        .await
        .expect("POST /admin/matches request failed");
    assert_eq!(resp.status(), 200, "POST /admin/matches must return 200");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["mode"], "live", "response mode must be 'live'");
    let match_id = body["matchId"]
        .as_str()
        .expect("matchId must be present in response")
        .to_owned();

    // Collect godView frames, bounded to 5 s.  At 30 TPS, 5 frames ≈ 167 ms.
    let frames = tokio::time::timeout(Duration::from_secs(5), collect_god_view_frames(&mut ws, 5))
        .await
        .expect("timed out waiting for godView frames from /observe");

    // Abort the live match so the spawn_blocking thread exits quickly
    // (prevents the multi-thread runtime from waiting up to 2 s for it).
    let _ = c
        .delete(format!("{base_url}/admin/matches/{match_id}"))
        .header(
            "Authorization",
            format!("Facilitator {FACILITATOR_PASSWORD}"),
        )
        .send()
        .await;
    // Allow up to 50 ms for the blocking thread to observe the abort flag.
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        frames.len() >= 5,
        "expected ≥5 godView frames, got {}",
        frames.len()
    );

    // Every frame must carry the required fields.
    for frame in &frames {
        assert_eq!(frame["type"], "godView", "type must be 'godView'");
        assert!(frame["ships"].is_array(), "ships must be an array");
        assert!(frame["events"].is_array(), "events must be an array");
        assert!(frame["tick"].as_u64().is_some(), "tick must be a u64");
    }

    // tick must advance across the window of collected frames.
    let first_tick = frames.first().unwrap()["tick"].as_u64().unwrap();
    let last_tick = frames.last().unwrap()["tick"].as_u64().unwrap();
    assert!(
        last_tick > first_tick,
        "tick must increase: first={first_tick} last={last_tick}"
    );
    _handle.abort();
}

async fn register_teams(c: &Client, base_url: &str) {
    join_all(["e2e-team-1", "e2e-team-2"].map(async |t| {
        c.post(format!("{base_url}/register"))
            .json(&serde_json::json!({"password":EVENT_PASSWORD,"team":t}))
            .send()
            .await
            .expect("POST /register failed")
    }))
    .await;
}

// ── E2E-2: Recordings survive a server restart ────────────────────────────────

/// E2E-2 — Recordings written to disk by server #1 are visible and replayable
/// on server #2 booted from the same directory (simulating a restart).
///
/// ## What is asserted
///
/// 1. `POST /admin/matches {mode:"headless", seed:7}` on server #1 returns
///    **200** with a `matchId`.  (Headless runs synchronously, so the
///    recording is on disk before the HTTP response arrives.)
/// 2. `GET /recordings` on server #1 lists the `matchId`.
/// 3. After aborting server #1 and booting server #2 from the **same temp
///    dir**, `GET /recordings` on server #2 still lists the `matchId` (loaded
///    from the persisted JSON file on startup).
/// 4. `POST /recordings/{id}/replay` on server #2 returns **200** — the
///    recording is fully replayable after the restart.
///
/// ## Hang prevention
///
/// reqwest has a 10 s per-request timeout; the test as a whole is async and
/// can be bounded by the test runner.
#[tokio::test]
async fn e2e_recordings_survive_restart() {
    // Persistent temp dir: both server instances share this path.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let tmp_path = tmp.path().to_path_buf();

    // ── Server #1 ─────────────────────────────────────────────────────────────
    let (base_url1, handle1) = start_server_with_store(tmp_path.clone()).await;
    let c = http_client();
    register_teams(&c, &base_url1).await;
    // Run a headless match.  The handler blocks until the match finishes and
    // the recording has been written to disk, then returns.
    let resp = c
        .post(format!("{base_url1}/admin/matches"))
        .header(
            "Authorization",
            format!("Facilitator {FACILITATOR_PASSWORD}"),
        )
        .json(&serde_json::json!({"mode": "headless", "seed": 7}))
        .send()
        .await
        .expect("POST /admin/matches (headless) request failed");
    assert_eq!(
        resp.status(),
        200,
        "POST /admin/matches (headless) must return 200"
    );
    let body: Value = resp.json().await.unwrap();
    let match_id = body["matchId"]
        .as_str()
        .expect("matchId in response")
        .to_owned();

    // Verify recording is immediately listed on server #1.
    let resp = c
        .get(format!("{base_url1}/recordings"))
        .send()
        .await
        .expect("GET /recordings on server #1 failed");
    assert_eq!(resp.status(), 200);
    let recordings: Vec<Value> = resp.json().await.unwrap();
    assert!(
        recordings.iter().any(|r| r["matchId"] == match_id),
        "server #1 must list matchId={match_id}; listings: {recordings:?}"
    );

    // Shut down server #1 (simulating a restart).
    handle1.abort();
    // Brief yield so the runtime drains the aborted task before we continue.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── Server #2 (simulated restart — same dir, fresh process state) ─────────
    let (base_url2, _handle2) = start_server_with_store(tmp_path.clone()).await;

    // RecordingStore::with_dir scans the dir on construction, so the recording
    // written by server #1 is already in memory when server #2 handles requests.
    let resp = c
        .get(format!("{base_url2}/recordings"))
        .send()
        .await
        .expect("GET /recordings on server #2 failed");
    assert_eq!(resp.status(), 200);
    let recordings2: Vec<Value> = resp.json().await.unwrap();
    assert!(
        recordings2.iter().any(|r| r["matchId"] == match_id),
        "server #2 must list matchId={match_id} after restart; listings: {recordings2:?}"
    );

    // The recording must still be replayable through server #2's observer hub.
    let resp = c
        .post(format!("{base_url2}/recordings/{match_id}/replay"))
        .send()
        .await
        .expect("POST /recordings/{id}/replay on server #2 failed");
    assert_eq!(resp.status(), 200, "replay after restart must return 200");
}
