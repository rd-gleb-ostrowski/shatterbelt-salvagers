# Admin match control & push-to-projector

Status: ready-for-agent
Type: AFK
User stories: 17, 18, 19

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

The facilitator's live match controls in the Admin.

- **Start an on-demand match**: live or headless, full field, chosen seed and length.
- **Control a live match**: pause/resume, change speed, and abort.
- **Push a match to the projector Viewer** so the audience sees what the facilitator chooses.

Builds on the gated Admin shell and the server's match-control endpoints.

## Acceptance criteria

- [ ] A facilitator can start a match specifying live/headless, field, seed, and length.
- [ ] A running live match can be paused, resumed, retimed (speed), and aborted from the Admin.
- [ ] The facilitator can push a selected match to the projector Viewer.
- [ ] All actions go through the password-gated controller API.

## Blocked by

- `08-admin-auth-health-dashboard.md`
