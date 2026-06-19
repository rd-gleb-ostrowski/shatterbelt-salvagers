# Bot health & moderation

Status: ready-for-agent
Type: AFK
User stories: 19, 20

## Parent

`.scratch/arena-server/PRD.md`

## What to build

Give the facilitator visibility into each bot and the power to remove bad actors.

- **Health per bot**: connected/last-seen, skipped ticks (missed deadlines / fuel exhaustions),
  crashes, and captured **log output** (including the WASM `log` import).
- **Moderation**: kick or disqualify a misbehaving bot so it can't disrupt matches; a kicked/DQ'd
  bot is removed from current and future matches (its slot falls back via the resolver).
- All of this is exposed through the facilitator-gated controller surface.

## Acceptance criteria

- [ ] The server tracks and exposes per-bot health: connected state, last-seen, skipped ticks,
      crashes, and log output.
- [ ] A WASM bot's `log` output and a crash/fuel-exhaustion are reflected in its health.
- [ ] A facilitator can kick or disqualify a bot; it is removed from the current match and excluded
      from future matches.
- [ ] A kicked/DQ'd team's slot is refilled per the connection resolver (WASM/Default).
- [ ] Health/moderation endpoints are facilitator-gated and tested as observable behaviour.

## Blocked by

- `03-ws-bot-connect-play.md`
- `05-wasmtime-host.md`
- `11-admin-controller-match-control.md`
