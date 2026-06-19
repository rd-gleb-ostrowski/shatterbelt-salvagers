# Admin controller API & match control

Status: ready-for-agent
Type: AFK
User stories: 10, 11, 12, 22 (facilitator password)

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The facilitator's control surface over the running Arena, gated by a separate facilitator password
(distinct from the event password; PROTOCOL §4). All control endpoints reject an unauthorised
caller.

- **Start an on-demand match** (live or headless) with a chosen full field, seed, and length.
- **Live match control**: pause / resume, change speed (TPS), and abort a running live match.
- **Always-live exhibition match**: the server keeps one live match running at all times so the
  projector is never blank — when one ends (or is aborted), another starts automatically.

This ties together the live loop, resolver, observer stream, and headless runner under
facilitator control.

## Acceptance criteria

- [ ] All control endpoints are gated by the facilitator password and reject unauthorised callers.
- [ ] A facilitator can start an on-demand match specifying live/headless, field, seed, and length.
- [ ] A running live match can be paused, resumed, retimed (TPS), and aborted.
- [ ] An exhibition live match is always running; when it ends or is aborted another starts so the
      projector is never blank.
- [ ] Control behaviour is tested as observable API behaviour (authorised vs rejected, resulting
      match state).

## Blocked by

- `06-connection-resolver.md`
- `07-observer-god-mode-stream.md`
- `09-headless-fast-runner.md`
