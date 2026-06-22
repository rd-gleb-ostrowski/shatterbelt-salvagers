//! Integration tests for the registration & token endpoint (issue 02).
//!
//! Tests assert **observable HTTP behaviour** via `tower::ServiceExt::oneshot`
//! — no real TCP port, no sleeping. The test app is constructed with a known
//! event password so results are deterministic.
//!
//! TDD order: one test written RED then driven GREEN before the next.
//!
//! 1. correct password returns 200 + non-empty token
//! 2. wrong password returns 401
//! 3. missing password field returns 4xx
//! 4. returned token resolves to the registered team identity
//! 5. two different teams get distinct tokens
//! 6. re-registering issues a new token and revokes the old one

use arena_server::{auth::TokenRegistry, routes::build_router};
use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ── Test helpers ──────────────────────────────────────────────────────────────

const EVENT_PASSWORD: &str = "test-secret";

fn app() -> axum::Router {
    let registry = TokenRegistry::new();
    build_router(EVENT_PASSWORD.to_owned(), registry, false)
}

fn app_with_registry() -> (axum::Router, std::sync::Arc<TokenRegistry>) {
    let registry = TokenRegistry::new();
    let app = build_router(EVENT_PASSWORD.to_owned(), registry.clone(), false);
    (app, registry)
}

async fn post_register(
    app: axum::Router,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/register")
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

// ── Test 1: correct password returns 200 and a non-empty token ───────────────
//
// RED → GREEN: `/register` handler exists and checks the event password.

#[tokio::test]
async fn register_with_correct_password_returns_token() {
    let (status, body) = post_register(
        app(),
        r#"{"password":"test-secret","team":"TeamAlpha"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().expect("response must contain a token field");
    assert!(!token.is_empty(), "token must be non-empty");
}

// ── Test 2: wrong password is rejected with 401 ───────────────────────────────
//
// Observable: any wrong password is a 401 Unauthorized, no token in body.

#[tokio::test]
async fn register_with_wrong_password_is_rejected() {
    let (status, body) = post_register(
        app(),
        r#"{"password":"wrong-password","team":"TeamBeta"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "wrong password must be 401");
    assert!(
        body["token"].is_null(),
        "rejected response must not contain a token, got: {body}"
    );
}

// ── Test 3: missing password field yields a client-error status ───────────────
//
// axum's `Json` extractor returns 422 for a body that is missing required fields.

#[tokio::test]
async fn register_with_missing_password_field_is_rejected() {
    let req = Request::builder()
        .method("POST")
        .uri("/register")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"team":"TeamGamma"}"#))
        .unwrap();

    let resp = app().oneshot(req).await.unwrap();

    assert!(
        resp.status().is_client_error(),
        "missing password field must return a 4xx status, got {}",
        resp.status()
    );
}

// ── Test 4: returned token resolves to the registered team identity ───────────
//
// The token in the HTTP response can be passed to `registry.resolve` and must
// return the team name supplied at registration.

#[tokio::test]
async fn registered_token_resolves_to_team_identity() {
    let (app, registry) = app_with_registry();

    let (status, body) = post_register(
        app,
        r#"{"password":"test-secret","team":"TeamDelta"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().unwrap();

    assert_eq!(
        registry.resolve(token).as_deref(),
        Some("TeamDelta"),
        "token must resolve to the registered team identity"
    );
}

// ── Test 5: two different teams receive distinct tokens ───────────────────────
//
// Tokens are UUID v4 — statistically guaranteed distinct, and the registry
// enforces a 1-to-1 token → team mapping.

#[tokio::test]
async fn two_teams_get_distinct_tokens() {
    let app = app();

    let (s1, b1) = post_register(
        app.clone(),
        r#"{"password":"test-secret","team":"TeamEcho"}"#,
    )
    .await;
    let (s2, b2) = post_register(
        app,
        r#"{"password":"test-secret","team":"TeamFoxtrot"}"#,
    )
    .await;

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    let t1 = b1["token"].as_str().unwrap();
    let t2 = b2["token"].as_str().unwrap();
    assert_ne!(t1, t2, "distinct teams must receive distinct tokens");
}

// ── Test 6: re-registering issues a new token and revokes the old one ─────────
//
// Chosen behaviour: a team that re-registers (e.g., after losing their token)
// always gets a *new* token; the old token is *revoked*.
//
// Rationale: this prevents stale credentials from accumulating and lets teams
// self-recover without administrator intervention.

#[tokio::test]
async fn re_registration_issues_new_token_and_revokes_old() {
    let (app, registry) = app_with_registry();

    let (s1, b1) = post_register(
        app.clone(),
        r#"{"password":"test-secret","team":"TeamGolf"}"#,
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    let old_token = b1["token"].as_str().unwrap().to_owned();

    let (s2, b2) = post_register(
        app,
        r#"{"password":"test-secret","team":"TeamGolf"}"#,
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    let new_token = b2["token"].as_str().unwrap().to_owned();

    assert_ne!(old_token, new_token, "re-registration must issue a new token");
    assert!(
        registry.resolve(&old_token).is_none(),
        "old token must be revoked after re-registration"
    );
    assert_eq!(
        registry.resolve(&new_token).as_deref(),
        Some("TeamGolf"),
        "new token must resolve to the team identity"
    );
}
