use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

use arena_engine::{Engine, Params, ShipClass, ShipSpec, Vec2};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    auth::TokenRegistry,
    headless::HeadlessRunner,
    health::{BotHealthStore, DqStore},
    observer::ObserverHub,
    recording::{Recording, RecordingMeta, RecordingStore},
    resolver::{ConnectionResolver, Slot, WsConnectionRegistry},
    routes::AppState,
    runner::{BotDriver, MatchOutcome, MatchRunner, TickPacer},
    store::WasmBotStore,
    ws::{obs_to_tick_json, MatchEndMsg, MatchResultsJson, MatchStartMsg},
};

pub struct MatchControlHandle {
    inner: Arc<ControlInner>,
}

struct ControlInner {
    paused: Mutex<bool>,
    pause_cv: Condvar,
    aborted: AtomicBool,
    tps: AtomicU32,
    tick_count: AtomicU32,
}

impl MatchControlHandle {
    pub fn new(initial_tps: u32) -> Self {
        Self {
            inner: Arc::new(ControlInner {
                paused: Mutex::new(false),
                pause_cv: Condvar::new(),
                aborted: AtomicBool::new(false),
                tps: AtomicU32::new(initial_tps),
                tick_count: AtomicU32::new(0),
            }),
        }
    }

    pub fn pause(&self) {
        *self.inner.paused.lock().unwrap() = true;
    }

    pub fn resume(&self) {
        *self.inner.paused.lock().unwrap() = false;
        self.inner.pause_cv.notify_all();
    }

    pub fn abort(&self) {
        self.inner.aborted.store(true, Ordering::SeqCst);
        self.inner.pause_cv.notify_all();
    }

    pub fn set_tps(&self, tps: u32) {
        self.inner.tps.store(tps, Ordering::SeqCst);
    }

    pub fn tps(&self) -> u32 {
        self.inner.tps.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        *self.inner.paused.lock().unwrap()
    }

    pub fn is_aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::SeqCst)
    }

    pub fn tick_count(&self) -> u32 {
        self.inner.tick_count.load(Ordering::SeqCst)
    }

    pub(crate) fn wait_while_paused(&self) {
        let mut paused = self.inner.paused.lock().unwrap();
        while *paused && !self.is_aborted() {
            paused = self.inner.pause_cv.wait(paused).unwrap();
        }
    }

    pub(crate) fn increment_tick(&self) {
        self.inner.tick_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl Clone for MatchControlHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub struct ControlledPacer {
    handle: MatchControlHandle,
    tick_start: Instant,
}

impl ControlledPacer {
    pub fn new(handle: MatchControlHandle) -> Self {
        Self {
            handle,
            tick_start: Instant::now(),
        }
    }
}

impl TickPacer for ControlledPacer {
    fn wait_for_next_tick(&mut self) {
        self.handle.wait_while_paused();
        if self.handle.is_aborted() {
            return;
        }
        let tps = self.handle.tps();
        if tps == 0 {
            self.tick_start = Instant::now();
            return;
        }
        let tick_duration = Duration::from_secs_f64(1.0 / f64::from(tps));
        let elapsed = self.tick_start.elapsed();
        if elapsed < tick_duration {
            std::thread::sleep(tick_duration - elapsed);
        }
        self.tick_start = Instant::now();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_live_controlled(
    seed: u64,
    params: Params,
    specs: Vec<ShipSpec>,
    drivers: Vec<Box<dyn BotDriver>>,
    pacer: Box<dyn TickPacer>,
    handle: MatchControlHandle,
    observer_hub: ObserverHub,
    recording_store: Arc<RecordingStore>,
) -> (String, MatchOutcome) {
    let match_id = Uuid::new_v4().to_string();
    run_live_controlled_with_id(
        match_id,
        seed,
        params,
        specs,
        drivers,
        pacer,
        handle,
        observer_hub,
        recording_store,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_live_controlled_with_id(
    match_id: String,
    seed: u64,
    params: Params,
    specs: Vec<ShipSpec>,
    drivers: Vec<Box<dyn BotDriver>>,
    pacer: Box<dyn TickPacer>,
    handle: MatchControlHandle,
    observer_hub: ObserverHub,
    recording_store: Arc<RecordingStore>,
) -> (String, MatchOutcome) {
    let ship_ids: Vec<_> = specs.iter().map(|spec| spec.id.clone()).collect();
    let mut runner = MatchRunner::new(seed, params.clone(), specs.clone(), drivers, pacer);

    loop {
        if runner.engine().is_match_over() || handle.is_aborted() {
            break;
        }
        handle.wait_while_paused();
        if handle.is_aborted() {
            break;
        }
        let events = runner.step_once();
        handle.increment_tick();
        let gv = runner.engine().god_view();
        observer_hub.publish_god_view(&gv, &events);
        runner.wait_for_next_tick();
    }

    let scores = ship_ids
        .iter()
        .map(|id| (id.clone(), runner.engine().score(id).unwrap_or(0.0)))
        .collect();
    let outcome = MatchOutcome {
        winner: runner.engine().winner(),
        scores,
        ticks: runner.engine().tick(),
    };
    let meta = RecordingMeta {
        match_id: match_id.clone(),
        seed,
        tick_count: outcome.ticks,
        winner: outcome.winner.clone(),
        scores: outcome.scores.clone(),
    };
    recording_store.record(Recording {
        match_id: match_id.clone(),
        seed,
        params,
        specs,
        intent_log: runner.engine().intent_log().to_vec(),
        meta,
    });

    (match_id, outcome)
}

#[allow(clippy::too_many_arguments)]
pub async fn spawn_live_match(
    seed: u64,
    params: Params,
    mut specs: Vec<ShipSpec>,
    teams: Vec<String>,
    wasm_store: Arc<WasmBotStore>,
    ws_registry: Arc<WsConnectionRegistry>,
    fuel_per_tick: u64,
    observer_hub: ObserverHub,
    recording_store: Arc<RecordingStore>,
    handle: MatchControlHandle,
    pacer: Box<dyn TickPacer + Send + 'static>,
) -> tokio::task::JoinHandle<(String, MatchOutcome)> {
    let drivers = resolve_drivers(
        seed,
        &params,
        &specs,
        &teams,
        wasm_store,
        Arc::clone(&ws_registry),
        fuel_per_tick,
        DqStore::new(),
        BotHealthStore::new(),
        Duration::from_millis(100),
        None,
    );
    // Send matchStart envelope to every connected WS bot in this roster.
    let match_start_json = serde_json::to_string(&MatchStartMsg {
        type_: "matchStart",
    })
    .unwrap_or_default();
    for team in &teams {
        if let Some(session) = ws_registry.get(team) {
            session.try_send_envelope(match_start_json.clone());
        }
    }
    let ws_reg_for_end = Arc::clone(&ws_registry);
    let teams_for_end = teams.clone();
    for (i, team) in teams.iter().enumerate() {
        specs[i].id = team.clone();
    }
    tokio::task::spawn_blocking(move || {
        let result = run_live_controlled(
            seed,
            params,
            specs,
            drivers,
            pacer,
            handle,
            observer_hub,
            recording_store,
        );
        // Send matchEnd envelope to every connected WS bot.
        let match_end = MatchEndMsg {
            type_: "matchEnd",
            results: MatchResultsJson {
                winner: result.1.winner.clone(),
                scores: result.1.scores.iter().cloned().collect(),
                ticks: result.1.ticks,
            },
        };
        let match_end_json = serde_json::to_string(&match_end).unwrap_or_default();
        for team in &teams_for_end {
            if let Some(session) = ws_reg_for_end.get(team) {
                session.try_send_envelope(match_end_json.clone());
            }
        }
        result
    })
}

pub struct MatchRegistry {
    matches: Mutex<HashMap<String, MatchControlHandle>>,
}

impl MatchRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            matches: Mutex::new(HashMap::new()),
        })
    }

    pub fn register(&self, match_id: String, handle: MatchControlHandle) {
        self.matches.lock().unwrap().insert(match_id, handle);
    }

    pub fn get(&self, match_id: &str) -> Option<MatchControlHandle> {
        self.matches.lock().unwrap().get(match_id).cloned()
    }

    pub fn remove(&self, match_id: &str) {
        self.matches.lock().unwrap().remove(match_id);
    }
}

pub struct ExhibitionConfig {
    pub seed: u64,
    pub params: Params,
    /// Explicit team override.  Empty = use `live_teams(ws_registry)` at the
    /// start of each match iteration so late-connecting bots are included.
    pub teams: Vec<String>,
    pub wasm_store: Arc<WasmBotStore>,
    pub ws_registry: Arc<WsConnectionRegistry>,
    pub registry: Arc<TokenRegistry>,
    pub fuel_per_tick: u64,
    pub observer_hub: ObserverHub,
    pub recording_store: Arc<RecordingStore>,
    pub tps: u32,
    pub max_matches: u32,
    pub pacer_factory: Arc<dyn Fn(MatchControlHandle) -> Box<dyn TickPacer + Send> + Send + Sync>,
}

pub struct ExhibitionSupervisor {
    stop_tx: Arc<tokio::sync::Mutex<Option<watch::Sender<bool>>>>,
    current_handle: Arc<tokio::sync::Mutex<Option<MatchControlHandle>>>,
    match_count: Arc<AtomicU32>,
    task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ExhibitionSupervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stop_tx: Arc::new(tokio::sync::Mutex::new(None)),
            current_handle: Arc::new(tokio::sync::Mutex::new(None)),
            match_count: Arc::new(AtomicU32::new(0)),
            task: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    pub fn start(&self, config: ExhibitionConfig) {
        let (stop_tx, stop_rx) = watch::channel(false);
        if let Ok(mut slot) = self.stop_tx.try_lock() {
            *slot = Some(stop_tx);
        }

        let current_handle = Arc::clone(&self.current_handle);
        let match_count = Arc::clone(&self.match_count);
        let task = tokio::spawn(async move {
            exhibition_loop(config, stop_rx, current_handle, match_count).await;
        });

        if let Ok(mut slot) = self.task.try_lock() {
            *slot = Some(task);
        }
    }

    pub async fn stop(&self) {
        if let Some(tx) = self.stop_tx.lock().await.as_ref() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.current_handle.lock().await.as_ref() {
            handle.abort();
        }
    }

    pub async fn current_handle(&self) -> Option<MatchControlHandle> {
        self.current_handle.lock().await.clone()
    }

    pub fn match_count(&self) -> u32 {
        self.match_count.load(Ordering::SeqCst)
    }

    pub async fn join(&self) {
        let task = self.task.lock().await.take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

async fn exhibition_loop(
    config: ExhibitionConfig,
    stop_rx: watch::Receiver<bool>,
    current_handle: Arc<tokio::sync::Mutex<Option<MatchControlHandle>>>,
    match_count: Arc<AtomicU32>,
) {
    let stop_rx = stop_rx;
    loop {
        if *stop_rx.borrow() {
            break;
        }
        if config.max_matches > 0 && match_count.load(Ordering::SeqCst) >= config.max_matches {
            break;
        }

        // Recompute the roster each iteration so bots that connect later
        // are picked up by the next match.
        let teams = if config.teams.is_empty() {
            config.registry.registered_teams()
        } else {
            config.teams.clone()
        };
        if teams.len() < 2 {
            eprintln!("Not enough teams to start exhibition match, trying again in a few...");
            tokio::time::sleep(tokio::time::Duration::from_mins(1)).await;
            continue;
        }
        let specs = default_specs(&config.params, &teams);

        let handle = MatchControlHandle::new(config.tps);
        *current_handle.lock().await = Some(handle.clone());
        let join = spawn_live_match(
            config.seed + u64::from(match_count.load(Ordering::SeqCst)),
            config.params.clone(),
            specs,
            teams,
            Arc::clone(&config.wasm_store),
            Arc::clone(&config.ws_registry),
            config.fuel_per_tick,
            config.observer_hub.clone(),
            Arc::clone(&config.recording_store),
            handle.clone(),
            (config.pacer_factory)(handle),
        )
        .await;
        let _ = join.await;
        match_count.fetch_add(1, Ordering::SeqCst);
        *current_handle.lock().await = None;
    }
}

pub(crate) fn check_facilitator_auth(
    headers: &HeaderMap,
    facilitator_password: &str,
) -> Result<(), StatusCode> {
    let Some(value) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    match value.strip_prefix("Facilitator ") {
        Some(password) if password == facilitator_password && !facilitator_password.is_empty() => {
            Ok(())
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMatchRequest {
    mode: String,
    seed: Option<u64>,
    max_ticks: Option<u32>,
    teams: Option<Vec<String>>,
    tps: Option<u32>,
}

#[derive(Deserialize)]
pub struct SetTpsRequest {
    tps: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartMatchResponse {
    match_id: String,
    mode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExhibitionStatus {
    running: bool,
    match_count: u32,
}

pub async fn post_admin_start_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StartMatchRequest>,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }

    let seed = body.seed.unwrap_or(state.match_seed);
    let mut params = state.match_params.clone();
    if let Some(max_ticks) = body.max_ticks {
        params.max_ticks = max_ticks;
    }
    let teams = body
        .teams
        .unwrap_or_else(|| state.registry.registered_teams());
    let specs = default_specs(&params, &teams);
    if teams.len() < 2 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "At least 2 registered teams needed."
            })),
        )
            .into_response();
    }

    match body.mode.as_str() {
        "headless" => {
            let runner = HeadlessRunner::new_with_health(
                Arc::clone(&state.wasm_store),
                Arc::clone(&state.recording_store),
                params,
                specs.clone(),
                teams.clone(),
                10_000_000,
                seed,
                Arc::clone(&state.dq_store),
                Arc::clone(&state.health_store),
            )
            .with_management(
                Arc::clone(&state.disabled_store),
                Arc::clone(&state.default_bot_store),
            );
            let result = tokio::task::spawn_blocking(move || runner.run_one_seeded(seed))
                .await
                .expect("headless match task panicked");

            // Feed the ladder.  Map each ship (specs[i]) to its team (teams[i])
            // by positional index — the same mapping used by consume_headless_results.
            let ship_to_team: HashMap<String, String> = specs
                .iter()
                .zip(teams.iter())
                .map(|(spec, team)| (spec.id.clone(), team.clone()))
                .collect();
            state.ladder.update_from_match(&result.outcome, |ship_id| {
                ship_to_team
                    .get(ship_id.as_str())
                    .cloned()
                    .unwrap_or_else(|| ship_id.to_string())
            });

            Json(StartMatchResponse {
                match_id: result.match_id,
                mode: "headless".to_owned(),
            })
            .into_response()
        }
        "live" => {
            let tps = body.tps.unwrap_or(30);
            let handle = MatchControlHandle::new(tps);
            let match_id = Uuid::new_v4().to_string();
            state
                .match_registry
                .register(match_id.clone(), handle.clone());
            spawn_registered_live_match(
                match_id.clone(),
                state,
                seed,
                params,
                specs,
                teams,
                handle,
            );
            Json(StartMatchResponse {
                match_id,
                mode: "live".to_owned(),
            })
            .into_response()
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

pub async fn post_admin_pause_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    let Some(handle) = state.match_registry.get(&id) else {
        return StatusCode::NOT_FOUND;
    };
    handle.pause();
    StatusCode::OK
}

pub async fn post_admin_resume_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    let Some(handle) = state.match_registry.get(&id) else {
        return StatusCode::NOT_FOUND;
    };
    handle.resume();
    StatusCode::OK
}

pub async fn post_admin_set_tps(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SetTpsRequest>,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    let Some(handle) = state.match_registry.get(&id) else {
        return StatusCode::NOT_FOUND;
    };
    handle.set_tps(body.tps);
    StatusCode::OK
}

pub async fn delete_admin_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    let Some(handle) = state.match_registry.get(&id) else {
        return StatusCode::NOT_FOUND;
    };
    handle.abort();
    state.match_registry.remove(&id);
    StatusCode::OK
}

pub async fn get_admin_exhibition(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    let running = state.exhibition.current_handle().await.is_some();
    Json(ExhibitionStatus {
        running,
        match_count: state.exhibition.match_count(),
    })
    .into_response()
}

#[derive(Debug, Deserialize)]

pub struct ExhibitionStartQuery {
    #[serde(default)]
    tps: Option<u32>,
}

pub async fn post_admin_exhibition_start(
    State(state): State<AppState>,
    Query(query): Query<ExhibitionStartQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    // `teams` is empty — the exhibition loop recomputes the roster from
    // connected bots at the start of each match iteration.
    let tps = query.tps.unwrap_or(30);
    state.exhibition.start(ExhibitionConfig {
        seed: state.match_seed,
        params: state.match_params.clone(),
        teams: vec![],
        wasm_store: Arc::clone(&state.wasm_store),
        ws_registry: Arc::clone(&state.ws_registry),
        registry: Arc::clone(&state.registry),
        fuel_per_tick: 10_000_000,
        observer_hub: state.observer_hub.clone(),
        recording_store: Arc::clone(&state.recording_store),
        tps,
        max_matches: 0,
        pacer_factory: Arc::new(move |h| {
                Box::new(ControlledPacer::new(h))
        }),
    });
    StatusCode::OK
}

pub async fn post_admin_exhibition_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.exhibition.stop().await;
    StatusCode::OK
}

fn spawn_registered_live_match(
    match_id: String,
    state: AppState,
    seed: u64,
    params: Params,
    mut specs: Vec<ShipSpec>,
    teams: Vec<String>,
    handle: MatchControlHandle,
) {
    tokio::spawn(async move {
        // Send matchStart envelope to every connected WS bot in this roster.
        let match_start_json = serde_json::to_string(&MatchStartMsg {
            type_: "matchStart",
        })
        .unwrap_or_default();
        for team in &teams {
            if let Some(session) = state.ws_registry.get(team) {
                session.try_send_envelope(match_start_json.clone());
            }
        }

        let drivers = resolve_drivers(
            seed,
            &params,
            &specs,
            &teams,
            Arc::clone(&state.wasm_store),
            Arc::clone(&state.ws_registry),
            10_000_000,
            Arc::clone(&state.dq_store),
            Arc::clone(&state.health_store),
            state.tick_deadline,
            Some(&state),
        );
        let pacer: Box<dyn TickPacer + Send> = Box::new(ControlledPacer::new(handle.clone()));
        let registry = Arc::clone(&state.match_registry);
        let id_for_remove = match_id.clone();
        for (i, team) in teams.iter().enumerate() {
            specs[i].id.clone_from(team);
        }
        // Build ship→team map before specs are moved into spawn_blocking.
        let ship_to_team: HashMap<String, String> = specs
            .iter()
            .zip(teams.iter())
            .map(|(spec, team)| (spec.id.clone(), team.clone()))
            .collect();

        let result = tokio::task::spawn_blocking(move || {
            run_live_controlled_with_id(
                match_id,
                seed,
                params,
                specs,
                drivers,
                pacer,
                handle,
                state.observer_hub,
                state.recording_store,
            )
        })
        .await;

        // Send matchEnd envelope to every connected WS bot in this roster.
        if let Ok((_, ref outcome)) = result {
            let match_end = MatchEndMsg {
                type_: "matchEnd",
                results: MatchResultsJson {
                    winner: outcome.winner.clone(),
                    scores: outcome.scores.iter().cloned().collect(),
                    ticks: outcome.ticks,
                },
            };
            let match_end_json = serde_json::to_string(&match_end).unwrap_or_default();
            for team in &teams {
                if let Some(session) = state.ws_registry.get(team) {
                    session.try_send_envelope(match_end_json.clone());
                }
            }
        }

        // Feed the ladder once the match finishes.
        if let Ok((_, outcome)) = result {
            state.ladder.update_from_match(&outcome, |ship_id| {
                ship_to_team
                    .get(ship_id.as_str())
                    .cloned()
                    .unwrap_or_else(|| ship_id.clone())
            });
        }

        registry.remove(&id_for_remove);
    });
}

#[allow(clippy::too_many_arguments)]
fn resolve_drivers(
    seed: u64,
    params: &Params,
    specs: &[ShipSpec],
    teams: &[String],
    wasm_store: Arc<WasmBotStore>,
    ws_registry: Arc<WsConnectionRegistry>,
    fuel_per_tick: u64,
    dq_store: Arc<DqStore>,
    health_store: Arc<BotHealthStore>,
    tick_deadline: Duration,
    state: Option<&AppState>,
) -> Vec<Box<dyn BotDriver>> {
    let mut resolver = ConnectionResolver::new(ws_registry, wasm_store, fuel_per_tick)
        .with_deadline(tick_deadline)
        .with_moderation(dq_store, health_store);
    if let Some(s) = state {
        resolver = resolver.with_management(
            Arc::clone(&s.disabled_store),
            Arc::clone(&s.default_bot_store),
        );
    }
    let engine0 = Engine::new(seed, params.clone(), specs.to_vec());
    let slots: Vec<Slot> = teams
        .iter()
        .zip(specs.iter())
        .map(|(team, spec)| {
            let tick0_obs_json = engine0
                .observation(&spec.id)
                .map(|obs| obs_to_tick_json(0, &obs))
                .unwrap_or_default();
            Slot {
                team: team.clone(),
                tick0_obs_json,
            }
        })
        .collect();
    resolver.resolve(&slots, params)
}

// ── Health & moderation handlers ─────────────────────────────────────────────

/// `GET /admin/bots` — list health for every known bot.
///
/// Returns an array of [`BotHealthSnapshot`](crate::health::BotHealthSnapshot)
/// objects, one per team.  Facilitator-gated.
pub async fn get_admin_bots(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    Json(state.health_store.list_snapshots()).into_response()
}

/// `POST /admin/bots/{team}/kick` — disqualify a misbehaving bot.
///
/// - Adds the team to the [`DqStore`](crate::health::DqStore) so:
///   - The live match's [`ExclusionDriver`](crate::health::ExclusionDriver) will
///     return `None` from the next tick onward (ship falls back to engine
///     persistence / Default behaviour).
///   - Future match resolutions skip WS and WASM bots for this team and
///     assign the Default Bot instead.
/// - Marks the team's health entry `connected=false`.
/// - Facilitator-gated.
pub async fn post_admin_kick_bot(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.dq_store.disqualify(&team);
    if let Some(entry) = state.health_store.get(&team) {
        entry.set_connected(false);
    }
    StatusCode::OK
}

/// Build a live-match roster from currently-connected WS bots.
///
/// Starts with `ws_registry.connected_teams()`, then fills up to a minimum
/// of 2 slots with placeholder team names ("team-a", "team-b") so the match
/// field is always valid.  Connected bots that share a name with a placeholder
/// are not duplicated.
// TODO: Remove idiotic placeholder BS.
//  Gather WS & WASM bots
//  Resolve to actual fucking team names
fn live_teams(ws_registry: &WsConnectionRegistry) -> Vec<String> {
    let mut teams = ws_registry.connected_teams();
    for placeholder in &["team-a", "team-b"] {
        if teams.len() >= 2 {
            break;
        }
        let p = (*placeholder).to_owned();
        if !teams.contains(&p) {
            teams.push(p);
        }
    }
    teams
}

fn default_specs(params: &Params, teams: &[String]) -> Vec<ShipSpec> {
    teams
        .iter()
        .enumerate()
        .map(|(idx, t)| {
            let fraction = (idx + 1) as f32 / (teams.len() + 1) as f32;
            ShipSpec {
                id: t.clone(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2::new(params.arena_w * fraction, params.arena_h * 0.5),
            }
        })
        .collect()
}

// ── Bot management handlers (issue 13) ───────────────────────────────────────

/// `POST /admin/bots/{team}` — facilitator-gated WASM upload on behalf of a team.
///
/// Identical semantics to `POST /bots` but authenticated with the facilitator
/// password instead of a team token.  Allows the facilitator to upload/replace
/// a bot without needing the team's credentials.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Artifact stored (or replaced). |
/// | **400 Bad Request** | Body does not start with WASM magic bytes. |
/// | **401 Unauthorized** | Wrong or absent facilitator auth. |
pub async fn post_admin_upload_bot(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    if body.len() < 4 || &body[..4] != b"\0asm" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "body must be a valid WASM artifact (missing magic bytes \\0asm)"})),
        )
            .into_response();
    }
    state.wasm_store.store(&team, body.to_vec());
    StatusCode::OK.into_response()
}

/// `POST /admin/bots/{team}/disable` — reversibly disable a team's bot.
///
/// A disabled team's slot resolves to the Default Bot in subsequent match
/// resolutions.  Re-enabling (via `.../enable`) restores normal priority.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Team marked as disabled. |
/// | **401 Unauthorized** | Wrong or absent facilitator auth. |
pub async fn post_admin_disable_bot(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.disabled_store.disable(&team);
    StatusCode::OK
}

/// `POST /admin/bots/{team}/enable` — re-enable a previously disabled team's bot.
///
/// Restores normal WS → WASM → Default resolution for the team.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Team re-enabled (no-op if not disabled). |
/// | **401 Unauthorized** | Wrong or absent facilitator auth. |
pub async fn post_admin_enable_bot(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.disabled_store.enable(&team);
    StatusCode::OK
}

/// `POST /admin/default-bot` — set/replace the custom Default Bot artifact.
///
/// When set, the resolver's Priority-3 (Default Bot) path instantiates a
/// WASM driver from this artifact instead of the built-in heuristic.  On WASM
/// instantiation failure the built-in driver is used so matches never abort.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Artifact stored. |
/// | **400 Bad Request** | Body does not start with WASM magic bytes. |
/// | **401 Unauthorized** | Wrong or absent facilitator auth. |
pub async fn post_admin_set_default_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    if body.len() < 4 || &body[..4] != b"\0asm" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "body must be a valid WASM artifact (missing magic bytes \\0asm)"})),
        )
            .into_response();
    }
    state.default_bot_store.set(body.to_vec());
    StatusCode::OK.into_response()
}

/// `DELETE /admin/default-bot` — clear the custom Default Bot.
///
/// Future matches revert to the built-in [`crate::bot::DefaultBotDriver`]
/// heuristic for unoccupied slots.
///
/// # Responses
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Custom Default Bot cleared. |
/// | **401 Unauthorized** | Wrong or absent facilitator auth. |
pub async fn delete_admin_default_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.default_bot_store.clear();
    StatusCode::OK
}

// ── LadderRunner ─────────────────────────────────────────────────────────────

/// Managed handle for the background headless ladder runner (issue 12/admin).
///
/// Mirrors [`ExhibitionSupervisor`]: holds the ability to start a
/// [`HeadlessRunner::spawn_loop`] that feeds results into the [`Ladder`] via
/// [`crate::ladder::consume_headless_results`], and to stop it via the
/// `watch::Sender<bool>` from `spawn_loop`.
///
/// ## Roster / params
///
/// The runner uses a fixed default roster of `["team-a", "team-b"]` (the same
/// default used by exhibition matches and `POST /admin/matches`).  Uploaded
/// WASM bots for those teams (in `wasm_store`) are used automatically via the
/// [`crate::resolver::ConnectionResolver`]; unoccupied slots fall back to the
/// Default Bot.  The AppState `match_params` and stores (wasm, disabled,
/// default_bot, dq, health) are respected.
///
/// ## Running state
///
/// `is_running()` returns `true` between a successful `start()` and a
/// subsequent `stop()`.  The flag is set synchronously in `start()` / `stop()`
/// so HTTP handlers can read it immediately without waiting for background
/// tasks.
pub struct LadderRunner {
    running: Arc<AtomicBool>,
    stop_tx: Arc<tokio::sync::Mutex<Option<watch::Sender<bool>>>>,
}

impl LadderRunner {
    /// Create a new, stopped `LadderRunner` wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            running: Arc::new(AtomicBool::new(false)),
            stop_tx: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// Returns `true` if the background loop is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the headless ladder runner loop (idempotent — no-op if already running).
    ///
    /// Spawns:
    /// 1. A [`HeadlessRunner::spawn_loop`] background task that runs
    ///    headless matches continuously until stopped.
    /// 2. A consumer task ([`crate::ladder::consume_headless_results`]) that
    ///    feeds each finished result into `state.ladder`.
    ///
    /// The `watch::Sender<bool>` from `spawn_loop` is retained so `stop()`
    /// can signal a graceful halt.
    pub fn start(&self, state: &AppState) {
        // Idempotent: skip if already running.
        if self.running.load(Ordering::SeqCst) {
            return;
        }

        let teams = state.registry.registered_teams();
        let specs = default_specs(&state.match_params, &teams);

        let runner = HeadlessRunner::new_with_health(
            Arc::clone(&state.wasm_store),
            Arc::clone(&state.recording_store),
            state.match_params.clone(),
            specs,
            teams.clone(),
            10_000_000,
            state.match_seed,
            Arc::clone(&state.dq_store),
            Arc::clone(&state.health_store),
        )
        .with_management(
            Arc::clone(&state.disabled_store),
            Arc::clone(&state.default_bot_store),
        );

        let (stop_tx, result_rx, _join) = runner.spawn_loop();

        // Store the stop sender so stop() can signal a halt.
        if let Ok(mut slot) = self.stop_tx.try_lock() {
            *slot = Some(stop_tx);
        }

        // Mark running BEFORE spawning so the HTTP status endpoint sees it
        // immediately after start() returns.
        self.running.store(true, Ordering::SeqCst);

        // Spawn the consumer task: feed results into the ladder.
        let ladder = Arc::clone(&state.ladder);
        let running_flag = Arc::clone(&self.running);
        tokio::spawn(async move {
            crate::ladder::consume_headless_results(result_rx, ladder, &teams).await;
            // When the channel closes (loop stopped), clear the running flag.
            running_flag.store(false, Ordering::SeqCst);
        });
    }

    /// Stop the background loop (graceful: current in-flight match finishes).
    ///
    /// Sets `is_running()` to `false` synchronously so the HTTP status
    /// endpoint reflects the change immediately.
    pub async fn stop(&self) {
        // Clear the running flag immediately (observable via the status endpoint).
        self.running.store(false, Ordering::SeqCst);
        if let Some(tx) = self.stop_tx.lock().await.as_ref() {
            let _ = tx.send(true);
        }
    }
}

// ── Ladder runner status DTO ──────────────────────────────────────────────────

#[derive(Serialize)]
struct LadderRunnerStatus {
    running: bool,
}

// ── Recording download DTO ────────────────────────────────────────────────────

// NOTE: RecordingDownloadDto was removed in issue B2b.  The download endpoint
// now returns a full `Recording` (serde_json serialised), which is a complete,
// re-importable artifact containing match_id, seed, params, specs, intent_log,
// and meta.  See `get_admin_recording_download` and PROTOCOL.md §download.

// ── Ladder runner handlers ────────────────────────────────────────────────────

/// `GET /admin/ladder/runner` — report whether the headless ladder loop is running.
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// # Response
///
/// ```json
/// { "running": true }
/// ```
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Status returned. |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
pub async fn get_admin_ladder_runner(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    Json(LadderRunnerStatus {
        running: state.ladder_runner.is_running(),
    })
    .into_response()
}

/// `POST /admin/ladder/runner/start` — start the headless ladder loop (idempotent).
///
/// Spawns a background headless match loop that feeds results into the TrueSkill
/// ladder.  If the loop is already running this is a no-op (200 OK).
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Loop started (or was already running). |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
pub async fn post_admin_ladder_runner_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.ladder_runner.start(&state);
    StatusCode::OK
}

/// `POST /admin/ladder/runner/stop` — stop the headless ladder loop.
///
/// Signals the background loop to halt after the current in-flight match
/// completes.  Safe to call when the loop is not running (no-op).
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Stop signal sent (or loop was already stopped). |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
pub async fn post_admin_ladder_runner_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    state.ladder_runner.stop().await;
    StatusCode::OK
}

// ── Recording download handler ────────────────────────────────────────────────

/// `GET /admin/recordings/{id}/download` — download a recording as a full JSON artifact.
///
/// Returns the complete [`Recording`] serialised as JSON — a re-importable
/// artifact containing `match_id`, `seed`, `params`, `specs`, `intent_log`,
/// and `meta`.  This artifact can be fed directly to
/// `POST /admin/recordings/import` on any server instance to restore the
/// recording and make it replayable.
///
/// ## Response shape
///
/// ```json
/// {
///   "match_id": "…",
///   "seed": 42,
///   "params": { … },
///   "specs": [ … ],
///   "intent_log": [ … ],
///   "meta": {
///     "match_id": "…",
///     "seed": 42,
///     "tick_count": 30,
///     "winner": "ship-0",
///     "scores": [["ship-0", 1.5], ["ship-1", 0.0]]
///   }
/// }
/// ```
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Full recording returned as JSON. |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
/// | **404 Not Found** | No recording with the given `id`. |
pub async fn get_admin_recording_download(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    match state.recording_store.get(&id) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "recording not found"})),
        )
            .into_response(),
        Some(rec) => Json(rec).into_response(),
    }
}

// ── Recording import handler ──────────────────────────────────────────────────

/// `POST /admin/recordings/import` — import a recording artifact into the store.
///
/// Accepts a full [`Recording`] JSON body (as produced by
/// `GET /admin/recordings/{id}/download`).  After import the recording is:
/// - Returned by `GET /recordings` (appears in the listing).
/// - Retrievable via `recording_store.get(match_id)`.
/// - Replayable via `POST /recordings/{id}/replay`.
/// - If the store has a persistence directory configured, the recording is
///   also written to disk immediately.
///
/// # Auth
///
/// ```text
/// Authorization: Facilitator <facilitator_password>
/// ```
///
/// | Status | Meaning |
/// |--------|---------|
/// | **200 OK** | Recording imported and stored. |
/// | **400 Bad Request** | Body is absent or not a valid Recording JSON. |
/// | **401 Unauthorized** | Missing, malformed, or wrong facilitator password. |
pub async fn post_admin_recording_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status.into_response();
    }
    match serde_json::from_slice::<Recording>(&body) {
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("malformed recording body: {e}")})),
        )
            .into_response(),
        Ok(rec) => {
            state.recording_store.record(rec);
            StatusCode::OK.into_response()
        }
    }
}
