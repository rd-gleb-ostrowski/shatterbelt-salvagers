# Observer god-mode stream

Status: ready-for-agent
Type: AFK
User stories: 16

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The full-fidelity feed for the Viewer (projector / recording), distinct from what bots receive.

- An **observer "god-mode"** stream of the current match: the full world each tick (the engine's
  god view), so the Viewer can render everything.
- Bots **never** receive this stream — it bypasses the per-bot fog/visibility rules that the WS and
  WASM observation paths enforce.
- The stream follows the current live match.

This is the server exposing the engine's god-mode view over the wire; it does not yet include
recording (separate slice) or the Admin controller (separate slice).

## Acceptance criteria

- [ ] A consumer can subscribe to a god-mode observer stream of the current match and receive the
      full per-tick world state.
- [ ] The observer stream contains information hidden from bots (e.g. all enemy aether/sigils, all
      mines), confirming it is not fog-filtered.
- [ ] No bot (WS or WASM) can receive the observer stream.
- [ ] The stream tracks the currently running match tick-by-tick.
- [ ] Observer output is tested as observable wire behaviour against a running match.

## Blocked by

- `01-server-skeleton-live-loop.md`
