// Bare-minimal WS Bot for Shatterbelt Salvagers — TypeScript / Deno reference.
//
// Hides nothing. The full wire protocol (PROTOCOL.md §4, §5, §6, §8) is written
// out inline: register for a token, open a WebSocket, do the
// welcome -> join -> assigned -> matchStart handshake, then each tick parse the
// observation JSON and send back a valid action JSON until matchEnd.
//
// Uses only the Deno/web platform: `fetch` for registration and the built-in
// `WebSocket`. No external dependencies, no SDK. The per-tick decision is a
// deliberately trivial placeholder — the point is to show the bytes on the wire.
//
// Run:
//   deno run --allow-net bot.ts
// Configuration via environment variables (needs --allow-env to read them):
//   deno run --allow-net --allow-env bot.ts

const HTTP = Deno.env.get("ARENA_HTTP") ?? "http://localhost:3000";
const WS = Deno.env.get("ARENA_WS") ?? "ws://localhost:3000/ws";
const PASSWORD = Deno.env.get("ARENA_PASSWORD") ?? "arena";
const TEAM = Deno.env.get("ARENA_TEAM") ?? "team-deno";
// Skip registration and use a pre-issued token if one is supplied.
const PRESET_TOKEN = Deno.env.get("ARENA_TOKEN");

// POST /register {password, team} -> {token} (PROTOCOL.md §4).
async function register(): Promise<string> {
  const res = await fetch(HTTP + "/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password: PASSWORD, team: TEAM }),
  });
  if (!res.ok) throw new Error(`register failed: ${res.status}`);
  return (await res.json()).token;
}

// Turn one observation (§6) into one action (§8). Trivial placeholder: steer at
// the nearest relic, thrust forward, hold the trigger.
function decide(obs: Record<string, any>): Record<string, unknown> {
  const me = obs.self;
  const action: Record<string, unknown> = { type: "action", thrust: 1.0, turn: 0.0, fire: true };

  const relics: any[] = obs.relics ?? [];
  if (relics.length > 0) {
    const px = me.pos.x, py = me.pos.y;
    let nearest = relics[0], best = Infinity;
    for (const r of relics) {
      const d = (r.pos.x - px) ** 2 + (r.pos.y - py) ** 2;
      if (d < best) { best = d; nearest = r; }
    }
    const want = Math.atan2(nearest.pos.y - py, nearest.pos.x - px);
    const diff = Math.atan2(Math.sin(want - me.heading), Math.cos(want - me.heading));
    action.turn = Math.max(-1, Math.min(1, diff));
  }
  return action;
}

async function main(): Promise<void> {
  const token = PRESET_TOKEN ?? (await register());
  console.log(`[bot] token acquired; connecting to ${WS}`);

  const ws = new WebSocket(WS);

  // Resolve when the match ends so the process can exit.
  await new Promise<void>((resolve, reject) => {
    let sessionId = "";
    ws.onerror = () => reject(new Error("websocket error"));
    ws.onclose = () => resolve();

    ws.onmessage = (ev: MessageEvent) => {
      const msg = JSON.parse(ev.data as string);
      switch (msg.type) {
        case "welcome":
          // 1. welcome -> remember the sessionId we must echo back.
          sessionId = msg.sessionId;
          // 2. join -> echo sessionId, present our token + a name.
          ws.send(JSON.stringify({ sessionId, token, name: TEAM }));
          break;
        case "assigned":
          // 3. assigned -> our ship id for this match.
          console.log(`[bot] assigned ship ${msg.shipId}`);
          break;
        case "tick":
          // 4. per tick: parse the observation, send a valid action.
          ws.send(JSON.stringify(decide(msg)));
          break;
        case "matchEnd":
          console.log(`[bot] matchEnd: ${JSON.stringify(msg.results)}`);
          ws.close();
          resolve();
          break;
        // matchStart and anything else need no reply.
      }
    };
  });

  console.log("[bot] done");
}

await main();
