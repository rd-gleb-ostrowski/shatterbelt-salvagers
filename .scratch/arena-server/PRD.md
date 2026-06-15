# Arena Server — Shatterbelt Salvagers

Status: ready-for-agent

## Problem Statement

Participants need to get their bots into matches — either by connecting a live **WS Bot** or
uploading a **WASM Bot** — and facilitators need to run the event: register teams, control
matches, keep a live match on the projector at all times, and maintain a ranking. The Arena
Engine simulates a match but knows nothing about networks, uploads, auth, replays, or ranking.
We need the server layer that turns the engine into a hosted, multi-bot, always-on Arena.

## Solution

A Rust server (axum) that wraps the Arena Engine and exposes it to the outside world: REST
**registration** (pre-shared event password → token) and **WASM upload**; outbound-WS **bot**
connections; an in-process **wasmtime** host that runs uploaded WASM Bots against the same JSON
contract; a built-in **Default Bot** to fill empty slots; **recording and replay** of matches; a
continuous **TrueSkill ladder** fed by headless-fast matches; an **always-live exhibition match**
for the Viewer; and an **observer** stream (god-mode) plus a password-gated **controller** API for
the Admin. It owns timing (live ~30 Hz with deadlines; headless uncapped; replay playback) per
ADR-0003 / ADR-0005.

## User Stories

1. As a participant, I want to register my team with the shared event password and receive a token, so that I can submit a bot.
2. As a participant, I want to upload a compiled WASM Bot with my token, so that the Arena runs my bot without me keeping a process alive.
3. As a participant, I want to re-upload to replace my WASM Bot, so that I can iterate during the day.
4. As a participant, I want to connect a WS Bot by including my token in the join handshake, so that my locally-running bot joins as my team.
5. As a WS Bot, I want to receive my observation each tick and have until a deadline to send an intent, so that I play in real time.
6. As a WS Bot, I want my previous intent to persist if I miss a tick's deadline, so that transient lag doesn't freeze my ship.
7. As a WASM Bot, I want the server to instantiate me, call `init` with the tick-0 observation, then call `tick` each tick within a fuel budget, so that I play headless and fast.
8. As a slot with no submitted bot, I want the Default Bot to play me, so that every match is full.
9. As the connection resolver, I want priority WS → WASM → Default per team, so that a live connection supersedes an upload which supersedes the fallback (ADR-0001).
10. As a facilitator, I want an always-running live exhibition match, so that the projector is never blank.
11. As a facilitator, I want to start an on-demand match (live or headless, full field, seed, length) from the Admin, so that I can showcase or test.
12. As a facilitator, I want to pause/resume, change speed (TPS), and abort a live match, so that I can control the spectacle.
13. As a facilitator, I want headless-fast matches to run continuously in the background, so that the ladder churns quickly.
14. As the ladder, I want each finished match's full ranking fed into TrueSkill, so that ratings update with uncertainty and converge fast.
15. As a facilitator, I want to view and reset the TrueSkill standings, so that I can manage the ladder.
16. As the Viewer, I want an observer "god-mode" stream of the current match, so that I can render everything; bots must never receive it.
17. As the server, I want to record applied intents and the seed for every match, so that any match can be replayed deterministically.
18. As a facilitator, I want to list and replay recorded matches in the Viewer, so that I can re-show great moments.
19. As a facilitator, I want to see each bot's health (connected, last-seen, skipped ticks, crashes, log output), so that I can spot problems.
20. As a facilitator, I want to kick or disqualify a misbehaving bot, so that it can't disrupt matches.
21. As a WASM Bot author, I want a `log` host import surfaced to the Admin, so that I can debug.
22. As the server, I want all control endpoints gated by the facilitator password, so that only the facilitator can run the event.
23. As the server, I want WS Bots restricted to live matches and WASM/Default Bots usable in headless-fast matches, so that uncapped runs aren't bottlenecked by the network.

## Implementation Decisions

- **Transport & wire format:** WebSocket + JSON exactly per `src/arena/PROTOCOL.md` (welcome →
  join-with-token → assigned → matchStart → per-tick tick/action → matchEnd). REST `POST
  /register` and `POST /bots`. One schema shared by WS and WASM paths.
- **Timing (ADR-0003/0005):** live matches paced to ~30 Hz with a per-tick deadline; missed
  deadline ⇒ previous intent persists; headless-fast runs uncapped; replay plays a recorded log.
  Both live and headless matches feed the ladder; one live exhibition match always runs.
- **WASM host (ADR-0004):** wasmtime, core-wasm ABI (`memory`/`alloc`/`init(tick-0 obs)`/`tick`
  returning packed ptr/len), instance persists per match, per-tick **fuel** budget (exceed ⇒ "no
  action this tick"), warm-up (`init`) before the match. Optional `log` import.
- **Observability split:** bots receive only their private (fog-respecting) observation; the
  Viewer gets the god-mode observer tick. Enemy aether/sigil hidden; enemy mines proximity-only;
  `invuln` flag exposed.
- **Auth:** pre-shared event password → token (identity → team's ship); separate facilitator
  password gates the controller API and Admin. No accounts. (PROTOCOL §4.)
- **Ladder:** TrueSkill over per-match FFA rankings; standings exposed to Admin and Viewer.
- **Replay & persistence:** store seed + applied-intent log per match; replay reconstructs via the
  engine (deterministic).

## Testing Decisions

- Good tests assert **observable protocol/behaviour**, not internals: given inputs over the wire
  (or stubbed transport) the server produces the right messages, assignments, and match outcomes.
- **Protocol seam:** observation/action JSON encode↔decode, per-field intent persistence, and the
  per-tick deadline behaviour, exercised against the engine through a stub transport.
- **Registration/WS seam:** `POST /register` with the event password returns a token; a WS `join`
  carrying that token reaches `assigned` and plays; a bad/absent token/password is rejected.
- **WASM-bot seam:** load a tiny known `.wasm` bot, run a (headless) match, and assert it acts —
  proving the ABI (`init`/`tick`/`alloc`, fuel limiting, warm-up).
- **Ladder seam:** feeding a known match ranking produces the expected TrueSkill rating movement.
- **Replay seam:** a recorded match replays to an identical result.
- Prior art: the Arena Engine's golden scenarios (separate PRD) underpin these.

## Out of Scope

The simulation rules/physics themselves (Arena Engine PRD), the Viewer and Admin UIs (Frontend
PRD), the participant-facing SDK templates (SDKs PRD), and final balance numbers.

## Further Notes

WS = live-only, WASM = headless-capable is exactly why the WASM path exists: run a whole ladder's
worth of matches in seconds, then re-show the best ones live. Keep the wire types authoritative
against `PROTOCOL.md`.
