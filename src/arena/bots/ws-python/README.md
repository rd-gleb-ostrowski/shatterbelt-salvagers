# Minimal WS Bot — Python

The rawest functional reference for the **WebSocket** bot path against
[`PROTOCOL.md`](../../PROTOCOL.md). No SDK, no helper layer: the handshake and the
per-tick observation→action exchange are written out inline in `bot.py`.

## What it shows

- `POST /register` with the event password to obtain a token (§4).
- The WS handshake: `welcome` → `join`(token) → `assigned` → `matchStart` (§5).
- Each tick: parse the observation JSON (§6) and send a valid action JSON (§8),
  until `matchEnd`.
- A deliberately trivial decision (steer at the nearest relic, thrust, fire) —
  replace `decide()` with a real strategy; nothing else changes.

## Run

Dependency: the [`websockets`](https://pypi.org/project/websockets/) library.

```sh
pip install websockets        # or: uv pip install websockets
python3 bot.py
```

Configuration via environment variables (defaults shown):

| Variable        | Default                  | Meaning                              |
|-----------------|--------------------------|--------------------------------------|
| `ARENA_HTTP`    | `http://localhost:3000`  | REST base URL (for `/register`)      |
| `ARENA_WS`      | `ws://localhost:3000/ws` | Bot WebSocket endpoint               |
| `ARENA_PASSWORD`| `arena`                  | Shared event password                |
| `ARENA_TEAM`    | `team-py`                | Team name                            |
| `ARENA_TOKEN`   | _(unset)_                | Skip `/register`; use this token     |

Connecting and joining immediately starts a match against the Default Bot, so
running `bot.py` plays a full game and prints the `matchEnd` result.

## Submit

A WS bot runs on your machine and dials out to the Arena, so "submitting" is
just pointing `ARENA_WS` at the event server and running `bot.py` during the
match window.
