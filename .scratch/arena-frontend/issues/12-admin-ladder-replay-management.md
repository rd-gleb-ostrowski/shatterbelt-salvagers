# Admin ladder & replay management

Status: ready-for-agent
Type: AFK
User stories: 20, 21

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Let the facilitator drive the ranking and the recorded-match archive from the Admin.

- **Ladder control**: start/pause the headless ladder runner and view/reset TrueSkill standings.
- **Replay management**: list, replay, and download recorded matches.

Builds on the gated Admin shell and the server's ladder + replay endpoints.

## Acceptance criteria

- [ ] The headless ladder runner can be started and paused from the Admin.
- [ ] TrueSkill standings can be viewed and reset.
- [ ] Recorded matches can be listed, replayed, and downloaded.
- [ ] All actions go through the password-gated controller API.

## Blocked by

- `08-admin-auth-health-dashboard.md`
