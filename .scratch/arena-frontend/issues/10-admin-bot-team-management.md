# Admin bot/team management

Status: ready-for-agent
Type: AFK
User stories: 15, 16

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Let the facilitator manage the field of bots from the Admin.

- **Upload or replace** a team's WASM Bot through the controller API.
- **Enable/disable** bots so they can be taken in and out of play.
- **Set or replace the Default Bot** that fills empty slots so they behave well.

Builds on the gated Admin shell and the server's bot-management endpoints.

## Acceptance criteria

- [ ] A facilitator can upload or replace a team's WASM Bot from the Admin.
- [ ] Bots can be enabled/disabled and the change takes effect on the server.
- [ ] The Default Bot can be set or replaced.
- [ ] All actions go through the password-gated controller API.

## Blocked by

- `08-admin-auth-health-dashboard.md`
