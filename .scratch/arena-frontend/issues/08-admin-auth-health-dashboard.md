# Admin auth & bot health dashboard

Status: ready-for-agent
Type: AFK
User stories: 13, 14

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

The foundation of the **Admin** console: a plain TypeScript tables-and-forms app on the server's
password-gated controller API.

- **Sign in** with the facilitator password so only the facilitator can control the event;
  unauthorised access is rejected.
- A **bot health dashboard** listing every connected WS Bot and uploaded WASM Bot with health:
  connected state, last-seen, skipped ticks, crashes, and log output.

This is the gated shell the other Admin slices (management, match control, ladder) build on.

## Acceptance criteria

- [ ] The Admin requires the facilitator password to sign in and rejects bad/absent credentials.
- [ ] Authenticated requests reach the controller API; unauthenticated ones are refused.
- [ ] The dashboard lists all WS and WASM bots with connected/last-seen/skipped-ticks/crashes/logs.
- [ ] Health data updates as the server reports it.
- [ ] Built as framework-free DOM tables/forms per the PRD (no DOM-driven test requirement).

## Blocked by

None - can start immediately.
