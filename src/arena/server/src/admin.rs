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
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    headless::HeadlessRunner,
    health::{BotHealthStore, DqStore},
    observer::ObserverHub,
    pacer::NoopPacer,
    recording::{Recording, RecordingMeta, RecordingStore},
    resolver::{ConnectionResolver, Slot, WsConnectionRegistry},
    routes::AppState,
    runner::{BotDriver, MatchOutcome, MatchRunner, TickPacer},
    store::WasmBotStore,
    ws::obs_to_tick_json,
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
    specs: Vec<ShipSpec>,
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
        ws_registry,
        fuel_per_tick,
        DqStore::new(),
        BotHealthStore::new(),
        None,
    );
    tokio::task::spawn_blocking(move || {
        run_live_controlled(
            seed,
            params,
            specs,
            drivers,
            pacer,
            handle,
            observer_hub,
            recording_store,
        )
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
    pub specs: Vec<ShipSpec>,
    pub teams: Vec<String>,
    pub wasm_store: Arc<WasmBotStore>,
    pub ws_registry: Arc<WsConnectionRegistry>,
    pub fuel_per_tick: u64,
    pub observer_hub: ObserverHub,
    pub recording_store: Arc<RecordingStore>,
    pub tps: u32,
    pub max_matches: u32,
    pub pacer_factory: Arc<dyn Fn() -> Box<dyn TickPacer + Send> + Send + Sync>,
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

        let handle = MatchControlHandle::new(config.tps);
        *current_handle.lock().await = Some(handle.clone());
        let join = spawn_live_match(
            config.seed + u64::from(match_count.load(Ordering::SeqCst)),
            config.params.clone(),
            config.specs.clone(),
            config.teams.clone(),
            Arc::clone(&config.wasm_store),
            Arc::clone(&config.ws_registry),
            config.fuel_per_tick,
            config.observer_hub.clone(),
            Arc::clone(&config.recording_store),
            handle,
            (config.pacer_factory)(),
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
    let teams = normalized_teams(body.teams);
    let specs = default_specs(&params, teams.len());

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

pub async fn post_admin_exhibition_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(status) = check_facilitator_auth(&headers, &state.facilitator_password) {
        return status;
    }
    let teams = normalized_teams(None);
    let specs = default_specs(&state.match_params, teams.len());
    state.exhibition.start(ExhibitionConfig {
        seed: state.match_seed,
        params: state.match_params.clone(),
        specs,
        teams,
        wasm_store: Arc::clone(&state.wasm_store),
        ws_registry: Arc::clone(&state.ws_registry),
        fuel_per_tick: 10_000_000,
        observer_hub: state.observer_hub.clone(),
        recording_store: Arc::clone(&state.recording_store),
        tps: 30,
        max_matches: 0,
        pacer_factory: Arc::new(|| Box::new(NoopPacer)),
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
    specs: Vec<ShipSpec>,
    teams: Vec<String>,
    handle: MatchControlHandle,
) {
    tokio::spawn(async move {
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
            Some(&state),
        );
        let pacer: Box<dyn TickPacer + Send> = Box::new(ControlledPacer::new(handle.clone()));
        let registry = Arc::clone(&state.match_registry);
        let id_for_remove = match_id.clone();

        // Build ship→team map before specs are moved into spawn_blocking.
        // Map: specs[i].id → teams[i], matching the headless and consume_headless_results approach.
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

        // Feed the ladder once the match finishes (state.ladder was not moved above).
        if let Ok((_, outcome)) = result {
            state.ladder.update_from_match(&outcome, |ship_id| {
                ship_to_team
                    .get(ship_id.as_str())
                    .cloned()
                    .unwrap_or_else(|| ship_id.to_string())
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
    state: Option<&AppState>,
) -> Vec<Box<dyn BotDriver>> {
    let mut resolver = ConnectionResolver::new(ws_registry, wasm_store, fuel_per_tick)
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

fn normalized_teams(teams: Option<Vec<String>>) -> Vec<String> {
    let teams = teams.unwrap_or_else(|| vec!["team-a".to_owned(), "team-b".to_owned()]);
    if teams.is_empty() {
        vec!["team-a".to_owned(), "team-b".to_owned()]
    } else {
        teams
    }
}

fn default_specs(params: &Params, count: usize) -> Vec<ShipSpec> {
    (0..count)
        .map(|idx| {
            let fraction = (idx + 1) as f32 / (count + 1) as f32;
            ShipSpec {
                id: format!("ship-{idx}"),
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
