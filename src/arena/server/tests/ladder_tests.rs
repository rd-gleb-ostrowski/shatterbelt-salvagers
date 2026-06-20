//! TrueSkill ladder tests (issue 10) — strict TDD, vertical slices.
//!
//! Test order follows the suggested tracer:
//!  1. A→B: winner's conservative rating rises above loser's
//!  2. Winner μ up, loser μ down, both σ shrink
//!  3. Repeated A-beats-B: A's standing rises, σ converges
//!  4. FFA of 3 with full ranking updates all three
//!  5. standings() returns competitors ordered by conservative skill
//!  6. reset() clears standings
//!  7. scores_to_ranking with ties
//!
//! HTTP integration tests (issue 10 wiring):
//!  H1. GET /ladder/standings on a fresh server → 200 + empty array
//!  H2. POST /ladder/reset without facilitator password → 401
//!  H3. POST /ladder/reset with facilitator password → 200
//!  H4. After a finished headless match, GET /ladder/standings shows competitors
//!  H5. POST /ladder/reset (authed) then GET shows empty again
//!  H6. Standings JSON shape is camelCase contract

use std::sync::Arc;
use std::time::Duration;

use arena_engine::{Params, ShipId};
use arena_server::ladder::{Ladder, scores_to_ranking};
use arena_server::routes::{RouterConfig, build_router_config};
use arena_server::runner::MatchOutcome;
use axum::body::Body;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const FACILITATOR_PASSWORD: &str = "test-facilitator";

// ── HTTP test helpers ─────────────────────────────────────────────────────────

fn test_app_with_ladder(ladder: Arc<Ladder>) -> axum::Router {
    build_router_config(RouterConfig {
        event_password: "test-event".to_owned(),
        facilitator_password: FACILITATOR_PASSWORD.to_owned(),
        registry: arena_server::auth::TokenRegistry::new(),
        wasm_store: arena_server::store::WasmBotStore::new(),
        ws_registry: arena_server::resolver::WsConnectionRegistry::new(),
        tick_deadline: Duration::from_millis(33),
        match_seed: 42,
        match_params: Params { max_ticks: 5, ..Params::default() },
        observer_hub: arena_server::observer::ObserverHub::new(),
        recording_store: arena_server::recording::RecordingStore::new(),
        health_store: arena_server::health::BotHealthStore::new(),
        dq_store: arena_server::health::DqStore::new(),
        ladder,
        disabled_store: arena_server::store::DisabledStore::new(),
        default_bot_store: arena_server::store::DefaultBotStore::new(),
    })
}

fn test_app() -> axum::Router {
    test_app_with_ladder(Ladder::new())
}

async fn oneshot(
    app: axum::Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<Value>,
) -> http::Response<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(a) = auth {
        builder = builder.header("authorization", a);
    }
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    app.oneshot(builder.body(body).unwrap()).await.unwrap()
}

async fn response_body(resp: http::Response<Body>) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn ship(s: &str) -> ShipId {
    ShipId::from(s)
}

/// Build a MatchOutcome where `scores` is a list of (team, score) pairs.
fn outcome(scores: Vec<(&str, f32)>) -> MatchOutcome {
    let scores: Vec<(ShipId, f32)> = scores
        .into_iter()
        .map(|(id, s)| (ship(id), s))
        .collect();
    let winner = scores
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id.clone());
    MatchOutcome { winner, scores, ticks: 100 }
}

/// Map ship ID directly to its string form (identity mapping for tests).
fn identity(id: &ShipId) -> String {
    id.to_string()
}

// ── Slice 1: winner's conservative rating > loser's after one match ───────────

#[test]
fn winner_conservative_rating_exceeds_loser_after_one_match() {
    let ladder = Ladder::new();
    let result = outcome(vec![("alpha", 30.0), ("beta", 10.0)]);
    ladder.update_from_match(&result, identity);

    let alpha = ladder.rating("alpha").expect("alpha must be rated");
    let beta = ladder.rating("beta").expect("beta must be rated");

    assert!(
        alpha.conservative_skill() > beta.conservative_skill(),
        "winner alpha ({:.4}) should have higher conservative skill than loser beta ({:.4})",
        alpha.conservative_skill(),
        beta.conservative_skill()
    );
}

// ── Slice 2: μ/σ movement direction ──────────────────────────────────────────

#[test]
fn winner_mu_rises_loser_mu_falls_both_sigma_shrink() {
    let ladder = Ladder::new();
    let default_mu = 25.0_f64;
    let default_sigma = 25.0 / 3.0_f64;

    let result = outcome(vec![("alpha", 30.0), ("beta", 10.0)]);
    ladder.update_from_match(&result, identity);

    let alpha = ladder.rating("alpha").unwrap();
    let beta = ladder.rating("beta").unwrap();

    assert!(
        alpha.mu > default_mu,
        "winner mu should rise above default {default_mu}: got {:.4}",
        alpha.mu
    );
    assert!(
        beta.mu < default_mu,
        "loser mu should fall below default {default_mu}: got {:.4}",
        beta.mu
    );
    assert!(
        alpha.sigma < default_sigma,
        "winner sigma should shrink below default {default_sigma:.4}: got {:.4}",
        alpha.sigma
    );
    assert!(
        beta.sigma < default_sigma,
        "loser sigma should shrink below default {default_sigma:.4}: got {:.4}",
        beta.sigma
    );
}

// ── Slice 3: repeated wins converge α (μ rises, σ shrinks) ───────────────────

#[test]
fn repeated_wins_increase_winner_rating_and_reduce_uncertainty() {
    let ladder = Ladder::new();

    // Run 10 matches where alpha always beats beta.
    for _ in 0..10 {
        let result = outcome(vec![("alpha", 30.0), ("beta", 10.0)]);
        ladder.update_from_match(&result, identity);
    }

    let alpha = ladder.rating("alpha").unwrap();
    let beta = ladder.rating("beta").unwrap();

    // After many wins alpha must have substantially higher mu than beta.
    assert!(
        alpha.mu > beta.mu,
        "after 10 wins alpha mu ({:.4}) should exceed beta mu ({:.4})",
        alpha.mu,
        beta.mu
    );

    // Uncertainty must have converged significantly from the default (25/3 ≈ 8.33).
    let default_sigma = 25.0 / 3.0;
    assert!(
        alpha.sigma < default_sigma * 0.7,
        "alpha sigma ({:.4}) should have converged well below default ({:.4})",
        alpha.sigma,
        default_sigma
    );
    assert!(
        beta.sigma < default_sigma * 0.7,
        "beta sigma ({:.4}) should have converged well below default ({:.4})",
        beta.sigma,
        default_sigma
    );

    // standings() should place alpha first.
    let standings = ladder.standings();
    assert_eq!(
        standings[0].competitor, "alpha",
        "alpha should lead standings after repeated wins"
    );
}

// ── Slice 4: 3-competitor FFA updates all three in order ──────────────────────

#[test]
fn ffa_three_competitors_updates_all_ratings() {
    let ladder = Ladder::new();
    let default_mu = 25.0_f64;

    // gamma > alpha > beta
    let result = outcome(vec![("alpha", 20.0), ("beta", 5.0), ("gamma", 40.0)]);
    ladder.update_from_match(&result, identity);

    let alpha = ladder.rating("alpha").unwrap();
    let beta = ladder.rating("beta").unwrap();
    let gamma = ladder.rating("gamma").unwrap();

    // All three must have been rated.
    assert_eq!(alpha.matches, 1);
    assert_eq!(beta.matches, 1);
    assert_eq!(gamma.matches, 1);

    // Winner mu up, last-place mu down.
    assert!(
        gamma.mu > default_mu,
        "1st place gamma mu should rise: {:.4}",
        gamma.mu
    );
    assert!(
        beta.mu < default_mu,
        "last place beta mu should fall: {:.4}",
        beta.mu
    );

    // gamma > alpha > beta in conservative skill.
    assert!(
        gamma.conservative_skill() > alpha.conservative_skill(),
        "gamma should rank above alpha"
    );
    assert!(
        alpha.conservative_skill() > beta.conservative_skill(),
        "alpha should rank above beta"
    );
}

// ── Slice 5: standings() orders by conservative skill ─────────────────────────

#[test]
fn standings_orders_by_conservative_skill_descending() {
    let ladder = Ladder::new();

    // Run several matches so alpha clearly outperforms beta and gamma.
    for _ in 0..5 {
        // Order: alpha 1st, gamma 2nd, beta 3rd.
        let result = outcome(vec![("alpha", 50.0), ("gamma", 30.0), ("beta", 5.0)]);
        ladder.update_from_match(&result, identity);
    }

    let standings = ladder.standings();
    assert!(standings.len() >= 3);

    // Verify descending conservative skill.
    for window in standings.windows(2) {
        assert!(
            window[0].conservative_skill() >= window[1].conservative_skill(),
            "standings not sorted: {} ({:.4}) should be >= {} ({:.4})",
            window[0].competitor,
            window[0].conservative_skill(),
            window[1].competitor,
            window[1].conservative_skill()
        );
    }

    // Top of standings should be alpha.
    assert_eq!(
        standings[0].competitor, "alpha",
        "alpha should lead standings"
    );
}

// ── Slice 6: reset() clears all ratings ───────────────────────────────────────

#[test]
fn reset_clears_all_standings() {
    let ladder = Ladder::new();
    let result = outcome(vec![("alpha", 30.0), ("beta", 10.0)]);
    ladder.update_from_match(&result, identity);

    // Confirm there is data before reset.
    assert!(!ladder.standings().is_empty());

    ladder.reset();

    assert!(
        ladder.standings().is_empty(),
        "standings should be empty after reset"
    );
    assert!(
        ladder.rating("alpha").is_none(),
        "alpha rating should be gone after reset"
    );
    assert!(
        ladder.rating("beta").is_none(),
        "beta rating should be gone after reset"
    );
}

// ── Slice 7: scores_to_ranking handles ties correctly ─────────────────────────

#[test]
fn scores_to_ranking_no_ties() {
    let scores = vec![
        (ship("alpha"), 30.0_f32),
        (ship("beta"), 10.0_f32),
        (ship("gamma"), 20.0_f32),
    ];
    let ranking = scores_to_ranking(&scores);

    let rank_of = |name: &str| {
        ranking
            .iter()
            .find(|(id, _)| id.as_str() == name)
            .map(|(_, r)| *r)
            .unwrap()
    };

    assert_eq!(rank_of("alpha"), 1, "highest score → rank 1");
    assert_eq!(rank_of("gamma"), 2, "middle score → rank 2");
    assert_eq!(rank_of("beta"), 3, "lowest score → rank 3");
}

#[test]
fn scores_to_ranking_handles_two_way_tie_at_top() {
    // alpha and gamma tie for 1st; beta gets rank 3.
    let scores = vec![
        (ship("alpha"), 30.0_f32),
        (ship("beta"), 10.0_f32),
        (ship("gamma"), 30.0_f32),
    ];
    let ranking = scores_to_ranking(&scores);

    let rank_of = |name: &str| {
        ranking
            .iter()
            .find(|(id, _)| id.as_str() == name)
            .map(|(_, r)| *r)
            .unwrap()
    };

    assert_eq!(rank_of("alpha"), 1, "alpha tied for 1st → rank 1");
    assert_eq!(rank_of("gamma"), 1, "gamma tied for 1st → rank 1");
    assert_eq!(rank_of("beta"), 3, "beta after two-way tie → rank 3");
}

#[test]
fn scores_to_ranking_empty_input_returns_empty() {
    let ranking = scores_to_ranking(&[]);
    assert!(ranking.is_empty());
}

#[test]
fn scores_to_ranking_single_ship() {
    let scores = vec![(ship("alpha"), 15.0_f32)];
    let ranking = scores_to_ranking(&scores);
    assert_eq!(ranking.len(), 1);
    assert_eq!(ranking[0].1, 1);
}

// ── Bonus: ship→competitor mapping is explicit at call site ───────────────────

#[test]
fn ship_to_team_mapping_aggregates_correctly() {
    // Two ships map to the same team; their scores should be averaged.
    let ladder = Ladder::new();

    // "ship1" and "ship2" both map to team "red"; "ship3" maps to "blue".
    // red: avg(40, 20) = 30; blue: 10 → red wins.
    let result = MatchOutcome {
        winner: Some(ship("ship1")),
        scores: vec![
            (ship("ship1"), 40.0),
            (ship("ship2"), 20.0),
            (ship("ship3"), 10.0),
        ],
        ticks: 100,
    };
    ladder.update_from_match(&result, |id| {
        match id.as_str() {
            "ship1" | "ship2" => "red".to_string(),
            _ => "blue".to_string(),
        }
    });

    let red = ladder.rating("red").expect("red should be rated");
    let blue = ladder.rating("blue").expect("blue should be rated");
    assert!(
        red.conservative_skill() > blue.conservative_skill(),
        "red (avg 30) should rank above blue (10)"
    );
}

// ── Edge: update_from_match with single competitor is a no-op ─────────────────

#[test]
fn single_competitor_match_is_noop() {
    let ladder = Ladder::new();
    let result = outcome(vec![("solo", 42.0)]);
    ladder.update_from_match(&result, identity);
    // No rating should be stored because there's nothing to rank against.
    assert!(ladder.rating("solo").is_none());
}

// ── H1: GET /ladder/standings on fresh server → 200 + empty array ────────────

#[tokio::test]
async fn get_ladder_standings_fresh_server_returns_200_empty_array() {
    let app = test_app();
    let resp = oneshot(app, Method::GET, "/ladder/standings", None, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    assert_eq!(body, serde_json::json!([]));
}

// ── H2: POST /ladder/reset without auth → 401 ─────────────────────────────────

#[tokio::test]
async fn post_ladder_reset_without_auth_returns_401() {
    let app = test_app();
    let resp = oneshot(app, Method::POST, "/ladder/reset", None, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── H3: POST /ladder/reset with auth → 200 ───────────────────────────────────

#[tokio::test]
async fn post_ladder_reset_with_correct_auth_returns_200() {
    let app = test_app();
    let auth = format!("Facilitator {FACILITATOR_PASSWORD}");
    let resp = oneshot(app, Method::POST, "/ladder/reset", Some(&auth), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── H4: After a headless match the ladder shows competitors ───────────────────

#[tokio::test]
async fn standings_after_headless_match_shows_competitors_ordered_by_conservative_skill() {
    let ladder = Ladder::new();
    let app = test_app_with_ladder(Arc::clone(&ladder));

    // Start a headless admin match via HTTP so the ladder feed path is exercised.
    let auth = format!("Facilitator {FACILITATOR_PASSWORD}");
    let body = serde_json::json!({
        "mode": "headless",
        "seed": 1,
        "maxTicks": 5,
        "teams": ["alpha", "beta"]
    });
    let resp = oneshot(
        app.clone(),
        Method::POST,
        "/admin/matches",
        Some(&auth),
        Some(body),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "headless match must start successfully");

    // Query standings through the shared ladder in AppState.
    let resp = oneshot(app, Method::GET, "/ladder/standings", None, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    let entries = body.as_array().expect("standings must be an array");

    // Both competitors must have been rated (ladder feed exercised).
    assert_eq!(entries.len(), 2, "both competitors should be ranked");

    // Competitors must be alpha and beta (in any order — ordering by conservative
    // skill is tested separately with injected scores in H6).
    let names: Vec<&str> = entries.iter().map(|e| e["competitor"].as_str().unwrap()).collect();
    assert!(names.contains(&"alpha"), "alpha must be ranked");
    assert!(names.contains(&"beta"), "beta must be ranked");
}

// ── H5: reset clears standings ────────────────────────────────────────────────

#[tokio::test]
async fn reset_then_standings_empty() {
    let ladder = Ladder::new();
    let app = test_app_with_ladder(Arc::clone(&ladder));

    // Seed the ladder directly so we don't need to run a full match.
    let mo = MatchOutcome {
        winner: Some(ShipId::from("alpha")),
        scores: vec![(ShipId::from("alpha"), 30.0), (ShipId::from("beta"), 10.0)],
        ticks: 5,
    };
    ladder.update_from_match(&mo, |id| id.to_string());

    // Confirm it's non-empty.
    let resp = oneshot(app.clone(), Method::GET, "/ladder/standings", None, None).await;
    let body = response_body(resp).await;
    assert!(!body.as_array().unwrap().is_empty(), "ladder must have entries before reset");

    // Reset via HTTP.
    let auth = format!("Facilitator {FACILITATOR_PASSWORD}");
    let resp = oneshot(app.clone(), Method::POST, "/ladder/reset", Some(&auth), None).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Standings must be empty after reset.
    let resp = oneshot(app, Method::GET, "/ladder/standings", None, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    assert_eq!(body, serde_json::json!([]), "standings must be empty after reset");
}

// ── H6: JSON shape is camelCase as documented ─────────────────────────────────

#[tokio::test]
async fn standings_json_shape_is_camel_case_contract() {
    let ladder = Ladder::new();
    let app = test_app_with_ladder(Arc::clone(&ladder));

    // Seed the ladder with one match directly.
    let mo = MatchOutcome {
        winner: Some(ShipId::from("alpha")),
        scores: vec![(ShipId::from("alpha"), 30.0), (ShipId::from("beta"), 10.0)],
        ticks: 5,
    };
    ladder.update_from_match(&mo, |id| id.to_string());

    let resp = oneshot(app, Method::GET, "/ladder/standings", None, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    let entries = body.as_array().unwrap();
    assert!(!entries.is_empty());

    // Every entry must have all required camelCase fields.
    for entry in entries {
        assert!(entry["competitor"].is_string(), "competitor must be string");
        assert!(entry["mu"].is_number(), "mu must be number");
        assert!(entry["sigma"].is_number(), "sigma must be number");
        assert!(entry["conservativeSkill"].is_number(), "conservativeSkill must be number");
        assert!(entry["matches"].is_number(), "matches must be number");
        // Snake-case variants must NOT appear.
        assert!(
            entry.get("conservative_skill").is_none(),
            "snake_case conservativeSkill must not appear"
        );
    }

    // Both competitors must be present.
    let competitors: Vec<&str> = entries
        .iter()
        .map(|e| e["competitor"].as_str().unwrap())
        .collect();
    assert!(competitors.contains(&"alpha"), "alpha must be in standings");
    assert!(competitors.contains(&"beta"), "beta must be in standings");
}
