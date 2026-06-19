# Registration & tokens (REST auth)

Status: ready-for-agent
Type: AFK
User stories: 1, 22 (event password)

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The participant-facing entry point: turn the pre-shared **event password** into a per-team
**token** that later identifies a team's ship (PROTOCOL §4). No accounts.

- `POST /register` accepting the shared event password (and team identity) returns a token.
- A bad or absent password is rejected.
- The token maps an identity to a team's ship for later use by WS join and WASM upload.

This slice is just the registration seam; consuming the token to join/upload is handled in the WS
and WASM slices.

## Acceptance criteria

- [ ] `POST /register` with the correct event password returns a token bound to the team identity.
- [ ] `POST /register` with a wrong or missing password is rejected.
- [ ] The issued token is in a form the WS-join and WASM-upload paths can validate to resolve a
      team's ship.
- [ ] Registration behaviour is tested over the wire (or via stubbed transport), asserting the
      observable response.

## Blocked by

- `01-server-skeleton-live-loop.md`
