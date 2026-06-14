# Arena tech stack: Rust + rapier2d + wasmtime + axum

The Arena is a continuous 2D-physics game server. It's built in **Rust** (the repo's
configured language) using **rapier2d** for physics, **wasmtime** to execute uploaded WASM
Bots in-process, and **axum + tokio-tungstenite** for the outbound WebSocket connections used
by WS Bots and by the browser **viewer** (a read-only WS client that renders matches on a
canvas for the projector).

## Why

- Continuous 2D physics (not a discrete grid) was a deliberate call — the world should feel
  substantial, and a mature library means we don't hand-roll a physics engine.
- **rapier2d** has a cross-platform determinism mode, so matches are fair and **replayable
  from a seed** — important for a tournament and for post-match retros.
- **wasmtime** gives a robust, sandboxed in-process runtime for WASM Bots (see ADR-0001).
- Rust is already the configured toolchain (rust-analyzer, LSP); Node/Python remain available
  for bot starter kits and tooling.

## Considered Options

- **Discrete grid world** — rejected as feeling "too cheap"; depth should come from physics +
  mechanics, not just grid moves.
- **Other physics libs (Box2D ports, Avian/bevy)** — rapier2d preferred for maturity, speed,
  and its determinism mode.
