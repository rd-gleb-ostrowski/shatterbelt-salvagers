# Arena Frontend — Viewer & Admin

Status: ready-for-agent

## Problem Statement

A bot-battle is only exciting if people can *watch* it, and the event only runs if the
facilitator can *control* it. We need two browser apps on top of the Arena Server: a **Viewer**
that makes matches a spectacle on the projector (graphics, sound, a live scoreboard, the ladder)
and supports replays, and an **Admin** console that lets the facilitator manage bots, run and
control matches, and drive the ladder. Without these, the Arena is invisible and unmanageable.

## Solution

Two TypeScript apps served by the Arena Server. The **Viewer** is a read-only client on the
server's observer "god-mode" stream, rendering the Drift and everything in it with **PixiJS**
(WebGL) plus event-driven **Web Audio**, with a HUD (scoreboard, match timer, TrueSkill ladder), a
flexible camera, and a scrubbable **replay** mode. The **Admin** is a plain tables-and-forms app
on the server's password-gated controller API, for bot/team management, match control, ladder
control, replays, and diagnostics. Both are described in `src/arena/FRONTEND.md`.

## User Stories

1. As a spectator, I want to watch the current live match on the projector with ships, relics, asteroids, and Anchors clearly drawn, so that I can follow the action.
2. As a spectator, I want ships shown in team colours with names, headings, thrust flames, and hull/shield bars, so that I can tell who's who and how they're doing.
3. As a spectator, I want a visible shimmer on ships during spawn protection, so that I understand why they aren't taking damage.
4. As a spectator, I want to see Sigil effects — singularity wells, mines, Arc Lance beams — and explosions on death, so that big plays read clearly.
5. As a spectator, I want relics to glow and Anchors to look like team home beacons, so that the economic game is legible.
6. As a spectator, I want a live scoreboard (team, banked score, relics carried) and the match timer, so that I know the state of play.
7. As a spectator, I want the TrueSkill ladder shown between matches, so that I can follow the standings.
8. As a spectator, I want sound for thrust, cannon fire, explosions, Sigil discharges, and relic pickup/bank, so that the spectacle has impact.
9. As a spectator, I want match start/end stingers, so that matches feel like events.
10. As an operator, I want the camera to fit the whole Drift by default, so that nothing happens off-screen.
11. As an operator, I want to follow a chosen ship and to zoom in/out and pan, so that I can highlight a duel.
12. As a spectator, I want to replay a recorded match with play/pause, speed control, and a scrubbable timeline, so that I can re-watch and slow-mo great moments.
13. As a facilitator, I want to sign into the Admin with the facilitator password, so that only I can control the event.
14. As a facilitator, I want to see every connected WS Bot and uploaded WASM Bot with health (connected, last-seen, skipped ticks, crashes, logs), so that I can spot problems.
15. As a facilitator, I want to upload or replace a team's WASM Bot and enable/disable bots, so that I can manage the field.
16. As a facilitator, I want to set or replace the Default Bot, so that empty slots behave well.
17. As a facilitator, I want to start an on-demand match (live or headless, full field, seed, length), so that I can showcase or test.
18. As a facilitator, I want to pause/resume, change speed, and abort a live match, so that I can control the projector.
19. As a facilitator, I want to push a match to the projector Viewer, so that the audience sees what I choose.
20. As a facilitator, I want to start/pause the headless ladder runner and view/reset TrueSkill standings, so that I manage ranking.
21. As a facilitator, I want to list, replay, and download recorded matches, so that I can re-show and archive them.
22. As a facilitator, I want to kick or disqualify a misbehaving bot, so that it can't disrupt matches.

## Implementation Decisions

- **Viewer renderer:** **PixiJS (WebGL)** for smooth glow/particles/effects on a projector;
  Canvas2D is an acceptable simpler fallback.
- **Viewer data:** the server's **observer god-mode** WS stream (sees everything, including
  otherwise-hidden mines); render ships (incl. `invuln` shimmer), relics, Anchors, asteroids,
  rune-cannon bolts, singularities, mines, Arc Lance beams, explosions. HUD = scoreboard + 2-min
  timer + TrueSkill ladder panel between matches.
- **Camera:** fit-the-Drift default (the Drift scales with field size), plus follow-a-ship, and
  zoom in/out + pan.
- **Sound:** Web Audio, driven by the stream's `events` (thrust, cannon, explosions, sigils,
  relic pickup/bank, match start/end). Sound is a first-class part of the spectacle.
- **Replay:** load a recorded match (seed + action log via the server) and offer play/pause,
  speed, and a scrubbable timeline.
- **Admin:** plain TypeScript with DOM tables/forms on the server's controller HTTP/WS + REST,
  gated by the facilitator password; covers bot/team management + WASM upload, match control
  (live/headless, pause/speed/abort), ladder control, replay management, and diagnostics/kick.
- Both apps are served by the Arena Server. (See `src/arena/FRONTEND.md`.)

## Testing Decisions

- This layer is primarily verified **manually/visually** on a real match — automated UI tests
  have low value for the spectacle itself.
- Where logic is pure and worth testing, write **small unit tests** for: camera transforms
  (world↔screen, fit/zoom/pan), the replay timeline/scrub mapping (tick↔position, speed), HUD
  formatting (scores/timer), and the event→sound mapping. Test these as plain functions, not via
  the DOM.
- No prior art in-repo (new apps); keep testable logic in framework-free modules.

## Out of Scope

The Arena Server and protocol (separate PRD), the simulation engine, the bot SDKs, and rendering
performance tuning beyond "smooth on a projector".

## Further Notes

Aesthetic is the sci-fi/fantasy blend — aether glow, runic accents. The Viewer should always have
the always-live exhibition match to show (ADR-0005), so it is never blank.
