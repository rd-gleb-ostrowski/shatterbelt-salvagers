# Arena per-tick protocol: real-time JSON intents over one shared contract

The Arena runs matches as a **real-time loop at ~30 ticks/second**. Each tick it sends every bot
a JSON **observation** of the world and collects back a JSON **action (intent)**; a bot that
misses the per-tick deadline keeps its previous intent. Both connection types (WS Bot, WASM Bot)
speak the **same JSON observation/action schema**, so a bot's decision logic ports between them
unchanged. The full contract is specified in [`src/arena/PROTOCOL.md`](../../src/arena/PROTOCOL.md).

Key decisions:

- **Real-time, deadline-based (not lockstep).** A fixed ~30 Hz rate gives a smooth spectacle and
  bounds every bot's latency/compute; a hung bot can't freeze the arena.
- **Replay from recorded applied actions + the match seed**, reproduced by rapier2d's
  deterministic mode — so bots themselves need not be deterministic for replays to work.
- **Intent-based, rate-first control** (inspired by Robocode Tank Royale): bots send rates/targets
  (turn, thrust, fire, sigil), never absolute state; the server applies physics and clamps.
  Omitted intent fields persist.
- **JSON** wire format (one schema, language-agnostic, inspectable).
- **Full observability in v1, but the schema is built so fog can be added per entity class later**
  (Anchors/asteroids/self/match-meta stay always-known; enemy ships/projectiles/relics are the
  fog-eligible classes). Enemy `aether` and held `sigil` are hidden even in v1; enemy mines are
  only visible within a short detection radius.
- **An `events` array each tick** (e.g. `tookHull`, `relicBanked`, `mineHit`) so reactive bots are
  easy to write.
- **Observer/viewer "god-mode" tick** carries the full world for the browser viewer and replay;
  bots get only their private (fog-respecting) slice.

## Considered alternatives

- **Lockstep** — fairer per-tick but a slow bot stalls the match and it can't pace to wall-clock
  for the projector.
- **Binary encoding (FlatBuffers/MessagePack)** — compact, but adds per-language codegen and raises
  the barrier for arbitrary-language WS bots; unnecessary at our scale.
- **Absolute control ("go to x,y / aim at θ")** — rejected; rate-based intents keep the server the
  sole authority over physics.
- **Fog of war in v1** — deferred for a cleaner contract and an easier bot-writing on-ramp.
