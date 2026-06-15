# Match execution: live / headless-fast / replay, all-bots FFA, TrueSkill ladder

One deterministic Arena engine is driven three ways, and bots are ranked by a continuously
updated **TrueSkill** ladder rather than a scheduled tournament.

## Decisions

- **Three run modes off one engine:**
  - **Live** — paced to wall-clock (~30 Hz) for the projector Viewer. Both WS Bots and WASM Bots
    play (WS Bots *require* live — they're network-bound).
  - **Headless-fast** — unpaced, as fast as the CPU allows, for building the ladder in the
    background. **WASM + Default Bots only** (a networked WS Bot can't keep up with an uncapped
    tick rate). Emits results **and** a recorded replay (action log + seed).
  - **Replay** — play any recorded match back in the Viewer at any speed.
- **All bots in every match.** With ≤6–8 teams expected, each match is the **full field as one
  free-for-all** — no pods or pairings. Every match yields a complete ranking.
- **Ladder, not tournament.** Headless-fast matches run continuously and feed a **TrueSkill**
  rating (it natively rates FFA: rank all bots by score each match → one rating update, with
  uncertainty, converging fast with few players). Plus **on-demand** live matches.
- **Always a live match.** The Arena always keeps **one live (paced) exhibition match** running
  for the projector Viewer, alongside the fast headless ladder matches — so the screen is never
  blank. On-demand matches simply take over the live slot; between them, exhibition matches loop.
  Both live and headless matches feed the ladder.
- **Canonical match length: 2 minutes (3600 ticks @ 30 Hz)** for both live and ladder, so ratings
  compare like-with-like. (Robocode uses elimination rounds; that doesn't fit our economic +
  respawn model, so a fixed time limit is the natural choice.) Headless, a match runs in
  milliseconds, so length never bottlenecks rating convergence.
- **Dynamic Drift size (constant density).** The arena and entity counts scale with the field so
  2 ships aren't lost in a void and 8 aren't shoulder-to-shoulder: each dimension scales by
  `√(N/4)` off the 2000×1200 baseline (area ∝ N), with asteroids and relics scaling with N. The
  balance harness confirms no shutouts and comparable scores across 2/4/8-ship fields.

## Why

- WS = live-only and WASM = headless-fast is exactly why the WASM path exists: run a whole
  ladder's worth of matches in seconds, then re-show the best ones live.
- TrueSkill suits a small, all-plays-all field far better than a bracket, and updates live as
  background matches complete.

## Considered alternatives

- **Scheduled tournament / round-robin pods** — unnecessary with ≤8 teams all in one FFA; a
  rolling ladder is simpler and always current.
- **Average-score ranking** — TrueSkill handles per-match rankings and uncertainty better.
- **Elimination rounds (Robocode-style)** — incompatible with economic + respawn scoring.
- **Fixed arena size** — leaves small fields empty and large fields cramped.
