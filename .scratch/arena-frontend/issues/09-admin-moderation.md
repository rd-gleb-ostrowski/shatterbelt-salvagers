# Admin moderation: kick & disqualify

Status: ready-for-agent
Type: AFK
User stories: 22

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Give the facilitator the power to remove bad actors from the Admin.

- From the bot health dashboard, **kick or disqualify** a misbehaving bot via the controller API so
  it can't disrupt matches.
- Reflect the bot's removed/disqualified state in the dashboard.

This builds on the Admin auth + health dashboard slice and the server's moderation endpoints.

## Acceptance criteria

- [ ] The facilitator can kick or disqualify a selected bot from the dashboard.
- [ ] The action calls the password-gated controller endpoint and the bot is removed from play.
- [ ] The dashboard reflects the bot's kicked/disqualified state.

## Blocked by

- `08-admin-auth-health-dashboard.md`
