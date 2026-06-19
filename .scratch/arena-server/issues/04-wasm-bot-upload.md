# WASM Bot upload (REST + storage)

Status: ready-for-agent
Type: AFK
User stories: 2, 3

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The upload half of the WASM path: accept a compiled **WASM Bot** artifact and store it against a
team, so a participant doesn't need to keep a process alive. Execution is the next slice.

- `POST /bots` accepting a compiled `.wasm` artifact, authorised by the participant's token.
- **Re-upload replaces** the team's current WASM Bot so participants can iterate during the day.
- The artifact is stored/addressable per team so the wasmtime host (next slice) and the connection
  resolver can pick it up.
- Upload without a valid token is rejected.

This slice does not instantiate or run the WASM — it only ingests and stores it.

## Acceptance criteria

- [ ] `POST /bots` with a valid token stores the uploaded `.wasm` artifact for that team.
- [ ] A second upload for the same team replaces the previous artifact.
- [ ] Upload with a bad/absent token is rejected.
- [ ] The stored artifact is retrievable per team for later instantiation.
- [ ] Upload/replacement behaviour is tested over the wire (or via stubbed transport).

## Blocked by

- `01-server-skeleton-live-loop.md`
- `02-registration-tokens.md`
