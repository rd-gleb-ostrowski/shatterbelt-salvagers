//! Integration tests for the three new facilitator-gated bot/team-management
//! capabilities (issue 13).
//!
//! ## Capabilities under test
//!
//! 1. **Admin upload** — `POST /admin/bots/{team}` (facilitator-gated WASM upload)
//! 2. **Enable/disable** — `POST /admin/bots/{team}/disable` and `.../enable`
//! 3. **Set/clear Default Bot** — `POST /admin/default-bot` and `DELETE /admin/default-bot`
//!
//! ## TDD slice order (RED → GREEN)
//!
//! 1.  `admin_upload_without_auth_returns_401`
//! 2.  `admin_upload_with_auth_and_valid_wasm_returns_200_and_stores`
//! 3.  `admin_upload_with_auth_and_invalid_body_returns_400`
//! 4.  `disable_bot_returns_200_and_is_disabled`
//! 5.  `disabled_team_resolves_to_default_kind`
//! 6.  `enable_bot_returns_200_and_restores_enabled`
//! 7.  `enabled_team_resolves_to_wasm_kind`
//! 8.  `disable_enable_without_auth_returns_401`
//! 9.  `set_default_bot_with_auth_and_valid_wasm_returns_200`
//! 10. `resolver_with_default_bot_store_resolves_empty_slot_to_wasm_kind`
//! 11. `delete_default_bot_returns_200_and_clears`
//! 12. `resolver_after_clear_falls_back_to_builtin_default`
//! 13. `default_bot_endpoints_require_auth`

use arena_engine::{Intent, Observation, Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    auth::TokenRegistry,
    observer::ObserverHub,
    recording::RecordingStore,
    resolver::{BotSessionSource, ConnectionResolver, Slot, WsConnectionRegistry},
    routes::{build_router_config, RouterConfig},
    runner::BotDriver,
    store::{DefaultBotStore, DisabledStore, WasmBotStore},
};

use std::{sync::Arc, time::Duration};
use axum::body::Body;
use http::{Method, Request, StatusCode};
use tower::ServiceExt;

// ── Constants ──────────────────────────────────────────────────────────────────

const FACILITATOR_PASSWORD: &str = "test-facilitator";
const FACILITATOR_HEADER: &str = "Facilitator test-facilitator";

/// Minimal valid WASM magic header (`\0asm` version 1).
const WASM_MAGIC: &[u8] = b"\x00asm\x01\x00\x00\x00";

/// A minimal WAT bot that returns `{"thrust":1.0}` — validates the full
/// alloc/init/tick round-trip used by WasmBotDriver.
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

fn wat_to_wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT assembly failed")
}

// ── Test helpers ───────────────────────────────────────────────────────────────

/// Build a test router that exposes `wasm_store`, `disabled_store`, and
/// `default_bot_store` for post-request assertion.
fn test_app() -> (
    axum::Router,
    Arc<WasmBotStore>,
    Arc<DisabledStore>,
    Arc<DefaultBotStore>,
) {
    let wasm_store = WasmBotStore::new();
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();
    let app = build_router_config(RouterConfig {
        event_password: "test-event".to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry: TokenRegistry::new(),
        wasm_store: Arc::clone(&wasm_store),
        ws_registry: WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params::default(),
        observer_hub: ObserverHub::new(),
        recording_store: RecordingStore::new(),
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
        ladder: arena_server::ladder::Ladder::new(),
        disabled_store: Arc::clone(&disabled_store),
        default_bot_store: Arc::clone(&default_bot_store),
        ladder_runner: arena_server::admin::LadderRunner::new(),
    });
    (app, wasm_store, disabled_store, default_bot_store)
}

async fn http_raw(
    app: axum::Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Vec<u8>,
) -> StatusCode {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(a) = auth {
        builder = builder.header("authorization", a);
    }
    let req = builder.body(Body::from(body)).unwrap();
    app.oneshot(req).await.unwrap().status()
}

// ── Stub session for resolver tests ──────────────────────────────────────────

struct StubWsSession;
struct StubWsDriver;

impl BotDriver for StubWsDriver {
    fn decide(&mut self, _tick: u32, _obs: &Observation) -> Option<Intent> {
        Some(Intent::default())
    }
    fn kind(&self) -> &'static str {
        "ws"
    }
}

impl BotSessionSource for StubWsSession {
    fn make_driver(
        &self,
        _deadline: Duration,
        _health: Option<std::sync::Arc<arena_server::health::BotHealthEntry>>,
    ) -> Box<dyn BotDriver> {
        Box::new(StubWsDriver)
    }
    fn try_send_envelope(&self, _json: String) -> bool {
        true
    }
}

fn make_slot(team: &str) -> Slot {
    Slot { team: team.to_owned(), tick0_obs_json: String::new() }
}

fn make_spec(team: &str, params: &Params) -> ShipSpec {
    ShipSpec {
        id: team.to_owned(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::new(params.arena_w * 0.5, params.arena_h * 0.5),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 1: POST /admin/bots/{team} — auth guard
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn admin_upload_without_auth_returns_401() {
    let (app, _, _, _) = test_app();
    let status =
        http_raw(app, Method::POST, "/admin/bots/team-a", None, WASM_MAGIC.to_vec()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 2: POST /admin/bots/{team} — valid upload stores artifact
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn admin_upload_with_auth_and_valid_wasm_returns_200_and_stores() {
    let (app, wasm_store, _, _) = test_app();
    let status = http_raw(
        app,
        Method::POST,
        "/admin/bots/team-alpha",
        Some(FACILITATOR_HEADER),
        WASM_MAGIC.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        wasm_store.get("team-alpha").is_some(),
        "artifact must be persisted in wasm_store"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 3: POST /admin/bots/{team} — non-wasm body → 400
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn admin_upload_with_auth_and_invalid_body_returns_400() {
    let (app, _, _, _) = test_app();
    let status = http_raw(
        app,
        Method::POST,
        "/admin/bots/team-alpha",
        Some(FACILITATOR_HEADER),
        b"not-wasm".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 4: POST /admin/bots/{team}/disable → 200, is_disabled true
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disable_bot_returns_200_and_is_disabled() {
    let (app, _, disabled_store, _) = test_app();
    let status = http_raw(
        app,
        Method::POST,
        "/admin/bots/team-a/disable",
        Some(FACILITATOR_HEADER),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(disabled_store.is_disabled("team-a"), "team-a must be marked disabled");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 5: disabled team resolver → Default Bot kind
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disabled_team_resolves_to_default_kind() {
    let wasm_store = WasmBotStore::new();
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();

    // Upload a valid WASM artifact for team-a.
    wasm_store.store("team-a", wat_to_wasm(CONST_ACTION_WAT));

    // Disable team-a.
    disabled_store.disable("team-a");

    let ws_registry = WsConnectionRegistry::new();
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    )
    .with_management(Arc::clone(&disabled_store), Arc::clone(&default_bot_store));

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("team-a")], &params);
    assert_eq!(
        drivers[0].kind(),
        "default",
        "disabled team must resolve to built-in default bot, not wasm"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 6: POST /admin/bots/{team}/enable → 200, is_disabled false
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn enable_bot_returns_200_and_restores_enabled() {
    let (app, _, disabled_store, _) = test_app();
    // Pre-disable.
    disabled_store.disable("team-a");

    let status = http_raw(
        app,
        Method::POST,
        "/admin/bots/team-a/enable",
        Some(FACILITATOR_HEADER),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!disabled_store.is_disabled("team-a"), "team-a must be re-enabled");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 7: enabled team with WASM artifact resolves to wasm kind
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn enabled_team_resolves_to_wasm_kind() {
    let wasm_store = WasmBotStore::new();
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();

    // Upload valid WASM artifact.
    wasm_store.store("team-a", wat_to_wasm(CONST_ACTION_WAT));

    // Disable then immediately re-enable — should restore WASM resolution.
    disabled_store.disable("team-a");
    disabled_store.enable("team-a");

    let ws_registry = WsConnectionRegistry::new();
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    )
    .with_management(Arc::clone(&disabled_store), Arc::clone(&default_bot_store));

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("team-a")], &params);
    assert_eq!(
        drivers[0].kind(),
        "wasm",
        "re-enabled team with artifact must resolve to wasm driver"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 8: disable/enable without auth → 401
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disable_without_auth_returns_401() {
    let (app, _, _, _) = test_app();
    let status =
        http_raw(app, Method::POST, "/admin/bots/team-a/disable", None, vec![]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn enable_without_auth_returns_401() {
    let (app, _, _, _) = test_app();
    let status =
        http_raw(app, Method::POST, "/admin/bots/team-a/enable", None, vec![]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 9: POST /admin/default-bot → 200, DefaultBotStore has bytes
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn set_default_bot_with_auth_and_valid_wasm_returns_200() {
    let (app, _, _, default_bot_store) = test_app();
    let status = http_raw(
        app,
        Method::POST,
        "/admin/default-bot",
        Some(FACILITATOR_HEADER),
        WASM_MAGIC.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        default_bot_store.get().is_some(),
        "DefaultBotStore must contain the uploaded bytes"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 10: resolver with DefaultBotStore → empty slot gets wasm kind
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn resolver_with_default_bot_store_resolves_empty_slot_to_wasm_kind() {
    let wasm_store = WasmBotStore::new(); // empty — no team artifact
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();

    // Set a real WASM artifact as the custom Default Bot.
    default_bot_store.set(wat_to_wasm(CONST_ACTION_WAT));

    let ws_registry = WsConnectionRegistry::new();
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    )
    .with_management(Arc::clone(&disabled_store), Arc::clone(&default_bot_store));

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("no-bot-team")], &params);
    assert_eq!(
        drivers[0].kind(),
        "wasm",
        "empty slot with custom DefaultBotStore must resolve to wasm driver"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 11: DELETE /admin/default-bot → 200, store cleared
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delete_default_bot_returns_200_and_clears() {
    let (app, _, _, default_bot_store) = test_app();
    // Pre-set a bot.
    default_bot_store.set(WASM_MAGIC.to_vec());

    let status = http_raw(
        app,
        Method::DELETE,
        "/admin/default-bot",
        Some(FACILITATOR_HEADER),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        default_bot_store.get().is_none(),
        "DefaultBotStore must be empty after DELETE"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 12: resolver after clear → built-in default kind
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn resolver_after_clear_falls_back_to_builtin_default() {
    let wasm_store = WasmBotStore::new();
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();

    // Set then clear.
    default_bot_store.set(wat_to_wasm(CONST_ACTION_WAT));
    default_bot_store.clear();

    let ws_registry = WsConnectionRegistry::new();
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    )
    .with_management(Arc::clone(&disabled_store), Arc::clone(&default_bot_store));

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("no-bot-team")], &params);
    assert_eq!(
        drivers[0].kind(),
        "default",
        "after clearing DefaultBotStore, resolver must fall back to built-in default"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Slice 13: default-bot endpoints require facilitator auth
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn post_default_bot_without_auth_returns_401() {
    let (app, _, _, _) = test_app();
    let status =
        http_raw(app, Method::POST, "/admin/default-bot", None, WASM_MAGIC.to_vec()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delete_default_bot_without_auth_returns_401() {
    let (app, _, _, _) = test_app();
    let status = http_raw(app, Method::DELETE, "/admin/default-bot", None, vec![]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Extra: disabled team with WS bot registered → still gets default (not ws)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disabled_team_with_ws_bot_still_resolves_to_default() {
    let wasm_store = WasmBotStore::new();
    let disabled_store = DisabledStore::new();
    let default_bot_store = DefaultBotStore::new();

    // Disable team-a.
    disabled_store.disable("team-a");

    let ws_registry = WsConnectionRegistry::new();
    // Insert a stub WS driver for team-a — should be bypassed.
    ws_registry.register("team-a", Arc::new(StubWsSession));

    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    )
    .with_management(Arc::clone(&disabled_store), Arc::clone(&default_bot_store));

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("team-a")], &params);
    assert_eq!(
        drivers[0].kind(),
        "default",
        "disabled team must use Default Bot even when WS driver is registered"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Extra: existing resolver behavior unchanged when management stores absent
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn resolver_without_management_uses_builtin_default_for_empty_slot() {
    let wasm_store = WasmBotStore::new();
    let ws_registry = WsConnectionRegistry::new();

    // No with_management call — existing behavior must be unchanged.
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    );

    let params = Params::default();
    let mut drivers = resolver.resolve(&[make_slot("any-team")], &params);
    assert_eq!(
        drivers[0].kind(),
        "default",
        "without management stores, empty slot must still resolve to built-in default"
    );
}
