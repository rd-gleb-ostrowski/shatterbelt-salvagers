# Arena bot connection model: outbound WS, else uploaded WASM, else default

The Arena is centrally hosted, but participants cannot host reachable servers — their bots
run on localhost or are uploaded. So a bot connects in one of three ways, resolved in
priority order per slot: (1) **WS Bot** — the bot process dials _outbound_ to the Arena's
WebSocket endpoint (NAT-friendly, bot initiates); (2) **WASM Bot** — a compiled WebAssembly
artifact uploaded to and run in-process by the Arena (sandboxed, no process or network, and
the artifact is what gets handed over in the afternoon brownfield swap); (3) **Default Bot** —
the built-in fallback so every slot always plays.

## Considered Options

- **HTTP request/response per turn (Battlesnake-style)** — rejected: the hosted Arena would
  have to call _into_ each participant's machine, which NAT/firewalls block. Only works when
  the Arena runs locally.
- **Persistent socket + per-language SDK (Robocode-style)** — the WS tier follows this shape,
  but we don't commit to shipping/maintaining SDKs in N languages.
- **In-process sandboxed scripts (Screeps-style)** — the WASM tier captures the no-hosting
  benefit while WASM's sandbox avoids building a bespoke code sandbox.

## Consequences

- WASM bots need a defined ABI (how match state is passed in and an action returned). To be
  decided in a later ADR.
- WS and WASM bots must be driven by the same per-tick game protocol so a bot's logic ports
  between the two with minimal change.
