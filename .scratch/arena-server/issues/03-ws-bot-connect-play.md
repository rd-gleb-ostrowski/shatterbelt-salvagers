# WS Bot connect & play

Status: ready-for-agent
Type: AFK
User stories: 4, 5, 6

## Parent

`.scratch/arena-server/PRD.md`

## What to build

Let a participant's locally-running **WS Bot** connect outbound and play a live match as its team,
end-to-end over the wire per `src/arena/PROTOCOL.md`:

- The handshake: `welcome` → `join`-with-token → `assigned` → `matchStart` → per-tick
  `tick`/`action` → `matchEnd`.
- Each tick the bot receives its **private observation** (fog-respecting: enemy aether/sigil hidden,
  enemy mines proximity-only, `invuln` exposed) and has until the per-tick **deadline** to return an
  intent.
- A missed deadline persists the bot's **previous intent** so transient lag doesn't freeze the ship.
- The join token (from registration) resolves the connection to the correct team's ship; a bad or
  absent token is rejected.

WS Bots are restricted to **live** matches (the headless restriction is enforced in the headless
slice). The wire types stay authoritative against `PROTOCOL.md`.

## Acceptance criteria

- [ ] A WS Bot connecting outbound completes welcome → join(token) → assigned and plays a live
      match through matchStart … tick/action … matchEnd.
- [ ] A join with a valid token is assigned to that team's ship; a bad/absent token is rejected.
- [ ] Each tick the bot receives only its private, fog-respecting observation.
- [ ] An intent received before the deadline is applied; a missed deadline persists the previous
      intent.
- [ ] Observation/action JSON encode↔decode and per-field intent persistence are tested against the
      engine via a stub transport.

## Blocked by

- `01-server-skeleton-live-loop.md`
- `02-registration-tokens.md`
