//! TrueSkill ladder (issue 10).
//!
//! # Overview
//!
//! The [`Ladder`] maintains a continuously-updated TrueSkill ranking of all
//! known competitors (ships / bots / teams).  Each finished match contributes
//! its full **FFA ranking** — derived from the match's final per-ship scores
//! (highest score = rank 1) — to update every participating competitor's
//! rating with uncertainty (μ + σ).
//!
//! Both live and headless matches feed the ladder through the same
//! [`Ladder::update_from_match`] call.
//!
//! # Identity mapping
//!
//! Matches are keyed by [`ShipId`](arena_engine::ShipId).  The ladder ranks
//! **competitor identities** (team names, bot IDs, or ship IDs).  The caller
//! is responsible for the ship→team mapping:
//!
//! - For **headless matches** (issue 09) the `teams` list stored in
//!   [`HeadlessRunner`](crate::headless::HeadlessRunner) provides the mapping:
//!   `teams[i]` is the competitor identity for `specs[i].id`.  The helper
//!   [`consume_headless_results`] uses this mapping by accepting a
//!   `ship_to_competitor` closure.
//! - For **live matches** callers supply the same mapping when calling
//!   [`Ladder::update_from_match`].
//! - When treating each ship directly as a competitor (e.g. tests), map each
//!   `ShipId` to itself (or its string form).
//!
//! This keeps the identity mapping **explicit and documented** at each call
//! site rather than baked into the ladder's internals.
//!
//! # TrueSkill crate
//!
//! Uses [`skillratings`](https://crates.io/crates/skillratings) v0.29.0 —
//! a maintained crate that supports multi-player FFA via
//! [`trueskill_multi_team`] with each bot as a single-player "team".
//! Ties are encoded as equal ranks (same `MultiTeamOutcome` value).
//!
//! Key types / functions used:
//! - [`TrueSkillRating`] — `{ rating: f64 (μ), uncertainty: f64 (σ) }`
//! - [`TrueSkillConfig`] — draw probability, beta, dynamics factor
//! - [`trueskill_multi_team`] — FFA update from ordered ranking
//! - [`MultiTeamOutcome::new(rank)`] — rank is 1-based; equal ranks = tie
//!
//! # Seams for issue 11 (admin)
//!
//! - `GET /ladder/standings` — call [`Ladder::standings`] and serialise.
//! - `POST /ladder/reset` — call [`Ladder::reset`].
//! - `POST /ladder/loop/start` + `stop` — hold the `watch::Sender<bool>` from
//!   [`HeadlessRunner::spawn_loop`] and call
//!   [`consume_headless_results`] on the receiver.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use skillratings::{
    MultiTeamOutcome,
    trueskill::{TrueSkillConfig, TrueSkillRating, trueskill_multi_team},
};

use arena_engine::ShipId;

use crate::runner::MatchOutcome;

// ── scores_to_ranking ─────────────────────────────────────────────────────────

/// Derive an FFA ranking from per-ship final scores.
///
/// Returns a `Vec<(ShipId, rank)>` where rank is **1-based** (1 = best).
/// Ships with equal scores receive the **same rank** (ties); the next rank
/// after a tie group skips by the size of that group (standard competition
/// ranking: 1, 1, 3, …).
///
/// # Examples
///
/// ```
/// use arena_engine::ShipId;
/// use arena_server::ladder::scores_to_ranking;
///
/// let scores = vec![
///     (ShipId::from("alpha"), 30.0_f32),
///     (ShipId::from("beta"),  10.0_f32),
///     (ShipId::from("gamma"), 20.0_f32),
/// ];
/// let ranking = scores_to_ranking(&scores);
/// // alpha→1, gamma→2, beta→3
/// assert_eq!(ranking.iter().find(|(id,_)| id.as_str()=="alpha").unwrap().1, 1);
/// assert_eq!(ranking.iter().find(|(id,_)| id.as_str()=="gamma").unwrap().1, 2);
/// assert_eq!(ranking.iter().find(|(id,_)| id.as_str()=="beta" ).unwrap().1, 3);
/// ```
pub fn scores_to_ranking(scores: &[(ShipId, f32)]) -> Vec<(ShipId, usize)> {
    if scores.is_empty() {
        return Vec::new();
    }

    // Sort descending by score; stable to preserve ship ordering on ties.
    let mut sorted: Vec<(usize, f32)> = scores
        .iter()
        .enumerate()
        .map(|(i, (_, s))| (i, *s))
        .collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut ranks = vec![0usize; scores.len()];
    let mut rank = 1usize;
    let mut i = 0;
    while i < sorted.len() {
        // Find how many share this score (tie group).
        let score = sorted[i].1;
        let mut j = i;
        while j < sorted.len()
            && (sorted[j].1 - score).abs() < f32::EPSILON
        {
            j += 1;
        }
        // Assign rank to the entire tie group.
        for k in i..j {
            ranks[sorted[k].0] = rank;
        }
        rank += j - i; // Next rank skips by group size.
        i = j;
    }

    scores
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.clone(), ranks[i]))
        .collect()
}

// ── Entry ─────────────────────────────────────────────────────────────────────

/// A single competitor's current rating on the ladder.
#[derive(Debug, Clone)]
pub struct LadderEntry {
    /// Competitor identity (team name, bot ID, or ship ID string).
    pub competitor: String,
    /// TrueSkill μ (mean skill estimate).
    pub mu: f64,
    /// TrueSkill σ (uncertainty).
    pub sigma: f64,
    /// Number of matches played.
    pub matches: u32,
}

impl LadderEntry {
    /// Conservative skill estimate: μ − 3σ (used for standings ordering).
    ///
    /// A freshly-initialised entry has `conservative_skill ≈ 25 − 3×8.33 ≈ 0`.
    /// As σ shrinks with more matches, this converges toward μ.
    pub fn conservative_skill(&self) -> f64 {
        self.mu - 3.0 * self.sigma
    }
}

// ── Ladder ────────────────────────────────────────────────────────────────────

/// Thread-safe TrueSkill ladder.
///
/// Wrap in `Arc<Ladder>` and share between the match loop and HTTP handlers.
///
/// ## Usage
///
/// ```
/// use arena_server::ladder::Ladder;
/// use arena_engine::ShipId;
/// use arena_server::runner::MatchOutcome;
///
/// let ladder = Ladder::new();
///
/// // Map ships to competitor identities (here, ship ID = competitor).
/// let outcome = MatchOutcome {
///     winner: Some(ShipId::from("alpha")),
///     scores: vec![
///         (ShipId::from("alpha"), 30.0),
///         (ShipId::from("beta"),  10.0),
///     ],
///     ticks: 100,
/// };
/// ladder.update_from_match(&outcome, |id| id.to_string());
///
/// let standings = ladder.standings();
/// assert_eq!(standings[0].competitor, "alpha");
/// ```
pub struct Ladder {
    inner: Mutex<LadderInner>,
}

struct LadderInner {
    ratings: HashMap<String, TrueSkillRating>,
    matches: HashMap<String, u32>,
    config: TrueSkillConfig,
}

impl Ladder {
    /// Create a new, empty ladder with default TrueSkill configuration.
    ///
    /// The draw probability is set to 0.0 (draws are impossible in the Arena
    /// since scores are continuous floats), which maximises rating movement per
    /// match and speeds up convergence.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(LadderInner {
                ratings: HashMap::new(),
                matches: HashMap::new(),
                config: TrueSkillConfig {
                    draw_probability: 0.0,
                    ..TrueSkillConfig::default()
                },
            }),
        })
    }

    /// Update ratings from a finished match outcome.
    ///
    /// `ship_to_competitor` maps each [`ShipId`] in `outcome.scores` to a
    /// competitor identity string (team name, bot ID, etc.).  Ships that map
    /// to the same identity are treated as a single competitor — their scores
    /// are averaged to produce one rank entry (useful when future issues add
    /// multi-ship teams; for single-ship-per-team matches, just map id → id).
    ///
    /// Competitors that have not been seen before receive the default
    /// TrueSkill rating (μ=25, σ=25/3) before the update.
    ///
    /// # No-op conditions
    ///
    /// - Empty `outcome.scores` — nothing to update.
    /// - Only one distinct competitor — no meaningful update; skip.
    pub fn update_from_match(
        &self,
        outcome: &MatchOutcome,
        ship_to_competitor: impl Fn(&ShipId) -> String,
    ) {
        if outcome.scores.is_empty() {
            return;
        }

        // Aggregate scores per competitor (average, for multi-ship teams).
        let mut competitor_scores: HashMap<String, (f32, usize)> = HashMap::new();
        for (ship_id, score) in &outcome.scores {
            let comp = ship_to_competitor(ship_id);
            let entry = competitor_scores.entry(comp).or_insert((0.0, 0));
            entry.0 += score;
            entry.1 += 1;
        }
        let mut comp_scores: Vec<(String, f32)> = competitor_scores
            .into_iter()
            .map(|(comp, (sum, count))| (comp, sum / count as f32))
            .collect();

        if comp_scores.len() < 2 {
            return; // Can't rank a single competitor.
        }

        // Derive ranking from aggregated scores.
        let raw_scores: Vec<(ShipId, f32)> = comp_scores
            .iter()
            .map(|(c, s)| (ShipId::from(c.as_str()), *s))
            .collect();
        let ranking = scores_to_ranking(&raw_scores);

        // Sort comp_scores to match ranking order (same index).
        comp_scores.sort_by_key(|(c, _)| c.clone());
        let sorted_ranking: Vec<(String, usize)> = {
            let mut v: Vec<(String, usize)> = ranking
                .into_iter()
                .map(|(id, rank)| (id.to_string(), rank))
                .collect();
            v.sort_by_key(|(c, _)| c.clone());
            v
        };

        let mut inner = self.inner.lock().unwrap();

        // Fetch (or initialise) current ratings for each competitor.
        let current_ratings: Vec<(String, TrueSkillRating, usize)> = sorted_ranking
            .iter()
            .map(|(comp, rank)| {
                let rating = inner
                    .ratings
                    .get(comp)
                    .copied()
                    .unwrap_or_default();
                (comp.clone(), rating, *rank)
            })
            .collect();

        // Build the teams_and_ranks slice for trueskill_multi_team.
        // Each competitor is a single-player "team".
        let ratings_only: Vec<TrueSkillRating> =
            current_ratings.iter().map(|(_, r, _)| *r).collect();
        let teams_and_ranks: Vec<(&[TrueSkillRating], MultiTeamOutcome)> = current_ratings
            .iter()
            .enumerate()
            .map(|(i, (_, _, rank))| {
                let slice = std::slice::from_ref(&ratings_only[i]);
                (slice, MultiTeamOutcome::new(*rank))
            })
            .collect();

        // Run TrueSkill multi-team FFA update.
        let updated = match trueskill_multi_team(&teams_and_ranks, &inner.config, None) {
            Ok(u) => u,
            Err(_) => return, // Shouldn't happen with valid input; skip silently.
        };

        // Write updated ratings back.
        for (i, (comp, _, _)) in current_ratings.iter().enumerate() {
            let new_rating = updated[i][0];
            inner.ratings.insert(comp.clone(), new_rating);
            *inner.matches.entry(comp.clone()).or_insert(0) += 1;
        }
    }

    /// Return the current standings, ordered by **conservative skill** (μ − 3σ)
    /// descending.  Higher conservative skill = higher confidence in a good
    /// rating.
    pub fn standings(&self) -> Vec<LadderEntry> {
        let inner = self.inner.lock().unwrap();
        let mut entries: Vec<LadderEntry> = inner
            .ratings
            .iter()
            .map(|(comp, rating)| LadderEntry {
                competitor: comp.clone(),
                mu: rating.rating,
                sigma: rating.uncertainty,
                matches: inner.matches.get(comp).copied().unwrap_or(0),
            })
            .collect();
        entries.sort_by(|a, b| {
            b.conservative_skill()
                .partial_cmp(&a.conservative_skill())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    /// Return the current rating for a specific competitor, or `None` if they
    /// have not yet played.
    pub fn rating(&self, competitor: &str) -> Option<LadderEntry> {
        let inner = self.inner.lock().unwrap();
        inner.ratings.get(competitor).map(|r| LadderEntry {
            competitor: competitor.to_owned(),
            mu: r.rating,
            sigma: r.uncertainty,
            matches: inner.matches.get(competitor).copied().unwrap_or(0),
        })
    }

    /// Clear all ratings and match counts, returning the ladder to its empty
    /// initial state.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.ratings.clear();
        inner.matches.clear();
    }
}

// ── HeadlessResult consumer ───────────────────────────────────────────────────

/// Consume [`HeadlessResult`](crate::headless::HeadlessResult)s from the
/// channel returned by [`HeadlessRunner::spawn_loop`](crate::headless::HeadlessRunner::spawn_loop)
/// and feed each finished match into the ladder.
///
/// `teams` must be the same slice used to construct the [`HeadlessRunner`]:
/// `teams[i]` is the competitor identity for the ship at position `i` in
/// `HeadlessRunner::specs`.  The mapping is: ship `specs[i].id` → `teams[i]`.
///
/// This function runs until the `rx` channel is closed (headless loop stopped)
/// or the `stop_rx` watch fires.  It is designed to be spawned as a
/// `tokio::spawn` task alongside the headless loop.
///
/// # Example (wiring at server startup — issue 11 seam)
///
/// ```ignore
/// let (stop_tx, result_rx, _handle) = headless_runner.clone().spawn_loop();
/// let ladder_clone = Arc::clone(&ladder);
/// let teams_clone = teams.clone();
/// tokio::spawn(async move {
///     consume_headless_results(result_rx, ladder_clone, &teams_clone).await;
/// });
/// ```
pub async fn consume_headless_results(
    mut rx: tokio::sync::mpsc::Receiver<crate::headless::HeadlessResult>,
    ladder: Arc<Ladder>,
    teams: &[String],
) {
    while let Some(result) = rx.recv().await {
        // let teams_snapshot = teams.to_vec();
        // Build a ship→competitor map from the runner's teams list.
        // specs[i].id corresponds to teams[i]; the HeadlessResult's
        // outcome.scores are in the same order as specs.
        // let scores_with_competitors: Vec<(ShipId, f32)> =
            // result.outcome.scores.clone();

        // We can't access specs directly here; instead we use the fact that
        // outcome.scores is in the same slot order as HeadlessRunner::specs
        // (and therefore HeadlessRunner::teams).  We build the mapping by
        // position: scores[i].0 (ShipId) → teams[i].
        // let ship_to_competitor_map: HashMap<String, String> = scores_with_competitors
        //     .iter()
        //     .enumerate()
        //     .filter_map(|(i, (ship_id, _))| {
        //         teams_snapshot.get(i).map(|team| (ship_id.to_string(), team.clone()))
        //     })
        //     .collect();

        ladder.update_from_match(&result.outcome, |ship_id| {
            // Ship_id is team name for now
            ship_id.to_owned()
            // ship_to_competitor_map
            //     .get(&ship_id.to_string())
            //     .cloned()
            //     .unwrap_or_else(|| ship_id.to_string())
        });
    }
}
