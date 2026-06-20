//! WASM Bot host: compiles, instantiates, and drives uploaded `.wasm` modules
//! via wasmtime (ADR-0004, PROTOCOL §9).
//!
//! # ABI contract (ADR-0004)
//!
//! A bot module exports:
//! - `memory` — linear memory.
//! - `alloc(len: i32) -> i32` — host calls this to obtain a write buffer.
//! - `init(ptr: i32, len: i32)` — called once before the match with the
//!   tick-0 observation JSON (warm-up, pays JIT cost up front).
//! - `tick(ptr: i32, len: i32) -> i64` — per-tick: reads observation JSON at
//!   `[ptr, len)`, writes action JSON into its own memory, returns packed
//!   `(out_ptr << 32) | out_len`.
//!
//! Optional host import: `env::log(ptr, len)` for debug output.
//!
//! # Fuel
//!
//! Per-tick compute is bounded by a [`fuel_per_tick`](WasmBotDriver) budget
//! (wasmtime instruction-fuel; [`Config::consume_fuel`]).  Exhausting fuel
//! yields `None` from `decide` — the engine carries the ship's previous intent
//! forward.  Any other trap is equally degraded rather than crashing the match.
//!
//! # Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 06 (resolver) | `WasmBotDriver::new(store.get(team), obs0_json, budget)` |
//! | 09 (headless-fast) | `WasmBotDriver` + `NoopPacer` = CPU-speed ladder |
//! | 12 (admin) | `drain_log()`, `fuel_exhausted_count()`, `trap_count()` |

use anyhow::{anyhow, Result};
use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store, Trap, TypedFunc};

use arena_engine::{Intent, Observation};

use crate::runner::BotDriver;
use crate::ws::{obs_to_tick_json, parse_action};

// ── DriverState ───────────────────────────────────────────────────────────────

/// Host-side state stored inside the wasmtime [`Store`].
///
/// The `log` import writes into `log_buffer`; the admin (issue 12) drains it
/// via [`WasmBotDriver::drain_log`].  Fault counters accumulate for every tick
/// that ends in a fuel-exhaustion or other trap.
struct DriverState {
    /// Bytes appended by the bot's `log(ptr, len)` calls (in order).
    log_buffer: Vec<u8>,
    /// Ticks where the bot ran out of wasmtime fuel (→ None, match continues).
    fuel_exhausted_count: u64,
    /// Ticks where the bot trapped for a non-fuel reason (→ None, match continues).
    trap_count: u64,
}

// ── WasmBotDriver ─────────────────────────────────────────────────────────────

/// A [`BotDriver`] that runs an uploaded WASM module in-process via wasmtime.
///
/// ## Construction
///
/// Build with [`WasmBotDriver::new`].  This compiles the module, instantiates
/// it, and calls `init` with the pre-serialised tick-0 observation JSON so the
/// bot can warm up and pay any JIT cost before the match starts.
///
/// ```no_run
/// use arena_server::wasm_host::WasmBotDriver;
///
/// # let wasm_bytes: Vec<u8> = vec![];
/// # let tick0_json = String::new();
/// let driver = WasmBotDriver::new(&wasm_bytes, &tick0_json, 10_000_000).unwrap();
/// ```
///
/// ## Observation / action contract
///
/// Both the observation written to the module and the action read back from it
/// use the **same PROTOCOL §6 JSON** as the WS path (`obs_to_tick_json` /
/// `parse_action` from `ws.rs`).  One schema, two transports.
///
/// ## Fuel
///
/// [`BotDriver::decide`] resets the store fuel to `fuel_per_tick` before every
/// `tick` call.  Exhausting fuel returns `None`; the engine's per-field intent
/// persistence applies the previous turn/thrust/fire values.  The fault counter
/// increments for monitoring.
pub struct WasmBotDriver {
    store: Store<DriverState>,
    memory: Memory,
    alloc_fn: TypedFunc<i32, i32>,
    tick_fn: TypedFunc<(i32, i32), i64>,
    fuel_per_tick: u64,
    /// Issue 12: shared health entry updated after every tick.
    health: Option<std::sync::Arc<crate::health::BotHealthEntry>>,
}

impl WasmBotDriver {
    /// Compile, instantiate, and warm-up a WASM bot.
    ///
    /// # Parameters
    ///
    /// - `wasm_bytes` — raw `.wasm` bytes from [`crate::store::WasmBotStore`].
    /// - `tick0_obs_json` — tick-0 observation serialised with
    ///   [`crate::ws::obs_to_tick_json`].  The bot's `init` is called with
    ///   this so it can allocate and precompute before the match.
    /// - `fuel_per_tick` — wasmtime fuel budget per `tick` call.
    ///   `10_000_000` is a generous default; tests can pass `100` to trigger
    ///   exhaustion deterministically.
    ///
    /// # Errors
    ///
    /// Returns an error if the module fails to compile, lacks a required export
    /// (`memory`, `alloc`, `init`, `tick`), or if the `init` call traps.
    pub fn new(wasm_bytes: &[u8], tick0_obs_json: &str, fuel_per_tick: u64) -> Result<Self> {
        let engine = {
            let mut cfg = Config::new();
            cfg.consume_fuel(true);
            Engine::new(&cfg).map_err(|e| anyhow!("failed to create wasmtime Engine: {e}"))?
        };

        let module = Module::from_binary(&engine, wasm_bytes)
            .map_err(|e| anyhow!("failed to compile WASM module: {e}"))?;

        let mut store = Store::new(
            &engine,
            DriverState {
                log_buffer: Vec::new(),
                fuel_exhausted_count: 0,
                trap_count: 0,
            },
        );

        // Define the optional `env::log(ptr, len)` host import.
        // Bots that don't import it simply never call it.
        let mut linker: Linker<DriverState> = Linker::new(&engine);
        linker.func_wrap(
            "env",
            "log",
            |mut caller: Caller<'_, DriverState>, ptr: i32, len: i32| {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return,
                };
                // Safety: read is bounds-checked; copy before mutating caller.
                let start = ptr as usize;
                let end = start.saturating_add(len as usize);
                let bytes: Vec<u8> = {
                    let data = mem.data(&caller);
                    if end <= data.len() {
                        data[start..end].to_owned()
                    } else {
                        return;
                    }
                };
                caller.data_mut().log_buffer.extend_from_slice(&bytes);
            },
        )
        .map_err(|e| anyhow!("failed to define env::log import: {e}"))?;

        // Any imports the module declares beyond `env::log` (e.g., WASI stubs)
        // are satisfied with trapping functions so instantiation doesn't fail.
        linker
            .define_unknown_imports_as_traps(&module)
            .map_err(|e| anyhow!("failed to define unknown imports as traps: {e}"))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| anyhow!("failed to instantiate WASM module: {e}"))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("WASM module must export `memory`"))?;

        let alloc_fn: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, "alloc")
            .map_err(|e| anyhow!("WASM module must export `alloc(i32) -> i32`: {e}"))?;

        let init_fn: TypedFunc<(i32, i32), ()> = instance
            .get_typed_func(&mut store, "init")
            .map_err(|e| anyhow!("WASM module must export `init(i32, i32)`: {e}"))?;

        let tick_fn: TypedFunc<(i32, i32), i64> = instance
            .get_typed_func(&mut store, "tick")
            .map_err(|e| anyhow!("WASM module must export `tick(i32, i32) -> i64`: {e}"))?;

        // Warm-up: call `init` with tick-0 observation JSON (ADR-0004).
        // Give init a generous fuel budget so slow JIT warm-up doesn't exhaust it.
        store
            .set_fuel(fuel_per_tick.saturating_mul(100))
            .map_err(|e| anyhow!("fuel unavailable (engine not configured with consume_fuel): {e}"))?;

        let obs_bytes = tick0_obs_json.as_bytes();
        let obs_len = i32::try_from(obs_bytes.len())
            .map_err(|e| anyhow!("tick-0 JSON too large for i32: {e}"))?;

        let buf_ptr = alloc_fn
            .call(&mut store, obs_len)
            .map_err(|e| anyhow!("alloc trapped during init warm-up: {e}"))?;

        memory
            .write(&mut store, buf_ptr as usize, obs_bytes)
            .map_err(|_| anyhow!("failed to write tick-0 JSON into WASM memory"))?;

        init_fn
            .call(&mut store, (buf_ptr, obs_len))
            .map_err(|e| anyhow!("init trapped during warm-up: {e}"))?;

        Ok(Self { store, memory, alloc_fn, tick_fn, fuel_per_tick, health: None })
    }

    /// Inject a shared health entry so `decide` can update it each tick.
    pub fn set_health(&mut self, health: std::sync::Arc<crate::health::BotHealthEntry>) {
        self.health = Some(health);
    }

    // ── Observability (issue 12 seams) ────────────────────────────────────────

    /// Drain and return bytes written to the `log` import since the last drain.
    ///
    /// The Admin (issue 12) calls this after each tick (or batch) to surface
    /// bot debug output in the facilitator UI / viewer stream.
    pub fn drain_log(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.store.data_mut().log_buffer)
    }

    /// Number of ticks where the bot exhausted its fuel budget.
    ///
    /// Each such tick returns `None` from `decide`; the engine persists the
    /// previous intent.  Issue 12 (admin) surfaces this as a bot health metric.
    pub fn fuel_exhausted_count(&self) -> u64 {
        self.store.data().fuel_exhausted_count
    }

    /// Number of ticks where the bot trapped for a reason other than fuel.
    ///
    /// Includes `unreachable`, stack overflow, memory out-of-bounds, etc.
    /// Issue 12 (admin) surfaces this as a bot health metric.
    pub fn trap_count(&self) -> u64 {
        self.store.data().trap_count
    }

    /// Inner decide — computes the intent without touching health fields.
    fn decide_inner(&mut self, tick: u32, obs: &Observation) -> Option<Intent> {
        // Fresh fuel budget for this tick.
        self.store.set_fuel(self.fuel_per_tick).ok()?;

        // Serialise the observation using the shared PROTOCOL §6 contract.
        let json = obs_to_tick_json(tick, obs);
        let obs_bytes = json.as_bytes();
        let obs_len = i32::try_from(obs_bytes.len()).ok()?;

        // Ask the bot to allocate a buffer, then write the observation JSON.
        let buf_ptr = self.alloc_fn.call(&mut self.store, obs_len).ok()?;
        self.memory
            .write(&mut self.store, buf_ptr as usize, obs_bytes)
            .ok()?;

        // Call tick; degrade any trap to None.
        let packed = match self.tick_fn.call(&mut self.store, (buf_ptr, obs_len)) {
            Ok(v) => v,
            Err(e) => {
                let is_fuel = e
                    .downcast_ref::<Trap>()
                    .is_some_and(|t| *t == Trap::OutOfFuel);
                if is_fuel {
                    self.store.data_mut().fuel_exhausted_count += 1;
                } else {
                    self.store.data_mut().trap_count += 1;
                }
                return None;
            }
        };

        // Unpack (out_ptr << 32) | out_len from the i64 return value.
        let out_ptr = ((packed as u64) >> 32) as usize;
        let out_len = (packed as u32) as usize;

        // Read action JSON from WASM linear memory.
        let mut action_buf = vec![0u8; out_len];
        self.memory.read(&self.store, out_ptr, &mut action_buf).ok()?;

        let action_str = std::str::from_utf8(&action_buf).ok()?;

        // Parse into an engine Intent using the shared WS-path parser.
        parse_action(action_str).ok()
    }
}

// ── BotDriver impl ────────────────────────────────────────────────────────────

impl BotDriver for WasmBotDriver {
    fn kind(&self) -> &'static str {
        "wasm"
    }

    /// Run one tick of the WASM bot.
    ///
    /// ## Fuel reset
    ///
    /// The store's fuel is reset to `fuel_per_tick` at the top of each call so
    /// every tick gets a fresh, equal budget regardless of what the previous
    /// tick consumed.
    ///
    /// ## Error handling
    ///
    /// Any failure (fuel exhaustion, other trap, bad action JSON, memory OOB)
    /// returns `None`.  The engine's per-field intent persistence keeps the
    /// ship moving on its previous intent; the match never crashes.
    fn decide(&mut self, tick: u32, obs: &Observation) -> Option<Intent> {
        let result = self.decide_inner(tick, obs);

        // Issue 12: sync health metrics after every tick.
        if let Some(h) = &self.health {
            let total_faults =
                self.store.data().fuel_exhausted_count + self.store.data().trap_count;
            h.set_crashes(total_faults);
            // Drain the log buffer into the health entry.
            let log_bytes = std::mem::take(&mut self.store.data_mut().log_buffer);
            h.append_logs(&log_bytes);
            if result.is_some() {
                h.touch();
            } else {
                h.increment_skipped();
            }
        }

        result
    }
}
