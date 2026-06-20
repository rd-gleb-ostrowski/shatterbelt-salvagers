//! Bare-minimal WASM Bot for Shatterbelt Salvagers — Rust reference.
//!
//! This hides nothing about the core-wasm ABI (ADR-0004 / PROTOCOL.md §9). The
//! module exports exactly four things the Arena host calls:
//!
//! * `memory`      — our linear memory (cdylib exports it automatically).
//! * `alloc(len)`  — the host asks us for a buffer, then writes observation JSON into it.
//! * `init(ptr,len)` — called once before the match with the tick-0 observation.
//! * `tick(ptr,len) -> i64` — read the observation at `[ptr,len)`, write an action
//!   into our memory, and return the packed `(out_ptr << 32) | out_len`.
//!
//! The instance persists for the whole match, so state lives in module globals.
//! There are no host imports. The JSON itself is parsed with `serde_json`; the
//! ABI plumbing (pointers, lengths, the packed return) is written out by hand.

use std::os::raw::c_void;
use std::ptr::addr_of_mut;

/// Buffer holding the most recent action JSON. The host reads it immediately
/// after `tick` returns (single-threaded, synchronous), before the next call,
/// so overwriting it each tick is safe and avoids leaking.
static mut OUT: Vec<u8> = Vec::new();

/// Our ship id, captured from the tick-0 observation in `init`, to demonstrate
/// that `init` really receives it (a real bot would precompute here).
static mut MY_ID: String = String::new();

/// Allocate `len` bytes in our linear memory and hand the host the pointer.
///
/// The host writes the observation JSON into this buffer, then passes the same
/// pointer + length to `init`/`tick`. We deliberately leak it: the host owns the
/// buffer's lifetime for the duration of the call.
#[no_mangle]
pub extern "C" fn alloc(len: i32) -> *mut c_void {
    let mut buf = Vec::<u8>::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as *mut c_void
}

/// Read `len` bytes of UTF-8 JSON at `ptr` out of our own linear memory.
///
/// # Safety
/// `ptr`/`len` come from the host and point at a buffer it filled via `alloc`.
unsafe fn read_json(ptr: *const u8, len: i32) -> serde_json::Value {
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
}

/// Called once before the match with the tick-0 observation JSON.
#[no_mangle]
pub extern "C" fn init(ptr: *const u8, len: i32) {
    let obs = unsafe { read_json(ptr, len) };
    if let Some(id) = obs.get("self").and_then(|s| s.get("id")).and_then(|v| v.as_str()) {
        unsafe { *addr_of_mut!(MY_ID) = id.to_owned() };
    }
}

/// Called each tick: parse the observation, decide an action, write the action
/// JSON into `OUT`, and return the packed `(out_ptr << 32) | out_len`.
#[no_mangle]
pub extern "C" fn tick(ptr: *const u8, len: i32) -> i64 {
    let obs = unsafe { read_json(ptr, len) };

    // --- trivial placeholder decision: steer at the nearest relic, thrust, fire ---
    // Real parsing of the observation, but obviously not a strategy.
    let me = &obs["self"];
    let (px, py) = (
        me["pos"]["x"].as_f64().unwrap_or(0.0),
        me["pos"]["y"].as_f64().unwrap_or(0.0),
    );
    let heading = me["heading"].as_f64().unwrap_or(0.0);

    let mut turn = 0.0_f64;
    if let Some(relics) = obs["relics"].as_array() {
        if let Some(nearest) = relics.iter().min_by(|a, b| {
            let da = (a["pos"]["x"].as_f64().unwrap_or(0.0) - px).powi(2)
                + (a["pos"]["y"].as_f64().unwrap_or(0.0) - py).powi(2);
            let db = (b["pos"]["x"].as_f64().unwrap_or(0.0) - px).powi(2)
                + (b["pos"]["y"].as_f64().unwrap_or(0.0) - py).powi(2);
            da.total_cmp(&db)
        }) {
            let want = (nearest["pos"]["y"].as_f64().unwrap_or(0.0) - py)
                .atan2(nearest["pos"]["x"].as_f64().unwrap_or(0.0) - px);
            let diff = (want - heading).sin().atan2((want - heading).cos());
            turn = diff.clamp(-1.0, 1.0);
        }
    }

    let action = serde_json::json!({
        "type": "action",
        "turn": turn,
        "thrust": 1.0,
        "fire": true,
    });

    // Serialize into OUT and return the packed pointer/length.
    let bytes = serde_json::to_vec(&action).unwrap_or_default();
    unsafe {
        *addr_of_mut!(OUT) = bytes;
        let out = &*addr_of_mut!(OUT);
        let out_ptr = out.as_ptr() as u64;
        let out_len = out.len() as u64;
        ((out_ptr << 32) | out_len) as i64
    }
}
