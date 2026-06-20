# Minimal WS Bot — Kotlin / JVM

The rawest functional reference for the **WebSocket** bot path against
[`PROTOCOL.md`](../../PROTOCOL.md), in Kotlin on the JVM. No SDK, no helper
layer: the handshake and the per-tick observation→action exchange are written
out inline in `bot.kt`.

Transport uses **only the JDK** — `java.net.http.HttpClient` for the register
POST and its built-in `WebSocket`. The single third-party piece is
[`org.json`](https://github.com/stleary/JSON-java), a tiny JSON parser (the JVM
has none built in).

## What it shows

- `POST /register` with the event password to obtain a token (§4).
- The WS handshake: `welcome` → `join`(token) → `assigned` → `matchStart` (§5),
  driven by a `WebSocket.Listener` that reassembles text frames.
- Each tick: parse the observation JSON (§6) and send a valid action JSON (§8),
  until `matchEnd`.
- A deliberately trivial decision (steer at the nearest relic, thrust, fire) —
  replace `decide()` with a real strategy; nothing else changes.

## Build & run

Needs a JDK (11+) and the Kotlin compiler. Fetch the JSON jar once:

```sh
mkdir -p lib
curl -fsSL -o lib/json.jar \
  https://repo1.maven.org/maven2/org/json/json/20240303/json-20240303.jar

kotlinc bot.kt -cp lib/json.jar -include-runtime -d bot.jar
java -cp bot.jar:lib/json.jar BotKt
```

Configuration via environment variables (defaults shown):

| Variable        | Default                  | Meaning                              |
|-----------------|--------------------------|--------------------------------------|
| `ARENA_HTTP`    | `http://localhost:3000`  | REST base URL (for `/register`)      |
| `ARENA_WS`      | `ws://localhost:3000/ws` | Bot WebSocket endpoint               |
| `ARENA_PASSWORD`| `arena`                  | Shared event password                |
| `ARENA_TEAM`    | `team-kotlin`            | Team name                            |
| `ARENA_TOKEN`   | _(unset)_                | Skip `/register`; use this token     |

Connecting and joining immediately starts a match against the Default Bot, so
running the bot plays a full game and prints the `matchEnd` result.

## Submit

A WS bot runs on your machine and dials out to the Arena, so "submitting" is
just pointing `ARENA_WS` at the event server and running the bot during the
match window.
