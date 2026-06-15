# Arena

The custom bot-battle game built for the retreat — **"Shatterbelt Salvagers"**: a centrally-hosted
server that runs **matches** in which participants' **bots** pilot **salvage ships** through a
debris belt, scavenging **relics** and fighting over them, shown on a big screen.

## Language

**Arena**:
The hosted server that owns all game rules, runs matches, and is the single source of truth
for game state. The only centrally-hosted component — everything a participant writes runs
on their own machine or is uploaded to the Arena.
_Avoid_: server, engine, game host

**Bot**:
A participant's program that pilots one salvage ship in a match. A bot only decides actions;
it never enforces rules.
_Avoid_: agent (reserved for the LLM coding agent), player, AI

**Salvage Ship** (often **Ship**):
The in-world avatar a bot controls: a thruster-driven, rune-etched hull. Distinct from the
Bot, which is the controlling program.
_Avoid_: drone, vehicle, unit

**Drift**:
The playfield — the debris belt of a shattered world, strewn with asteroids and pooled aether.
_Avoid_: map, board, field, arena (Arena is the server)

**Aether**:
The single resource that powers both thrust and arcane systems, so spending to move trades
against spending to fight. Each ship carries a regenerating reserve.
_Avoid_: energy, fuel, mana

**Relic**:
Salvage collected from wreckage in the Drift. Must be banked at the ship's Anchor to score;
collecting a relic also grants a single-use Sigil.
_Avoid_: loot, pickup, item, resource

**Anchor**:
A ship's home beacon where it banks relics to score.
_Avoid_: base, home, goal

**Asteroid**:
A drifting chunk of debris — both collision hazard and cover.
_Avoid_: rock, obstacle, wall

**Hull**:
A ship's core health. Damage hits the Shield first, then the Hull; a depleted Hull destroys
the ship.
_Avoid_: health, HP, armor

**Shield**:
A regenerating buffer that absorbs damage before the Hull.
_Avoid_: armor, barrier

**Rune-cannon**:
A ship's standard ranged weapon, firing arcane projectiles. Costs aether to fire.
_Avoid_: gun, blaster, laser

**Sigil**:
A single-use arcane ability obtained by collecting a Relic (Mario-Kart-item style) and
discharged once. Each ship carries at most one at a time. The five Sigils are:

- **Afterburner** — a sustained directional thrust surge (mobility; keeps inertia, must be steered).
- **Bulwark** — instantly overcharges the Shield and grants a few ticks of damage immunity (defense).
- **Singularity** — deploys a short-lived gravity well at a point that draws loose Relics and enemy ships toward it (control).
- **Aether Mine** — drops a near-invisible proximity mine that detonates on enemy contact (trap).
- **Arc Lance** — fires one fast, piercing bolt that punches through Shields and can hit ships in a line (offense).

_Avoid_: spell, power-up, item, ability

**Match**:
A single scored game between two or more bots in the Arena.
_Avoid_: game, round, battle

**Tick**:
One discrete step of a match. Each tick the Arena sends every bot the current state and
collects one action back.
_Avoid_: turn, frame, step

**Observation**:
The per-tick JSON snapshot of the Drift the Arena sends a bot (its own ship in full, plus the
world it can see). The same shape for WS and WASM bots. See `PROTOCOL.md`.
_Avoid_: state, world, snapshot

**Intent** (also **Action**):
The per-tick command a bot returns — rate-first (turn, thrust, fire, sigil), not absolute state.
Omitted fields keep their previous value.
_Avoid_: command, move, order

**Viewer**:
A read-only client that receives the full "god-mode" stream of a match for the projector and
for recording replays. Bots never receive it.
_Avoid_: observer, spectator, renderer

**WS Bot**:
A bot that runs as a process on a participant's machine and connects _outbound_ over
WebSocket to the Arena. The preferred connection: the bot initiates, so it works from behind
NAT/firewalls and needs no inbound hosting.
_Avoid_: socket bot, live bot

**WASM Bot**:
A bot uploaded to the Arena as a compiled WebAssembly artifact and executed in-process,
sandboxed. Needs no running process or network and is the unit handed over for the
brownfield swap.
_Avoid_: uploaded bot, module bot

**Default Bot**:
The Arena's built-in bot used to fill any slot with no WS Bot or WASM Bot, so every match
slot always plays.
_Avoid_: dummy, stub, house bot
