# Arena Engine — Shatterbelt Salvagers

Status: ready-for-agent

## Problem Statement

We need the deterministic, headless heart of the **Arena** game *Shatterbelt Salvagers*: the
thing that actually simulates salvage ships drifting, fighting, and banking relics in the Drift.
Everything else — the network server, the Viewer, the balance tooling — depends on a simulation
that is correct, reproducible, and free of any networking or I/O concerns. Without this core as a
clean, separately-testable unit, rules and physics get tangled with transport and rendering, and
match outcomes become impossible to reason about or replay.

## Solution

A pure Rust **engine** that owns all game rules and physics for one match. It is constructed from
a seed, the balance parameters, and a field of ships; it advances one **tick** at a time by
taking each ship's **intent** and producing the next world state plus a list of per-ship
**events**; and it can produce the per-ship **observation** that a bot would see. It is
deterministic — the same seed and the same sequence of applied intents reproduce a match exactly
— and it records the applied intents so a match can be replayed. It has no awareness of WebSocket,
WASM, HTTP, the Viewer, the ladder, or auth.

## User Stories

1. As the Arena Server, I want to create an engine from a seed, a parameter set, and a list of ships, so that I can run a match with a known starting state.
2. As the Arena Server, I want to advance the engine one tick by supplying each ship's intent, so that the simulation progresses under server-controlled pacing.
3. As the Arena Server, I want each tick to return the per-ship events that occurred, so that I can forward them to bots and the Viewer.
4. As the Arena Server, I want to ask the engine for a given ship's observation, so that I can serialise and send it to that bot.
5. As the Arena Server, I want a separate "god-mode" view of the whole world, so that I can feed the Viewer the full state.
6. As a balance designer, I want the engine to be deterministic given a seed and applied intents, so that golden scenarios are stable and replays are exact.
7. As a balance designer, I want all gameplay numbers to come from a parameter set, so that I can tune the game without changing engine logic.
8. As a ship's bot, I want continuous 2D drift movement (thrust along heading, light damping, a max-speed cap) so that piloting feels like space but is controllable.
9. As a ship's bot, I want rate-first control (turn rate, thrust fraction) with the engine applying physics and clamps, so that I never set absolute state.
10. As a ship's bot, I want thrust to cost aether and aether to regenerate, so that moving and fighting trade against each other.
11. As a ship's bot, I want to fire a fixed rune-cannon along my heading subject to a cooldown and an aether cost, so that shooting is a managed resource.
12. As a ship, I want a Shield that absorbs damage before my Hull and regenerates after a delay without being hit, so that brief disengagements let me recover.
13. As a ship, I want to be destroyed when my Hull reaches zero, drop the relics I was carrying, and respawn at my Anchor after a delay, so that combat sets me back without eliminating me.
14. As a freshly respawned ship, I want a spawn-protection window of invulnerability, so that I can't be spawn-camped.
15. As a salvage ship, I want to pick up relics up to a carry cap and bank them at my Anchor for score, so that the economic game works.
16. As a salvage ship, I want picking up a relic to grant me a single random Sigil if I hold none, so that I gain occasional powers.
17. As a ship, I want to discharge my held Sigil for its effect — Afterburner, Bulwark, Singularity, Aether Mine, or Arc Lance — so that I have Mario-Kart-style burst options.
18. As a ship, I want collisions with asteroids, other ships, and the arena walls to bounce me and deal damage scaled to impact speed, so that the Drift is a real hazard.
19. As a competitor, I want a kill to award a direct score bounty, so that denial is worth pursuing.
20. As a match, I want to end after a fixed number of ticks with the highest banked-plus-bounty score winning, so that matches are time-boxed and decisive.
21. As a match runner, I want the Drift size and entity counts to scale with the number of ships at constant density, so that 2-ship and 8-ship matches both feel right.
22. As the Arena Server, I want the engine to record the applied intent per ship per tick, so that I can persist and replay the match deterministically from the seed.
23. As a balance designer, I want to run many headless matches with scripted/stub bots and read aggregate stats, so that I can validate balance from evidence.

## Implementation Decisions

- **Physics:** continuous 2D using `rapier2d` (ADR-0002), with light linear damping and a
  max-speed cap; turn is rate-controlled, thrust is force/momentum. Determinism uses rapier's
  deterministic mode.
- **Tick model:** the engine exposes a pure `step(intents) -> events` plus observation accessors.
  Real-time pacing, deadlines, and the 30 Hz rate are the Server's concern (ADR-0003), not the
  engine's. The engine is just the stepper.
- **Observation & Intent shapes:** exactly as defined in `src/arena/PROTOCOL.md` (§6 Observation,
  §8 Action). Full observability in v1, but structured so visibility can later be restricted per
  entity class (always-known: arena/seed/maxTicks/self/anchors/asteroids; fog-eligible: enemy
  ships/relics/projectiles). Enemy `aether`/`sigil` hidden; enemy mines proximity-visible; an
  `invuln` flag on ships. Intents are rate-first with per-field persistence.
- **Sigils:** Afterburner (timed thrust/speed boost, free aether), Bulwark (shield refill +
  timed immunity), Singularity (deployed gravity well pulling ships and relics), Aether Mine
  (armed proximity detonation), Arc Lance (fast, piercing, shield-bypassing bolt). One held at a
  time, granted on relic pickup, consumed on use.
- **Scoring:** economic + respawn. Score = banked relic value + a per-kill bounty (no extra relic
  drop). Time-boxed match (default 3600 ticks); highest score wins (ADR-0005).
- **Dynamic Drift:** arena dimensions scale by `√(N/4)` off the 2000×1200 baseline (area ∝ N),
  with asteroids/relics scaling with N.
- **Parameters:** all gameplay numbers live in a parameter set seeded from the first-pass values
  in `src/arena/balance/BALANCE.md` / `params.py`; they are placeholders pending playtests.
- **Replay:** the engine records applied intents per tick; combined with the seed and rapier
  determinism, this reproduces a match exactly. TrueSkill and persistence are *not* in the engine.

## Testing Decisions

- Good tests assert **external behaviour** at the engine boundary — given a seed, parameters, and
  a scripted sequence of intents, the resulting world state / events / scores match expectations
  — never internal physics representations.
- **Golden scenarios** (port the Python harness's checks): a thrusting ship reaches the expected
  speed/position envelope; N cannon hits destroy a ship (TTK); a relic picked up and carried to
  the Anchor scores; a destroyed ship drops its relics and respawns invulnerable; spawn-protection
  blocks damage for its window; each Sigil produces its effect (Afterburner speed-up, Bulwark
  immunity, Singularity pull, Mine detonation, Arc Lance pierce + shield bypass); asteroid/wall
  collisions damage by impact speed; a full match is decisive with no shutout.
- **Determinism test:** the same seed + applied-intent log produces an identical match (state and
  result) across runs.
- Prior art: `src/arena/balance/harness.py` already encodes these scenarios and their expected
  magnitudes; mirror them as Rust tests.

## Out of Scope

WebSocket/REST networking, the wasmtime WASM host, the Default Bot wiring, authentication, the
TrueSkill ladder, persistence/storage, the Viewer and Admin, and the bot SDKs. Final balance
numbers (these are first-pass and explicitly tunable).

## Further Notes

The engine is the foundation the Server (separate PRD) wraps. Keep the Observation/Intent types
authoritative against `PROTOCOL.md`; if they drift, update the protocol doc in the same change.
