# Arena Frontend — Viewer & Admin

The Arena has **three kinds of client**: **Bots** (WS/WASM, see `PROTOCOL.md`), the **Viewer**
(read-only spectator), and the **Admin** (the facilitator's controller). The Arena server
(Rust/axum) serves the Viewer and Admin static apps and their streams.

## The Viewer (projector, replay, ladder)

A read-only browser app on the Arena's **observer "god-mode"** WS stream (it sees everything,
including otherwise-hidden mines — bots never get this stream).

- **Renderer:** **PixiJS (WebGL)** — the theme wants glow, thrust trails, explosions, singularity
  warps, and aether shimmer; WebGL keeps that smooth on a projector. (Canvas2D is the simpler
  fallback.)
- **Renders:** the Drift bounds; asteroids; **ships** in team colours with name, heading, a
  thrust flame, hull/shield bars, and an **invuln shimmer** during spawn protection; **relics**
  glowing; **Anchors** as team-coloured home beacons; rune-cannon bolts; and god-mode effects —
  singularity wells, mines, Arc Lance beams, and explosions on death/respawn.
- **HUD:** a live scoreboard (team · banked score · relics carried), the 2-minute match timer,
  and a **TrueSkill ladder** panel shown between matches.
- **Camera:** **fit-the-Drift** by default (it scales with field size), plus a **follow-a-ship**
  cam and **zoom in/out + pan**.
- **Sound (Web Audio, driven by the stream's `events`):** thrust hum, cannon fire, explosions,
  sigil discharges, relic pickup/bank chimes, and match start/end stingers. Sound is a
  first-class part of the spectacle, not an afterthought.
- **Replay mode:** load a recorded match (action log + seed) and get play/pause, speed control,
  and a scrubbable timeline.
- **Always something to show:** the Arena always keeps a live exhibition match running, so the
  Viewer is never blank (see ADR-0005).

## The Admin (the controller role)

A plain web app (tables/forms — no PixiJS), gated by the **facilitator password**, talking to the
Arena's controller HTTP/WS.

- **Bots & teams:** list connected WS bots and uploaded WASM bots; upload/replace a team's WASM
  bot; enable/disable a bot; set/replace the **Default Bot**; per-bot health (connected,
  last-seen, skipped ticks, crashes, and `log` output).
- **Matches:** start an **on-demand** match (choose **live** or **headless-fast**, participants
  default to the full field, set seed + length); control a running **live** match —
  **pause/resume, change speed (TPS), abort**; push a match to the projector Viewer.
- **Ladder:** start/pause the background **headless-fast** runner; view **TrueSkill** standings;
  reset ratings; set match cadence.
- **Replays:** list recorded matches; replay one in the Viewer; download a replay.
- **Diagnostics:** server status, per-bot status, **kick/disqualify** a misbehaving bot, view logs.

## Auth (pre-shared passwords — deliberately simple)

- A shared **event password** → `POST /register` (with a team name) → a **token**. The token
  authorizes uploading a WASM bot (`POST /bots`) or connecting a WS bot (token in the `join`
  message). See `PROTOCOL.md` §4.
- A separate **facilitator password** gates the Admin page and all control endpoints.
- No accounts, no OAuth.

## Tech summary

- **Server:** Rust/axum serves the static Viewer + Admin assets, the observer WS (Viewer), the
  controller WS/HTTP (Admin), the bot WS, and the REST endpoints (`/register`, `/bots`).
- **Viewer:** TypeScript + PixiJS.
- **Admin:** TypeScript with plain DOM/tables.
