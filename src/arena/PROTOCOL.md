# Arena Protocol — Shatterbelt Salvagers

The technical contract between the **Arena** (the hosted Rust server) and a **Bot**. A bot only
decides actions; the Arena owns all rules and physics. This document is the single source of
truth for the per-tick wire format and the WASM ABI.

> **Status:** v1 design. All gameplay numbers (ranges, damage, costs, the tick rate) are
> placeholders pending a balance pass — they're marked as such.

Related decisions: ADR-0001 (connection model), ADR-0002 (tech stack), ADR-0003 (this protocol),
ADR-0004 (WASM ABI).

---

## 1. Roles & connections

- **Arena** — authoritative server: runs the rapier2d physics sim, applies rules, scores matches.
- **Bot** — controls one **Salvage Ship**. Connects one of three ways, in priority order
  (ADR-0001): a **WS Bot** (outbound WebSocket), a **WASM Bot** (uploaded, run in-process), or the
  **Default Bot** fallback.
- **Viewer** — a read-only WebSocket client that receives the full "god-mode" stream for the
  projector and for recording replays. Bots never receive the viewer stream.

Both WS and WASM bots consume the **same JSON observation** and return the **same JSON action**,
so one bot's decision logic targets either path unchanged.

## 2. Timing model

- The match advances in **ticks at ~30 Hz** (≈33 ms/tick; rate is configurable — *balance*).
- Each tick the Arena sends every bot its observation, then waits up to a **per-tick deadline**
  for an action. If none arrives, the bot's **previous action persists**.
- A bot that sends nothing for many consecutive ticks may be dropped (replaced by the Default
  Bot for the rest of the match).
- **Replay:** the Arena records the *applied action per ship per tick* plus the match **seed**;
  rapier2d's deterministic mode reproduces the match exactly. Bots themselves need not be
  deterministic.

## 3. Coordinates & units

- Positions are in **arena units**; the Drift spans `arena.width × arena.height`, origin top-left,
  `x` right, `y` down.
- **Angles are radians**, `0 = +x` (East), increasing **counter-clockwise**.
- **Velocities are units per tick**; angular rates are radians per tick.

## 4. Registration & auth

Auth is intentionally simple — pre-shared passwords, no accounts.

- **Register (REST):** `POST /register` with the shared **event password** and a team name →
  returns a **token** for that team. The token authorizes the two bot paths:
  - **Upload a WASM bot:** `POST /bots` with the token + the `.wasm` artifact (replaces the
    team's current WASM Bot).
  - **Connect a WS bot:** include the token in the `join` message (below).
- **Admin** endpoints (match control, ladder control, kicks) are gated by a separate
  **facilitator password**.

## 5. WS connection handshake

```
Bot                              Arena
 |──── WebSocket connect ────────▶|
 |◀──── welcome ──────────────────|  { type:"welcome", protocolVersion, sessionId, gameType }
 |──── join ─────────────────────▶|  { type:"join", sessionId (echo), token, name, preferredClass? }
 |◀──── assigned ─────────────────|  { type:"assigned", shipId }
 |              ... lobby ...      |
 |◀──── matchStart ───────────────|  (followed immediately by the first tick)
 |◀──── tick (observation) ───────|  per tick
 |──── action (intent) ──────────▶|  per tick, before the deadline
 |◀──── matchEnd ─────────────────|  { type:"matchEnd", results }
```

The `sessionId` is a UUID the Arena issues in `welcome`; the bot echoes it in `join` (connection
challenge) along with its **token** from registration (identity → which team's ship it gets). The
bot learns *everything else* about the world from the observations themselves (each one is
self-describing — see `self.id`, `seed`, `arena`, `maxTicks`).

WASM bots skip the handshake: the Arena instantiates the module, calls `init` with the tick-0
observation, then calls `tick` each tick (see §9).

## 6. Observation (Arena → bot, each tick)

Full observability in v1. The schema is structured so visibility can later be restricted per
entity class; the **always-known** classes are `arena`, `maxTicks`, `seed`, `self`, `anchors`,
and `asteroids`. The **fog-eligible** classes (restricted in a future mode) are `ships`,
`relics`, and `projectiles`.

```json
{
  "type": "tick",
  "tick": 412,
  "maxTicks": 3600,
  "seed": 1234567890,
  "arena": { "width": 2000, "height": 1200 },

  "self": {
    "id": "ship-3", "class": "skiff", "alive": true, "invuln": false,
    "pos": { "x": 812.5, "y": 410.0 }, "vel": { "x": 12.0, "y": -3.5 },
    "heading": 1.57, "angVel": 0.0,
    "hull":   { "cur": 80,  "max": 100 },
    "shield": { "cur": 45,  "max": 60  },
    "aether": { "cur": 60,  "max": 100 },
    "sigil": "Bulwark",            // held Sigil name, or null
    "cannonCooldown": 0,           // ticks until the rune-cannon can fire again
    "relicsCarried": 2
  },

  "anchors": [
    { "shipId": "ship-3", "pos": { "x": 100,  "y": 600 } },
    { "shipId": "ship-7", "pos": { "x": 1900, "y": 600 } }
  ],

  "ships": [                       // OTHER ships only; no aether or sigil (hidden)
    { "id": "ship-7", "class": "skiff", "alive": true, "invuln": false,
      "pos": {…}, "vel": {…}, "heading": 3.0,
      "hull": { "cur": 100, "max": 100 }, "shield": { "cur": 50, "max": 60 },
      "relicsCarried": 0 }
  ],

  "relics":        [ { "id": "relic-12", "pos": {…}, "vel": {…}, "value": 1 } ],
  "asteroids":     [ { "id": "ast-1", "pos": {…}, "vel": {…}, "radius": 60 } ],
  "projectiles":   [ { "id": "proj-88", "pos": {…}, "vel": {…}, "owner": "ship-7" } ],
  "singularities": [ { "id": "sing-2", "pos": {…}, "radius": 150, "ticksLeft": 20 } ],

  "mines": [                       // own mines always shown; enemy mines only within ~120u (balance)
    { "id": "mine-9",  "pos": {…}, "own": true  },
    { "id": "mine-22", "pos": {…}, "own": false }
  ],

  "scores": { "ship-3": 5, "ship-7": 3 },   // v1 is free-for-all (teams later)

  "events": [ … ]                  // things that happened to you last tick — see §7
}
```

Notes:
- `hull`, `shield`, `aether` are `{cur,max}`; `class` is present on every ship — both so new ship
  types slot in with no schema change.
- Enemy ships never expose `aether` or `sigil` (bluff room).
- `invuln` is `true` while a ship has **spawn protection** — a freshly respawned ship is immune
  to damage for a short window (anti-gank), and Bulwark also sets it. Don't waste shots on an
  `invuln` ship.
- Enemy `mines` appear only when your ship is within the detection radius (~120 units — *balance*);
  your own mines are always listed (`own:true`).

## 7. Events (in each observation)

A list of what happened to **you** since the last tick, so reactive bots don't have to diff
state. Indicative set (extensible):

| event           | payload                                  | meaning |
|-----------------|------------------------------------------|---------|
| `tookHull`      | `{ amount, by }`                         | hull damage taken |
| `tookShield`    | `{ amount, by }`                         | shield damage absorbed |
| `shieldDown`    | `{}`                                      | shield reached 0 |
| `relicTaken`    | `{ relicId, value }`                      | you picked up a relic |
| `relicBanked`   | `{ value, total }`                        | you banked relics at your Anchor |
| `sigilGranted`  | `{ sigil }`                               | a relic granted you a Sigil |
| `mineHit`       | `{ mineId, amount }`                       | a mine detonated on you |
| `killedShip`    | `{ shipId }`                              | you destroyed another ship |
| `died`          | `{ by }`                                  | your ship was destroyed |
| `matchOver`     | `{ results }`                             | match ended |

## 8. Action / intent (bot → Arena, each tick)

Rate-first intent, inspired by Robocode. **All fields optional; an omitted field keeps its
previous value.** The Arena applies physics and clamps to limits.

```json
{
  "type": "action",
  "turn":   0.3,                    // -1..1 fraction of max turn rate (+ = CCW)
  "thrust": 1.0,                    // -1..1 fraction of max thrust along heading (+ fwd / - reverse)
  "fire":   true,                   // fire the rune-cannon when cooldown + aether allow (persists)
  "sigil":  true,                   // discharge the held Sigil once (one-shot; then consumed)
  "sigilTarget": { "x": 900, "y": 400 }   // aim point for Sigils that need one (Singularity, Arc Lance)
}
```

- **Cannon is fixed:** the rune-cannon fires along the ship's `heading` — you aim by turning the
  ship. (A separately-aimed turret is a possible later addition.)
- `fire` persists like a held trigger: while `true`, the ship fires whenever the cannon cooldown
  and aether allow.
- `sigil:true` discharges the currently held Sigil **once** and consumes it; it does nothing if
  no Sigil is held. Sigils that need a direction/point read `sigilTarget`:
  - **Afterburner** — none (thrust surge along heading).
  - **Bulwark** — none (self).
  - **Singularity** — `sigilTarget` (where to deploy the gravity well).
  - **Aether Mine** — none (drops at the ship's position).
  - **Arc Lance** — `sigilTarget` (direction of the bolt).

## 9. WASM ABI

A WASM Bot is a **core-wasm** module (ADR-0004). It exports:

- `memory` — its linear memory.
- `alloc(len: i32) -> i32` — the host allocates a buffer in the module and writes JSON into it.
- `init(ptr: i32, len: i32)` — called once before the match with the **tick-0 observation**
  (same shape as §6), so the bot can precompute/allocate.
- `tick(ptr: i32, len: i32) -> i64` — reads the observation JSON at `[ptr,len)`, writes its action
  JSON into its own memory, and returns `(out_ptr << 32) | out_len`.

The instance persists for the whole match (state lives in the module). The only optional host
import is `log(ptr,len)`. Per-tick compute is bounded by wasmtime **fuel**; exhausting it = "no
action this tick". Modules are instantiated and `init`-ed before the match to pay JIT cost up
front. There are no clock/random host imports — a bot derives any randomness from the `seed` and
`tick` in the observation.

## 10. SDKs & starters

- **WASM SDKs (templates hiding the alloc/`tick` glue — author writes `tick(obs) -> action`):**
  Rust, AssemblyScript, TinyGo, C/C++, Zig.
- **WS starters (connect, read JSON, write JSON):** Python, TypeScript on **Deno**, Kotlin (JVM).
- **Kotlin/Wasm:** experimental stretch only; Kotlin users take the WS path in v1.

## 11. Out of scope for this document (decide separately)

- Exact physics constants and gameplay numbers (the balance pass).
- The scoring/`results` formula (how banking relics, kills, and survival combine).
- Match setup: ship counts, `class` roster, relic/asteroid spawning, match length.
- Fog-of-war mode and any turret/teams extensions.
